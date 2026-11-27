//! LuckPerms' flat-file storages (`SeparatedConfigurateStorage`, `CombinedConfigurateStorage`,
//! `AbstractConfigurateStorage`, `FileUuidCache`, `FileActionLogger`): YAML, JSON, HOCON and TOML, one file
//! per user, group and track (`users/<uuid>.yml`, `groups/<name>.yml`, `tracks/<name>.yml`), or one file per
//! kind (`users.yml`, `groups.yml`, `tracks.yml`) in `<data dir>/<format>-storage/`, plus `uuidcache.txt` and
//! `actions.txt` next to them. The node layout of the files is Configurate's, so a folder written by LuckPerms
//! on Paper reads here and the other way round.
//!
//! Every change is written through at once (LuckPerms keeps the combined files in memory and saves them at
//! shutdown and on a timer); a combined file that changed on disk since it was read is read again.
//! Names that would leave the storage folder (`..`, a path separator) are refused.

use super::formats::Format;
use super::{HolderKey, KeyMatch, PlayerSave, PlayerRow, Result, Storage};
use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::context::ContextSet;
use crate::horizon_luckperms_core::model::{Group, Track, User, DEFAULT_GROUP};
use crate::horizon_luckperms_core::node::{key, Equality, Kind, Node};
use crate::horizon_luckperms_core::uuid;
use crate::horizon_luckperms_core::yaml::Node as Tree;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Loc {
    Users,
    Groups,
    Tracks,
}

impl Loc {
    fn dir(self) -> &'static str {
        match self {
            Loc::Users => "users",
            Loc::Groups => "groups",
            Loc::Tracks => "tracks",
        }
    }
}

/// A storage method name (`yaml`, `json-combined`, ...) as the format and whether it is one file per kind.
pub fn parse_method(method: &str) -> Option<(Format, bool)> {
    let m = method.trim().to_lowercase();
    let (base, combined) = match m.strip_suffix("-combined") {
        Some(b) => (b.to_owned(), true),
        None => (m, false),
    };
    let format = match base.as_str() {
        "yaml" | "yml" => Format::Yaml,
        "json" | "flatfile" => Format::Json,
        "hocon" => Format::Hocon,
        "toml" => Format::Toml,
        _ => return None,
    };
    Some((format, combined))
}

struct Cached {
    tree: Tree,
    stamp: Option<SystemTime>,
}

pub struct FileStorage {
    format: Format,
    combined: bool,
    root: PathBuf,
    cache: HashMap<Loc, Cached>,
    uuids: BTreeMap<u128, String>,
}

fn io_err(file: &Path, e: std::io::Error) -> String {
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    format!("{name}: {}", e.kind())
}

/// A name that is safe to use as a file name inside the storage folder.
fn safe_name(name: &str) -> Result<()> {
    if name.is_empty() || name == "." || name == ".." || name.chars().any(|c| matches!(c, '/' | '\\' | ':' | '\0') || c.is_control()) {
        return Err(format!("the name {:?} cannot be used as a file name", name.chars().take(40).collect::<String>()));
    }
    Ok(())
}

impl FileStorage {
    /// Where the data lives: `<data_dir>/<yaml|json|hocon|toml>-storage`.
    pub fn new(format: Format, combined: bool, data_dir: &Path) -> Self {
        FileStorage { format, combined, root: data_dir.join(format.dir_name()), cache: HashMap::new(), uuids: BTreeMap::new() }
    }

    fn primary_key(&self) -> &'static str {
        // LuckPerms' JSON files say `primaryGroup`, every other format `primary-group`.
        if self.format == Format::Json { "primaryGroup" } else { "primary-group" }
    }

    fn separated_file(&self, loc: Loc, name: &str) -> Result<PathBuf> {
        safe_name(name)?;
        Ok(self.root.join(loc.dir()).join(format!("{name}{}", self.format.extension())))
    }

    fn combined_file(&self, loc: Loc) -> PathBuf {
        self.root.join(format!("{}{}", loc.dir(), self.format.extension()))
    }

    fn read_tree(&self, file: &Path) -> Result<Tree> {
        let text = std::fs::read_to_string(file).map_err(|e| io_err(file, e))?;
        self.format.parse(&text).map_err(|e| format!("{}: {e}", file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()))
    }

    fn write_file(&self, file: &Path, tree: &Tree) -> Result<()> {
        let tmp = file.with_extension("tmp");
        std::fs::write(&tmp, self.format.write(tree)).map_err(|e| io_err(&tmp, e))?;
        std::fs::rename(&tmp, file).map_err(|e| io_err(file, e))
    }

    /// Makes the cached copy of a combined file current (read again when its modification time changed).
    fn load_combined(&mut self, loc: Loc) -> Result<()> {
        let file = self.combined_file(loc);
        let stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
        if self.cache.get(&loc).is_some_and(|c| c.stamp == stamp) {
            return Ok(());
        }
        let tree = if file.exists() { self.read_tree(&file)? } else { Tree::map() };
        let tree = if tree.is_map() { tree } else { Tree::map() };
        self.cache.insert(loc, Cached { tree, stamp });
        Ok(())
    }

    fn read_entry(&mut self, loc: Loc, name: &str) -> Result<Option<Tree>> {
        if self.combined {
            safe_name(name)?;
            self.load_combined(loc)?;
            Ok(self.cache.get(&loc).and_then(|c| c.tree.get(name)).filter(|n| n.is_map()).cloned())
        } else {
            let file = self.separated_file(loc, name)?;
            if !file.exists() {
                return Ok(None);
            }
            Ok(Some(self.read_tree(&file)?))
        }
    }

    /// Writes an entry; `None` deletes it.
    fn write_entry(&mut self, loc: Loc, name: &str, node: Option<Tree>) -> Result<()> {
        if self.combined {
            safe_name(name)?;
            self.load_combined(loc)?;
            let file = self.combined_file(loc);
            let Some(c) = self.cache.get_mut(&loc) else { return Err("storage cache missing".into()) };
            match node {
                Some(n) => c.tree.set(name, n),
                None => {
                    c.tree.remove(name);
                }
            }
            let tree = c.tree.clone();
            self.write_file(&file, &tree)?;
            let stamp = std::fs::metadata(&file).and_then(|m| m.modified()).ok();
            if let Some(c) = self.cache.get_mut(&loc) {
                c.stamp = stamp;
            }
            Ok(())
        } else {
            let file = self.separated_file(loc, name)?;
            match node {
                Some(n) => self.write_file(&file, &n),
                None => match std::fs::remove_file(&file) {
                    Ok(()) => Ok(()),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                    Err(e) => Err(io_err(&file, e)),
                },
            }
        }
    }

    fn list(&mut self, loc: Loc) -> Result<Vec<String>> {
        if self.combined {
            self.load_combined(loc)?;
            return Ok(self.cache.get(&loc).map(|c| c.tree.keys().map(str::to_owned).collect()).unwrap_or_default());
        }
        let dir = self.root.join(loc.dir());
        let ext = self.format.extension();
        let mut names = Vec::new();
        for e in std::fs::read_dir(&dir).map_err(|e| io_err(&dir, e))? {
            let e = e.map_err(|e| io_err(&dir, e))?;
            let file = e.file_name().to_string_lossy().into_owned();
            if let Some(n) = file.strip_suffix(ext) {
                if !n.is_empty() {
                    names.push(n.to_owned());
                }
            }
        }
        names.sort();
        Ok(names)
    }

    fn uuid_file(&self) -> PathBuf {
        self.root.join("uuidcache.txt")
    }

    fn actions_file(&self) -> PathBuf {
        self.root.join("actions.txt")
    }

    fn save_uuid_cache(&self) -> Result<()> {
        let mut text = String::from("# LuckPerms UUID lookup cache\n");
        for (u, n) in &self.uuids {
            text.push_str(&format!("{}:{n}\n", uuid::format(*u)));
        }
        let file = self.uuid_file();
        std::fs::write(&file, text).map_err(|e| io_err(&file, e))
    }

    /// `FileUuidCache.load`, including the legacy `name=uuid|time` lines.
    fn load_uuid_cache(&mut self) {
        let Ok(text) = std::fs::read_to_string(self.uuid_file()) else { return };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((u, n)) = line.split_once(':') {
                let n = n.split(':').next().unwrap_or("");
                if let (Some(u), false) = (uuid::parse(u), n.is_empty()) {
                    self.uuids.insert(u, n.to_owned());
                }
            } else if let Some((n, rest)) = line.split_once('=') {
                let u = rest.split('|').next().unwrap_or("");
                if let (Some(u), false) = (uuid::parse(u), n.is_empty()) {
                    self.uuids.insert(u, n.to_owned());
                }
            }
        }
    }

    /// `FileActionLogger.init`: the old `actions.json` array is moved into `actions.txt`.
    fn migrate_legacy_actions(&mut self) {
        let legacy = self.root.join("actions.json");
        let Ok(text) = std::fs::read_to_string(&legacy) else { return };
        let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(&text) else { return };
        let mut lines = String::new();
        for v in &items {
            if let Some(a) = Action::from_json(v) {
                lines.push_str(&a.to_json().to_string());
                lines.push('\n');
            }
        }
        let ok = std::fs::OpenOptions::new().create(true).append(true).open(self.actions_file()).and_then(|mut f| f.write_all(lines.as_bytes())).is_ok();
        if ok {
            let _ = std::fs::remove_file(&legacy);
        }
    }

    fn holder_nodes(&mut self, loc: Loc, name: &str) -> Vec<Node> {
        match self.read_entry(loc, name) {
            Ok(Some(t)) => read_nodes(&t),
            Ok(None) => Vec::new(),
            Err(e) => {
                eprintln!("[LuckPerms] could not read {name} while searching: {e}");
                Vec::new()
            }
        }
    }

    fn holder_tree(&self, loc: Loc, name: &str) -> Tree {
        let mut t = Tree::map();
        // The separated files name their holder, the combined ones are keyed by it.
        if !self.combined {
            t.set(if loc == Loc::Users { "uuid" } else { "name" }, Tree::str(name));
        }
        t
    }
}

// ---- nodes <-> tree --------------------------------------------------------------------------

/// `AbstractConfigurateStorage.parseNode`: a map with one entry whose key is no field name is `key: attributes`,
/// else the map itself holds the key field and the attributes.
fn parse_node<'a>(node: &'a Tree, key_field: &str) -> Option<(String, &'a Tree)> {
    let children = node.entries();
    if children.is_empty() {
        return None;
    }
    if children.len() == 1 {
        let e = &children[0];
        if e.key != key_field && !e.key.is_empty() {
            return Some((e.key.clone(), &e.value));
        }
    }
    let appended = node.get(key_field)?;
    Some((appended.as_str().unwrap_or("").to_owned(), node))
}

/// A string that is a string in the file (not a number or a boolean), as `Types::strictAsString` sees it.
fn strict_str(n: &Tree) -> Option<&str> {
    if matches!(n, Tree::Scalar { .. }) && n.tag() == "str" { n.as_str() } else { None }
}

fn read_contexts(attrs: &Tree) -> ContextSet {
    let mut set = ContextSet::new();
    if let Some(map) = attrs.get("context").filter(|c| c.is_map()) {
        for e in map.entries() {
            if e.value.is_seq() {
                for v in e.value.items() {
                    if let Some(s) = v.as_str() {
                        set.add(&e.key, s);
                    }
                }
            } else if let Some(s) = e.value.as_str() {
                set.add(&e.key, s);
            }
        }
    }
    set.add("server", attrs.get("server").and_then(Tree::as_str).unwrap_or("global"));
    set.add("world", attrs.get("world").and_then(Tree::as_str).unwrap_or("global"));
    set
}

fn expiry_of(attrs: &Tree) -> i64 {
    attrs.get("expiry").and_then(Tree::as_i64).unwrap_or(0).max(0)
}

fn push_unique(nodes: &mut Vec<Node>, n: Node) {
    if !nodes.iter().any(|o| o.equals(&n, Equality::Exact)) {
        nodes.push(n);
    }
}

/// `AbstractConfigurateStorage.readNodes`: the `permissions`, `parents`, `prefixes`, `suffixes` and `meta` lists.
pub(crate) fn read_nodes(data: &Tree) -> Vec<Node> {
    let mut nodes = Vec::new();
    let list = |name: &str| data.get(name).map(|n| n.items().to_vec()).unwrap_or_default();
    for appended in list("permissions") {
        if let Some(plain) = strict_str(&appended).filter(|s| !s.is_empty()) {
            push_unique(&mut nodes, Node::new(plain, true, 0, ContextSet::new()));
            continue;
        }
        let Some((k, attrs)) = parse_node(&appended, "permission") else { continue };
        if k.is_empty() {
            continue;
        }
        let value = attrs.get("value").and_then(Tree::as_bool).unwrap_or(true);
        push_unique(&mut nodes, Node::new(k, value, expiry_of(attrs), read_contexts(attrs)));
    }
    for appended in list("parents") {
        if let Some(plain) = strict_str(&appended) {
            push_unique(&mut nodes, Node::new(key::group(plain), true, 0, ContextSet::new()));
            continue;
        }
        let Some((k, attrs)) = parse_node(&appended, "group") else { continue };
        if k.is_empty() {
            continue;
        }
        push_unique(&mut nodes, Node::new(key::group(&k), true, expiry_of(attrs), read_contexts(attrs)));
    }
    for (name, prefix) in [("prefixes", true), ("suffixes", false)] {
        for appended in list(name) {
            let Some((k, attrs)) = parse_node(&appended, if prefix { "prefix" } else { "suffix" }) else { continue };
            let priority = attrs.get("priority").and_then(Tree::as_i64).unwrap_or(0) as i32;
            let node_key = if prefix { key::prefix(priority, &k) } else { key::suffix(priority, &k) };
            push_unique(&mut nodes, Node::new(node_key, true, expiry_of(attrs), read_contexts(attrs)));
        }
    }
    for appended in list("meta") {
        let Some((k, attrs)) = parse_node(&appended, "key") else { continue };
        if k.is_empty() {
            continue;
        }
        let value = attrs.get("value").and_then(Tree::as_str).unwrap_or("null").to_owned();
        push_unique(&mut nodes, Node::new(key::meta(&k, &value), true, expiry_of(attrs), read_contexts(attrs)));
    }
    nodes
}

fn contexts_tree(c: &ContextSet) -> Tree {
    let mut by_key: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (k, v) in c.pairs() {
        by_key.entry(k.to_owned()).or_default().push(v.to_owned());
    }
    let mut t = Tree::map();
    for (k, mut vs) in by_key {
        match vs.len() {
            0 => {}
            1 => t.set(&k, Tree::str(&vs.remove(0))),
            _ => t.set(&k, Tree::seq(vs.iter().map(|v| Tree::str(v)).collect())),
        }
    }
    t
}

/// `writeAttributesTo`: the value (for permissions only), the expiry and the contexts.
fn attributes(n: &Node, write_value: bool) -> Tree {
    let mut a = Tree::map();
    if write_value {
        a.set("value", Tree::bool(n.value));
    }
    if n.has_expiry() {
        a.set("expiry", Tree::int(n.expiry));
    }
    if !n.contexts.is_empty() {
        a.set("context", contexts_tree(&n.contexts));
    }
    a
}

/// `appendNode`: YAML writes `- key: {attributes}`, the other formats one map with the key field.
fn appended(format: Format, k: &str, attrs: Tree, key_field: &str) -> Tree {
    let mut m = Tree::map();
    if format == Format::Yaml && !k.is_empty() {
        m.set(k, attrs);
    } else {
        m.set(key_field, Tree::str(k));
        for e in attrs.entries() {
            m.set(&e.key, e.value.clone());
        }
    }
    m
}

/// `AbstractConfigurateStorage.writeNodes`. `always_permissions`: the combined files keep an empty list.
fn write_nodes(to: &mut Tree, nodes: &[Node], format: Format, always_permissions: bool) {
    let (mut permissions, mut parents, mut prefixes, mut suffixes, mut meta) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for n in nodes {
        let plain = n.value && !n.has_expiry() && n.contexts.is_empty();
        if format == Format::Yaml && plain {
            if let Kind::Inheritance(g) = &n.kind {
                parents.push(Tree::str(g));
                continue;
            }
            if !n.kind.is_meta_or_chat_meta() {
                permissions.push(Tree::str(&n.key));
                continue;
            }
        }
        match &n.kind {
            Kind::Prefix(p, v) | Kind::Suffix(p, v) if n.value => {
                let mut a = Tree::map();
                a.set("priority", Tree::int(*p as i64));
                for e in attributes(n, false).entries() {
                    a.set(&e.key, e.value.clone());
                }
                if matches!(n.kind, Kind::Prefix(..)) {
                    prefixes.push(appended(format, v, a, "prefix"));
                } else {
                    suffixes.push(appended(format, v, a, "suffix"));
                }
            }
            Kind::Meta(k, v) if n.value => {
                let mut a = Tree::map();
                a.set("value", Tree::str(v));
                for e in attributes(n, false).entries() {
                    a.set(&e.key, e.value.clone());
                }
                meta.push(appended(format, k, a, "key"));
            }
            Kind::Inheritance(g) if n.value => parents.push(appended(format, g, attributes(n, false), "group")),
            _ => permissions.push(appended(format, &n.key, attributes(n, true), "permission")),
        }
    }
    let mut section = |name: &str, items: Vec<Tree>, force: bool| {
        if !items.is_empty() || force {
            to.set(name, Tree::seq(items));
        } else {
            to.remove(name);
        }
    };
    section("permissions", permissions, always_permissions);
    section("parents", parents, false);
    section("prefixes", prefixes, false);
    section("suffixes", suffixes, false);
    section("meta", meta, false);
}

// ---- the Storage -----------------------------------------------------------------------------

impl Storage for FileStorage {
    fn name(&self) -> &'static str {
        match (self.format, self.combined) {
            (Format::Yaml, false) => "YAML",
            (Format::Yaml, true) => "YAML Combined",
            (Format::Json, false) => "JSON",
            (Format::Json, true) => "JSON Combined",
            (Format::Hocon, false) => "HOCON",
            (Format::Hocon, true) => "HOCON Combined",
            (Format::Toml, false) => "TOML",
            (Format::Toml, true) => "TOML Combined",
        }
    }

    fn init(&mut self) -> Result<()> {
        std::fs::create_dir_all(&self.root).map_err(|e| io_err(&self.root, e))?;
        if !self.combined {
            for loc in [Loc::Users, Loc::Groups, Loc::Tracks] {
                let dir = self.root.join(loc.dir());
                std::fs::create_dir_all(&dir).map_err(|e| io_err(&dir, e))?;
            }
        }
        self.load_uuid_cache();
        self.migrate_legacy_actions();
        Ok(())
    }

    fn sql_messenger(&self) -> bool {
        false
    }

    fn load_user(&mut self, id: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let Some(t) = self.read_entry(Loc::Users, &uuid::format(id))? else { return Ok((Vec::new(), None)) };
        let name = t.get("name").and_then(Tree::as_str).unwrap_or("null").to_owned();
        let primary = t.get(self.primary_key()).and_then(Tree::as_str).unwrap_or("").to_owned();
        Ok((read_nodes(&t), Some(PlayerRow { username: name, primary_group: primary })))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let name = uuid::format(user.uuid);
        if !user.is_non_default() {
            return self.write_entry(Loc::Users, &name, None);
        }
        let mut t = self.holder_tree(Loc::Users, &name);
        t.set("name", Tree::str(user.username.as_deref().unwrap_or("null")));
        let primary = if user.primary_group.is_empty() { DEFAULT_GROUP } else { user.primary_group.as_str() };
        t.set(self.primary_key(), Tree::str(primary));
        write_nodes(&mut t, &user.data.nodes, self.format, self.combined);
        self.write_entry(Loc::Users, &name, Some(t))
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        let mut out = Vec::new();
        for name in self.list(Loc::Groups)? {
            if let Some(g) = self.load_group(&name)? {
                out.push(g);
            }
        }
        Ok(out)
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        Ok(self.read_entry(Loc::Groups, name)?.map(|t| {
            let mut g = Group::new(name);
            g.data.nodes = read_nodes(&t);
            g
        }))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        let name = name.to_lowercase();
        if self.read_entry(Loc::Groups, &name)?.is_some() {
            return Ok(());
        }
        let mut t = self.holder_tree(Loc::Groups, &name);
        write_nodes(&mut t, &[], self.format, self.combined);
        self.write_entry(Loc::Groups, &name, Some(t))
    }

    fn save_group(&mut self, group: &Group, _before: &[Node]) -> Result<()> {
        let mut t = self.holder_tree(Loc::Groups, &group.name);
        write_nodes(&mut t, &group.data.nodes, self.format, self.combined);
        self.write_entry(Loc::Groups, &group.name, Some(t))
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.write_entry(Loc::Groups, name, None)
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        for name in self.list(Loc::Tracks)? {
            if let Some(t) = self.read_entry(Loc::Tracks, &name)? {
                let groups = t.get("groups").map(|g| g.str_list()).unwrap_or_default();
                out.push(Track { name: name.to_lowercase(), groups: groups.into_iter().map(|g| g.to_lowercase()).collect() });
            }
        }
        Ok(out)
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        let mut t = self.holder_tree(Loc::Tracks, &track.name);
        t.set("groups", Tree::seq(track.groups.iter().map(|g| Tree::str(g)).collect()));
        self.write_entry(Loc::Tracks, &track.name, Some(t))
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.write_entry(Loc::Tracks, name, None)
    }

    fn save_player_data(&mut self, id: u128, username: &str) -> Result<PlayerSave> {
        let old = self.uuids.insert(id, username.to_owned());
        let mut r = PlayerSave::default();
        match &old {
            None => r.new_player = true,
            Some(o) if !o.eq_ignore_ascii_case(username) => r.username_changed = Some(o.clone()),
            _ => {}
        }
        let conflicts: Vec<u128> = self.uuids.iter().filter(|(u, n)| **u != id && n.eq_ignore_ascii_case(username)).map(|(u, _)| *u).collect();
        for c in &conflicts {
            self.uuids.remove(c);
        }
        r.other_uuids = conflicts;
        if r.new_player || r.username_changed.is_some() || !r.other_uuids.is_empty() || old.as_deref() != Some(username) {
            self.save_uuid_cache()?;
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        Ok(self.uuids.iter().find(|(_, n)| n.eq_ignore_ascii_case(username)).map(|(u, _)| *u))
    }

    fn name_of(&mut self, id: u128) -> Result<Option<String>> {
        Ok(self.uuids.get(&id).cloned())
    }

    fn log_action(&mut self, action: &Action) -> Result<()> {
        let file = self.actions_file();
        let mut line = action.to_json().to_string();
        line.push('\n');
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .and_then(|mut f| f.write_all(line.as_bytes()))
            .map_err(|e| io_err(&file, e))
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let file = self.actions_file();
        let text = match std::fs::read_to_string(&file) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_err(&file, e)),
        };
        let mut all: Vec<Action> = text
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| Action::from_json(&v))
            .filter(|a| target.is_none_or(|(t, n)| a.target_type == t && (a.target_name.eq_ignore_ascii_case(n) || a.target_uuid.map(uuid::format).as_deref() == Some(n))))
            .collect();
        all.sort_by_key(|a| a.timestamp);
        all.reverse();
        all.truncate(limit);
        Ok(all)
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        let mut out = Vec::new();
        for name in self.list(Loc::Users)? {
            let Some(u) = uuid::parse(&name) else { continue };
            for n in self.holder_nodes(Loc::Users, &name).into_iter().filter(|n| key.matches(&n.key)) {
                out.push((HolderKey::User(u), n));
            }
        }
        for name in self.list(Loc::Groups)? {
            for n in self.holder_nodes(Loc::Groups, &name).into_iter().filter(|n| key.matches(&n.key)) {
                out.push((HolderKey::Group(name.to_lowercase()), n));
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        let ids: BTreeSet<u128> = self.list(Loc::Users)?.iter().filter_map(|n| uuid::parse(n)).collect();
        Ok(ids.into_iter().collect())
    }

    fn messenger_init(&mut self) -> Result<i64> {
        Err("file storage has no SQL messenger".into())
    }

    fn messenger_send(&mut self, _message: &str) -> Result<()> {
        Err("file storage has no SQL messenger".into())
    }

    fn messenger_poll(&mut self, _last_id: i64) -> Result<Vec<(i64, String)>> {
        Ok(Vec::new())
    }

    fn messenger_housekeep(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const ALICE: u128 = 0x069a79f444e94726a5befca90e38aaf5;
    const BOB: u128 = 0xb0b;

    /// A fresh folder under the system's temp dir; removed when dropped.
    pub(crate) struct Dir(pub PathBuf);

    impl Dir {
        pub(crate) fn new() -> Dir {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!("lp-filestorage-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) const METHODS: &[&str] = &["yaml", "json", "hocon", "toml", "yaml-combined", "json-combined", "hocon-combined", "toml-combined"];

    pub(crate) fn open(method: &str, dir: &Path) -> FileStorage {
        let (f, c) = parse_method(method).unwrap();
        let mut s = FileStorage::new(f, c, dir);
        s.init().unwrap();
        s
    }

    fn sorted(mut v: Vec<Node>) -> Vec<(String, bool, i64, String)> {
        v.sort_by(|a, b| a.key.cmp(&b.key).then(a.expiry.cmp(&b.expiry)));
        v.into_iter().map(|n| (n.key, n.value, n.expiry, n.contexts.to_json())).collect()
    }

    /// One node of every kind, with the attribute combinations the files have to carry.
    pub(crate) fn every_kind() -> Vec<Node> {
        let ctx = ContextSet::of(&[("server", "survival"), ("world", "nether"), ("gamemode", "creative"), ("gamemode", "survival")]);
        vec![
            Node::permanent("essentials.fly", true, ContextSet::new()),
            Node::permanent("essentials.home", false, ContextSet::new()),
            Node::permanent("*", true, ContextSet::new()),
            Node::new("worldedit.*", true, 4_102_444_800, ContextSet::new()),
            Node::permanent("essentials.speed", true, ctx.clone()),
            Node::permanent("group.admin", true, ContextSet::new()),
            Node::new("group.mod", true, 4_102_444_800, ctx.clone()),
            Node::permanent("group.vip", false, ContextSet::new()),
            Node::permanent(key::prefix(100, "&c[Admin.Boss] "), true, ContextSet::new()),
            Node::permanent(key::suffix(5, "&7!"), true, ctx.clone()),
            Node::permanent(key::meta("color", "red.dark"), true, ContextSet::new()),
            Node::permanent(key::meta("level", "7"), false, ContextSet::new()),
            Node::permanent(key::weight(50), true, ContextSet::new()),
            Node::permanent(key::display_name("The Admins"), true, ContextSet::new()),
            Node::permanent("r=essentials\\..*", true, ContextSet::new()),
        ]
    }

    #[test]
    fn every_method_round_trips_users_groups_tracks_and_names() {
        for method in METHODS {
            let d = Dir::new();
            let mut s = open(method, &d.0);
            let mut u = User::new(ALICE);
            u.username = Some("Alice".into());
            u.primary_group = "admin".into();
            u.data.nodes = every_kind();
            s.save_user(&u).unwrap_or_else(|e| panic!("{method}: {e}"));
            let (nodes, row) = s.load_user(ALICE).unwrap_or_else(|e| panic!("{method}: {e}"));
            assert_eq!(sorted(nodes), sorted(every_kind()), "{method}");
            assert_eq!(row, Some(PlayerRow { username: "Alice".into(), primary_group: "admin".into() }), "{method}");
            assert_eq!(s.load_user(BOB).unwrap(), (Vec::new(), None), "{method}");
            assert_eq!(s.unique_users().unwrap(), vec![ALICE], "{method}");

            s.create_group("default").unwrap();
            let mut g = Group::new("admin");
            g.data.nodes = every_kind();
            s.save_group(&g, &[]).unwrap();
            let mut groups: Vec<String> = s.load_all_groups().unwrap().into_iter().map(|g| g.name).collect();
            groups.sort();
            assert_eq!(groups, vec!["admin", "default"], "{method}");
            assert_eq!(sorted(s.load_group("admin").unwrap().unwrap().data.nodes), sorted(every_kind()), "{method}");
            assert!(s.load_group("nope").unwrap().is_none(), "{method}");
            s.save_track(&Track { name: "staff".into(), groups: vec!["default".into(), "admin".into()] }).unwrap();
            assert_eq!(s.load_all_tracks().unwrap(), vec![Track { name: "staff".into(), groups: vec!["default".into(), "admin".into()] }], "{method}");

            // Everything survives a restart of the server (a new object on the same folder).
            drop(s);
            let mut s = open(method, &d.0);
            assert_eq!(sorted(s.load_user(ALICE).unwrap().0), sorted(every_kind()), "{method} after reopen");
            assert_eq!(s.load_all_groups().unwrap().len(), 2, "{method} after reopen");

            s.delete_group("admin").unwrap();
            s.delete_track("staff").unwrap();
            assert!(s.load_group("admin").unwrap().is_none(), "{method}");
            assert!(s.load_all_tracks().unwrap().is_empty(), "{method}");
        }
    }

    #[test]
    fn a_user_with_only_the_default_group_has_no_file() {
        for method in METHODS {
            let d = Dir::new();
            let mut s = open(method, &d.0);
            let mut u = User::new(ALICE);
            u.data.nodes = every_kind();
            s.save_user(&u).unwrap();
            assert_eq!(s.unique_users().unwrap(), vec![ALICE], "{method}");
            let mut plain = User::new(ALICE);
            plain.give_default_if_needed();
            assert!(!plain.is_non_default());
            s.save_user(&plain).unwrap();
            assert!(s.unique_users().unwrap().is_empty(), "{method}");
            assert_eq!(s.load_user(ALICE).unwrap().0.len(), 0, "{method}");
        }
    }

    #[test]
    fn search_finds_nodes_of_users_and_groups() {
        for method in METHODS {
            let d = Dir::new();
            let mut s = open(method, &d.0);
            let mut u = User::new(BOB);
            u.data.nodes = vec![Node::permanent("essentials.fly", true, ContextSet::new()), Node::permanent("a.b", true, ContextSet::new())];
            s.save_user(&u).unwrap();
            let mut g = Group::new("vip");
            g.data.nodes = vec![Node::permanent("essentials.fly", false, ContextSet::new())];
            s.save_group(&g, &[]).unwrap();
            let found = s.search(&KeyMatch::Exact("essentials.fly".into())).unwrap();
            assert_eq!(found.len(), 2, "{method}");
            assert_eq!(found[0].0, HolderKey::User(BOB));
            assert_eq!(found[1].0, HolderKey::Group("vip".into()));
            assert_eq!(s.search(&KeyMatch::Like("essentials.%".into())).unwrap().len(), 2);
            assert_eq!(s.search(&KeyMatch::NotEqual("essentials.fly".into())).unwrap().len(), 1);
        }
    }

    #[test]
    fn names_and_the_uuid_cache_behave_like_the_original() {
        let d = Dir::new();
        let mut s = open("yaml", &d.0);
        assert!(s.save_player_data(ALICE, "Alice").unwrap().new_player);
        assert_eq!(s.save_player_data(ALICE, "Alicia").unwrap().username_changed.as_deref(), Some("Alice"));
        let r = s.save_player_data(BOB, "ALICIA").unwrap();
        assert_eq!(r.other_uuids, vec![ALICE], "the old holder of the name loses it");
        assert_eq!(s.uuid_of("alicia").unwrap(), Some(BOB));
        assert_eq!(s.name_of(BOB).unwrap().as_deref(), Some("ALICIA"));
        assert_eq!(s.name_of(ALICE).unwrap(), None);
        drop(s);
        let mut s = open("yaml", &d.0);
        assert_eq!(s.uuid_of("Alicia").unwrap(), Some(BOB), "uuidcache.txt is read at start");
        let text = std::fs::read_to_string(d.0.join("yaml-storage/uuidcache.txt")).unwrap();
        assert!(text.starts_with("# LuckPerms UUID lookup cache\n"));
        assert!(text.contains("00000000-0000-0000-0000-000000000b0b:ALICIA"), "{text}");
    }

    #[test]
    fn legacy_uuid_cache_lines_are_read() {
        let d = Dir::new();
        std::fs::create_dir_all(d.0.join("json-storage")).unwrap();
        std::fs::write(d.0.join("json-storage/uuidcache.txt"), "# c\nolduser=069a79f4-44e9-4726-a5be-fca90e38aaf5|12345\n\n00000000-0000-0000-0000-000000000b0b:x\n").unwrap();
        let mut s = open("json", &d.0);
        assert_eq!(s.uuid_of("OldUser").unwrap(), Some(ALICE));
        assert_eq!(s.name_of(BOB).unwrap().as_deref(), Some("x"));
    }

    fn action(t: i64, desc: &str, ty: TargetType, name: &str) -> Action {
        Action { timestamp: t, source_uuid: ALICE, source_name: "Alice".into(), target_type: ty, target_uuid: None, target_name: name.into(), description: desc.into() }
    }

    #[test]
    fn the_action_log_is_newest_first_and_filtered() {
        let d = Dir::new();
        let mut s = open("hocon", &d.0);
        s.log_action(&action(10, "one", TargetType::Group, "admin")).unwrap();
        s.log_action(&action(30, "three", TargetType::Group, "mod")).unwrap();
        s.log_action(&action(20, "two", TargetType::Track, "staff")).unwrap();
        let all = s.actions(10, None).unwrap();
        assert_eq!(all.iter().map(|a| a.description.as_str()).collect::<Vec<_>>(), vec!["three", "two", "one"]);
        assert_eq!(s.actions(2, None).unwrap().len(), 2);
        let only = s.actions(10, Some((TargetType::Group, "ADMIN"))).unwrap();
        assert_eq!(only.len(), 1);
        assert_eq!(only[0].description, "one");
        // The log is JSON lines, one per action.
        let text = std::fs::read_to_string(d.0.join("hocon-storage/actions.txt")).unwrap();
        assert_eq!(text.lines().count(), 3);
        assert!(serde_json::from_str::<serde_json::Value>(text.lines().next().unwrap()).is_ok());
    }

    #[test]
    fn a_legacy_actions_json_moves_into_the_log() {
        let d = Dir::new();
        std::fs::create_dir_all(d.0.join("yaml-storage")).unwrap();
        let line = action(7, "legacy", TargetType::Group, "g").to_json();
        std::fs::write(d.0.join("yaml-storage/actions.json"), serde_json::Value::Array(vec![line]).to_string()).unwrap();
        let mut s = open("yaml", &d.0);
        assert_eq!(s.actions(10, None).unwrap().len(), 1);
        assert!(!d.0.join("yaml-storage/actions.json").exists());
    }

    #[test]
    fn a_yaml_folder_written_by_luckperms_is_read() {
        let d = Dir::new();
        std::fs::create_dir_all(d.0.join("yaml-storage/users")).unwrap();
        std::fs::create_dir_all(d.0.join("yaml-storage/groups")).unwrap();
        std::fs::create_dir_all(d.0.join("yaml-storage/tracks")).unwrap();
        std::fs::write(
            d.0.join("yaml-storage/users/069a79f4-44e9-4726-a5be-fca90e38aaf5.yml"),
            "uuid: 069a79f4-44e9-4726-a5be-fca90e38aaf5\nname: Notch\nprimary-group: admin\npermissions:\n- essentials.fly\n- essentials.home:\n    value: false\n    context:\n      world: nether\n- worldedit.*:\n    expiry: 4102444800\n    server: lobby\nparents:\n- default\n- admin:\n    expiry: 4102444800\n    context:\n      world: [a, b]\nprefixes:\n- '&c[Admin] ':\n    priority: 100\nmeta:\n- color:\n    value: red\n",
        )
        .unwrap();
        std::fs::write(d.0.join("yaml-storage/groups/default.yml"), "name: default\npermissions:\n- group.default.thing\n").unwrap();
        std::fs::write(d.0.join("yaml-storage/tracks/staff.yml"), "name: staff\ngroups:\n- default\n- admin\n").unwrap();
        let mut s = open("yaml", &d.0);
        let (nodes, row) = s.load_user(ALICE).unwrap();
        assert_eq!(row, Some(PlayerRow { username: "Notch".into(), primary_group: "admin".into() }));
        let want = sorted(vec![
            Node::permanent("essentials.fly", true, ContextSet::new()),
            Node::permanent("essentials.home", false, ContextSet::of(&[("world", "nether")])),
            Node::new("worldedit.*", true, 4_102_444_800, ContextSet::of(&[("server", "lobby")])),
            Node::permanent("group.default", true, ContextSet::new()),
            Node::new("group.admin", true, 4_102_444_800, ContextSet::of(&[("world", "a"), ("world", "b")])),
            Node::permanent(key::prefix(100, "&c[Admin] "), true, ContextSet::new()),
            Node::permanent(key::meta("color", "red"), true, ContextSet::new()),
        ]);
        assert_eq!(sorted(nodes), want);
        assert_eq!(s.load_all_tracks().unwrap()[0].groups, vec!["default", "admin"]);
        assert_eq!(s.load_all_groups().unwrap()[0].data.nodes.len(), 1);
    }

    #[test]
    fn the_files_are_written_in_the_original_layout() {
        let d = Dir::new();
        let mut s = open("yaml", &d.0);
        let mut u = User::new(ALICE);
        u.username = Some("Alice".into());
        u.data.nodes = vec![
            Node::permanent("essentials.fly", true, ContextSet::new()),
            Node::permanent("essentials.home", false, ContextSet::of(&[("world", "nether")])),
            Node::permanent("group.default", true, ContextSet::new()),
            Node::permanent(key::meta("color", "red"), true, ContextSet::new()),
        ];
        s.save_user(&u).unwrap();
        let text = std::fs::read_to_string(d.0.join("yaml-storage/users/069a79f4-44e9-4726-a5be-fca90e38aaf5.yml")).unwrap();
        assert!(text.starts_with("uuid: 069a79f4-44e9-4726-a5be-fca90e38aaf5\nname: Alice\nprimary-group: default\npermissions:\n"), "{text}");
        assert!(text.contains("- essentials.fly\n"), "{text}");
        assert!(text.contains("essentials.home:\n"), "{text}");
        assert!(text.contains("parents:\n- default\n"), "{text}");
        assert!(text.contains("meta:\n- color:\n"), "{text}");
        // JSON says primaryGroup, and writes the key field into the same map.
        let d = Dir::new();
        let mut s = open("json", &d.0);
        s.save_user(&u).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(d.0.join("json-storage/users/069a79f4-44e9-4726-a5be-fca90e38aaf5.json")).unwrap()).unwrap();
        assert_eq!(v["primaryGroup"], "default");
        assert_eq!(v["permissions"][0]["permission"], "essentials.fly");
        assert_eq!(v["permissions"][0]["value"], true);
        assert_eq!(v["parents"][0]["group"], "default");
        assert_eq!(v["meta"][0]["key"], "color");
        assert_eq!(v["meta"][0]["value"], "red");
    }

    #[test]
    fn a_combined_file_keeps_the_other_entries_and_follows_outside_edits() {
        let d = Dir::new();
        let mut s = open("yaml-combined", &d.0);
        let mut a = Group::new("a");
        a.data.nodes = vec![Node::permanent("x.y", true, ContextSet::new())];
        s.save_group(&a, &[]).unwrap();
        let mut b = Group::new("b");
        b.data.nodes = vec![Node::permanent("z", true, ContextSet::new())];
        s.save_group(&b, &[]).unwrap();
        s.delete_group("a").unwrap();
        assert_eq!(s.load_all_groups().unwrap().len(), 1);
        let file = d.0.join("yaml-storage/groups.yml");
        assert!(std::fs::read_to_string(&file).unwrap().starts_with("b:\n"));
        // An edit made outside (another server on a shared folder) is picked up on the next read.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(&file, "b:\n  permissions:\n  - changed.outside\nc:\n  permissions: []\n").unwrap();
        let names: Vec<String> = s.load_all_groups().unwrap().into_iter().map(|g| g.name).collect();
        assert_eq!(names, vec!["b", "c"]);
        assert_eq!(s.load_group("b").unwrap().unwrap().data.nodes[0].key, "changed.outside");
    }

    #[test]
    fn names_that_leave_the_folder_are_refused() {
        let d = Dir::new();
        let mut s = open("yaml", &d.0);
        for bad in ["../evil", "..", "a/b", "a\\b", "", "x\0y"] {
            let mut g = Group::new("tmp");
            g.name = bad.to_owned();
            assert!(s.save_group(&g, &[]).is_err(), "{bad:?}");
            assert!(s.load_group(bad).is_err(), "{bad:?}");
        }
        assert!(!d.0.join("evil.yml").exists());
        assert!(s.save_group(&Group::new("my.group"), &[]).is_ok(), "dots inside a name are fine");
        let mut s = open("yaml-combined", &d.0);
        assert!(s.create_group("a/b").is_err());
    }

    #[test]
    fn a_broken_file_is_an_error_that_names_the_file_not_the_text() {
        let d = Dir::new();
        let mut s = open("json", &d.0);
        std::fs::write(d.0.join("json-storage/groups/bad.json"), "{ \"password\": oops").unwrap();
        let e = s.load_all_groups().unwrap_err();
        assert!(e.starts_with("bad.json: parse error at line"), "{e}");
        assert!(!e.contains("oops") && !e.contains("password"), "{e}");
    }

    #[test]
    fn file_storage_has_no_sql_messenger() {
        let d = Dir::new();
        let s = open("toml", &d.0);
        assert!(!s.sql_messenger());
        assert_eq!(s.name(), "TOML");
    }

    #[test]
    fn method_names() {
        assert_eq!(parse_method("YAML"), Some((Format::Yaml, false)));
        assert_eq!(parse_method("yml"), Some((Format::Yaml, false)));
        assert_eq!(parse_method("flatfile"), Some((Format::Json, false)));
        assert_eq!(parse_method("hocon-combined"), Some((Format::Hocon, true)));
        assert_eq!(parse_method(" TOML-Combined "), Some((Format::Toml, true)));
        assert_eq!(parse_method("mariadb"), None);
        assert_eq!(parse_method("h2"), None);
    }
}
