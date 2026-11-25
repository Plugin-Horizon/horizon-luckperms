//! `/lp verbose`, `/lp tree` and `/lp bulkupdate`.

use super::args::Args;
use super::fmt::{aq, dred, green};
use super::{Ctx, Holder, LABEL};
use crate::horizon_luckperms_core::bytebin;
use crate::horizon_luckperms_core::context::ContextSet;
use crate::horizon_luckperms_core::known;
use crate::horizon_luckperms_core::lang::{self, tr};
use crate::horizon_luckperms_core::model::NodeSet;
use crate::horizon_luckperms_core::node::Node;
use crate::horizon_luckperms_core::runtime::Recipient;
use crate::horizon_luckperms_core::storage::{HolderKey, KeyMatch};
use crate::horizon_luckperms_core::text::{rich, Click};
use crate::horizon_luckperms_core::verbose;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn red(s: &str) -> String {
    format!("\u{a7}c{s}")
}

// ---- verbose ------------------------------------------------------------------------------------

/// A link line: the heading, then the address as a clickable line.
fn link(c: &mut Ctx<'_>, heading: &str, url: &str) {
    c.msg(format!("{}:", tr(heading, 'a', &[])));
    c.raw(rich(&format!("\u{a7}b{url}"), Some(Click::OpenUrl(url.to_owned())), None));
}

/// Gzips `json` and posts it to bytebin; the key, or the failure told to the sender.
pub(super) fn upload(c: &mut Ctx<'_>, json: String) -> Option<String> {
    #[cfg(feature = "net")]
    let body = bytebin::gzip(json.as_bytes());
    #[cfg(not(feature = "net"))]
    let body: Vec<u8> = json.into_bytes();
    if body.len() > bytebin::MAX_UPLOAD {
        c.msg("\u{a7}cThe data is too big to upload.");
        return None;
    }
    let url = c.cfg.bytebin_url.clone();
    match bytebin::with_client(&url, |b| b.post(&body)) {
        Ok(id) => Some(id),
        Err(e) => {
            super::editor::http_failure(c, e);
            None
        }
    }
}

/// `/lp verbose <on|record|off|upload> [filter]`
pub(crate) fn verbose(c: &mut Ctx<'_>, a: Args) {
    let mode = a.get(0).to_lowercase();
    let sender = (c.sender.uuid, c.sender.name.clone(), c.sender.console);
    match mode.as_str() {
        "command" | "cmd" => {
            if a.len() < 3 {
                c.detailed_usage("Verbose", "verbose");
                return;
            }
            let name = a.get(1);
            let lower = name.to_lowercase();
            let target = if lower == "me" || lower == "self" || lower == c.sender.name.to_lowercase() {
                if c.sender.console {
                    c.detailed_usage("Verbose", "verbose");
                    return;
                }
                (c.sender.uuid, c.sender.name.clone())
            } else {
                if !c.has("verbose.command.others") {
                    c.no_permission();
                    return;
                }
                match c.rt.online_players().into_iter().find(|p| p.name.eq_ignore_ascii_case(&name)) {
                    Some(p) => (p.uuid, p.name),
                    None => {
                        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_NOT_ONLINE, &[dred(&name)]);
                        return;
                    }
                }
            };
            let joined = (2..a.len()).map(|i| a.get(i)).collect::<Vec<_>>().join(" ");
            let command = joined.strip_prefix('/').unwrap_or(&joined).to_owned();
            c.msg(format!("{}.", tr(lang::T_COMMAND_VERBOSE_COMMAND_EXEC, 'b', &[green(&target.1), green(&command)])));
            c.rt.queue_verbose_job(crate::horizon_luckperms_core::runtime::VerboseJob {
                sender: c.sender.clone(),
                target: target.0,
                command,
            });
        }
        "on" | "true" | "record" => {
            let filter = (1..a.len()).map(|i| a.get(i)).collect::<Vec<_>>().join(" ");
            let ast = match verbose::compile(&filter) {
                Ok(ast) => ast,
                Err(why) => {
                    c.msg(format!("{}. \u{a7}7({why})", tr(lang::T_COMMAND_VERBOSE_INVALID_FILTER, 'c', &[dred(&filter)])));
                    return;
                }
            };
            let notify = mode != "record";
            let to = if sender.2 { Recipient::Console } else { Recipient::Player(sender.0) };
            verbose::register(to, sender.0, &sender.1, sender.2, ast, &filter, notify);
            let query = if filter.is_empty() { green(lang::T_COMMAND_VERBOSE_QUERY_ANY) } else { green(&filter) };
            let template = if notify { lang::T_COMMAND_VERBOSE_ENABLED } else { lang::T_COMMAND_VERBOSE_ENABLED_RECORDING };
            c.msg(format!("{}.", tr(template, 'b', &[green(lang::T_COMMAND_VERBOSE_ENABLED_TERM), query])));
        }
        "off" | "false" | "paste" | "upload" => {
            let finished = verbose::unregister(sender.0);
            let upload_mode = mode == "paste" || mode == "upload";
            match finished {
                Some(f) if upload_mode => {
                    c.msg(tr(lang::T_COMMAND_VERBOSE_UPLOADING, 'b', &[red(lang::T_COMMAND_VERBOSE_DISABLED_TERM)]));
                    if let Some(id) = upload(c, verbose::upload_json(&f)) {
                        let url = format!("{}{}", c.cfg.verbose_viewer_url, id);
                        link(c, lang::T_COMMAND_VERBOSE_URL, &url);
                    }
                }
                _ => c.msg(format!("{}.", tr(lang::T_COMMAND_VERBOSE_OFF, 'b', &[red(lang::T_COMMAND_VERBOSE_DISABLED_TERM)]))),
            }
        }
        _ => c.detailed_usage("Verbose", "verbose"),
    }
}

/// The lines after a forced command ran (`VERBOSE_OFF_COMMAND`, `VERBOSE_OFF_COMMAND_NO_CHECKS`).
pub fn verbose_done_lines(matched: usize) -> Vec<String> {
    use crate::horizon_luckperms_core::text::prefixed;
    if matched == 0 {
        vec![
            prefixed(tr(lang::T_COMMAND_VERBOSE_COMMAND_NO_CHECKS, 'c', &[])),
            prefixed(tr(lang::T_COMMAND_VERBOSE_COMMAND_POSSIBLY_ASYNC, '7', &[])),
            prefixed(tr(lang::T_COMMAND_VERBOSE_COMMAND_TRY_AGAIN_MANUALLY, '7', &[])),
        ]
    } else {
        vec![prefixed(format!("{}.", tr(lang::T_COMMAND_VERBOSE_COMMAND_EXEC_COMPLETE, 'b', &[])))]
    }
}

// ---- translations -------------------------------------------------------------------------------

/// `/lp translations [install]` (`TranslationsCommand`)
pub(crate) fn translations(c: &mut Ctx<'_>, a: Args) {
    use crate::horizon_luckperms_core::translations as tl;
    c.msg(format!("{}", tr(lang::T_COMMAND_TRANSLATIONS_SEARCHING, 'b', &[])));
    let languages = match tl::with_service(tl::available) {
        Ok(l) => l,
        Err(why) => {
            c.msg(tr(lang::T_COMMAND_TRANSLATIONS_SEARCHING_ERROR, 'c', &[]));
            eprintln!("[LuckPerms] Unable to obtain a list of available translations: {why}");
            return;
        }
    };
    if a.len() >= 1 && a.get(0).eq_ignore_ascii_case("install") {
        c.msg(tr(lang::T_COMMAND_TRANSLATIONS_INSTALLING, 'b', &[]));
        let mut lines: Vec<String> = Vec::new();
        tl::with_service(|client| {
            tl::install(client, &languages, |lang, starting| {
                let t = if starting { lang::T_COMMAND_TRANSLATIONS_INSTALLING_SPECIFIC } else { lang::T_COMMAND_TRANSLATIONS_DOWNLOAD_ERROR };
                lines.push(format!("{}", tr(t, if starting { 'b' } else { 'c' }, &[green(&lang.locale)])));
            })
        });
        for l in lines {
            c.msg(l);
        }
        let locale = std::env::var("LUCKPERMS_LOCALE").unwrap_or_else(|_| c.cfg.horizon.locale.clone());
        tl::load(&locale);
        c.msg(tr(lang::T_COMMAND_TRANSLATIONS_INSTALL_COMPLETE, 'b', &[]));
        return;
    }
    c.msg(format!("{}: {}", tr(lang::T_COMMAND_TRANSLATIONS_INSTALLED_TRANSLATIONS, 'b', &[]), super::fmt::string_list(&tl::installed())));
    c.msg(format!("{}:", tr(lang::T_COMMAND_TRANSLATIONS_AVAILABLE_TRANSLATIONS, 'b', &[])));
    let mut sorted = languages;
    sorted.sort_by(|x, y| x.locale.cmp(&y.locale));
    for l in sorted {
        let by = if l.contributors.is_empty() {
            String::new()
        } else {
            format!(" {} \u{a7}7{}", tr(lang::T_COMMAND_TRANSLATIONS_TRANSLATIONS_BY, '3', &[]), l.contributors.join(", "))
        };
        c.raw(format!(
            "\u{a7}3> \u{a7}b{} \u{a7}3- \u{a7}f{} \u{a7}3({}){by}",
            l.locale,
            l.name,
            tr(lang::T_COMMAND_TRANSLATIONS_PERCENT_TRANSLATED, '3', &[format!("\u{a7}b{}", l.progress)])
        ));
    }
    c.raw(String::new());
    c.msg(format!("{}.", tr(lang::T_COMMAND_TRANSLATIONS_DOWNLOAD_PROMPT, 'b', &[green(&format!("/{LABEL} translations install"))])));
    c.msg(tr(lang::T_COMMAND_TRANSLATIONS_DOWNLOAD_OVERRIDE_WARNING, 'c', &[]));
}

// ---- tree ---------------------------------------------------------------------------------------

#[derive(Default)]
struct Tree {
    children: BTreeMap<String, Tree>,
}

impl Tree {
    fn insert(&mut self, node: &str) {
        let mut at = self;
        for part in node.split('.').filter(|p| !p.is_empty()) {
            at = at.children.entry(part.to_owned()).or_default();
        }
    }

    /// `TreeView.findRoot`
    fn find(&self, root: &str) -> Option<&Tree> {
        let mut at = self;
        if root == "." {
            return Some(at);
        }
        for part in root.split('.').filter(|p| !p.is_empty()) {
            at = at.children.get(part)?;
        }
        Some(at)
    }

    /// `ImmutableTreeNode.toJson`
    fn json(&self, prefix: &str) -> Value {
        let mut map = Map::new();
        for (key, child) in &self.children {
            let name = format!("{prefix}{key}");
            let sub = child.json(&format!("{name}."));
            map.insert(name, sub);
        }
        Value::Object(map)
    }

    /// `ImmutableTreeNode.getNodeEndings`, as the full names below this node.
    fn endings(&self, prefix: &str, out: &mut Vec<String>) {
        for (key, child) in &self.children {
            let name = format!("{prefix}{key}");
            out.push(name.clone());
            child.endings(&format!("{name}."), out);
        }
    }
}

/// `/lp tree [scope] [player]`
pub(crate) fn tree(c: &mut Ctx<'_>, a: Args) {
    let mut scope = if a.len() > 0 { a.get(0) } else { ".".to_owned() };
    if scope.is_empty() || scope == "*" {
        scope = ".".to_owned();
    } else if scope != "." && scope.ends_with('.') {
        scope.pop();
    }
    let user = if a.len() > 1 {
        let name = a.get(1);
        let found = crate::horizon_luckperms_core::uuid::parse(&name)
            .and_then(|u| c.rt.online_player(u))
            .or_else(|| c.rt.online_players().into_iter().find(|p| p.name.eq_ignore_ascii_case(&name)));
        match found {
            Some(p) => Some(p),
            None => {
                c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_NOT_ONLINE, &[dred(&name)]);
                return;
            }
        }
    } else {
        None
    };
    let mut root = Tree::default();
    for node in known::all(&c.cfg) {
        root.insert(&node);
    }
    let Some(view) = root.find(&scope).filter(|t| !t.children.is_empty()) else {
        c.bad(lang::T_COMMAND_TREE_EMPTY, &[]);
        return;
    };
    c.msg(tr(lang::T_COMMAND_TREE_START, 'b', &[]));
    let prefix = if scope == "." { String::new() } else { format!("{scope}.") };
    let mut uploader = serde_json::json!({
        "name": c.sender.name,
        "uuid": crate::horizon_luckperms_core::uuid::format(c.sender.uuid),
    });
    if c.sender.console {
        uploader["name"] = Value::String("Console".into());
    }
    let mut metadata = serde_json::json!({
        "time": verbose::date_text(unix_ms()),
        "root": scope,
        "uploader": uploader,
        "platform": "Horizon",
    });
    let mut data = Map::new();
    data.insert("tree".into(), view.json(&prefix));
    if let Some(p) = &user {
        metadata["referenceUser"] = serde_json::json!({ "name": p.name, "uuid": crate::horizon_luckperms_core::uuid::format(p.uuid) });
        let mut ends = Vec::new();
        view.endings(&prefix, &mut ends);
        let mut checks = Map::new();
        for permission in ends {
            let result = match c.rt.check(p.uuid, &permission).as_bool() {
                Some(true) => "true",
                Some(false) => "false",
                None => "undefined",
            };
            checks.insert(permission, Value::String(result.into()));
        }
        data.insert("checkResults".into(), Value::Object(checks));
    }
    let payload = serde_json::json!({ "metadata": metadata, "data": Value::Object(data) });
    if let Some(id) = upload(c, payload.to_string()) {
        let url = format!("{}{}", c.cfg.tree_viewer_url, id);
        link(c, lang::T_COMMAND_TREE_URL, &url);
    }
}

fn unix_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

// ---- bulkupdate ---------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Permission,
    Server,
    World,
}

impl Field {
    fn parse(s: &str) -> Option<Field> {
        match s.to_lowercase().as_str() {
            "permission" => Some(Field::Permission),
            "server" => Some(Field::Server),
            "world" => Some(Field::World),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Field::Permission => "permission",
            Field::Server => "server",
            Field::World => "world",
        }
    }

    /// `BulkUpdateField.getValue`
    fn value(self, n: &Node) -> String {
        match self {
            Field::Permission => n.key.clone(),
            Field::Server => n.contexts.values("server").first().map_or_else(|| "global".to_owned(), |v| (*v).to_owned()),
            Field::World => n.contexts.values("world").first().map_or_else(|| "global".to_owned(), |v| (*v).to_owned()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Comparison {
    Equal,
    NotEqual,
    Similar,
    NotSimilar,
}

impl Comparison {
    fn parse(s: &str) -> Option<Comparison> {
        match s {
            "==" => Some(Comparison::Equal),
            "!=" => Some(Comparison::NotEqual),
            "~~" => Some(Comparison::Similar),
            "!~" => Some(Comparison::NotSimilar),
            _ => None,
        }
    }

    fn symbol(self) -> &'static str {
        match self {
            Comparison::Equal => "==",
            Comparison::NotEqual => "!=",
            Comparison::Similar => "~~",
            Comparison::NotSimilar => "!~",
        }
    }

    /// Case-insensitive, like the database's collation; `~~` is SQL `LIKE`.
    fn holds(self, value: &str, expr: &str) -> bool {
        match self {
            Comparison::Equal => value.eq_ignore_ascii_case(expr),
            Comparison::NotEqual => !value.eq_ignore_ascii_case(expr),
            Comparison::Similar => KeyMatch::Like(expr.to_owned()).matches(value),
            Comparison::NotSimilar => KeyMatch::NotLike(expr.to_owned()).matches(value),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Delete,
    Update(Field, String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Op {
    users: bool,
    groups: bool,
    action: Action,
    filters: Vec<(Field, Comparison, String)>,
    stats: bool,
}

impl Op {
    fn data_name(&self) -> &'static str {
        match (self.users, self.groups) {
            (true, true) => "all",
            (true, false) => "users",
            _ => "groups",
        }
    }

    /// The operation as a readable statement (what `/lp bulkupdate` shows before it asks to confirm).
    fn readable(&self) -> String {
        let mut s = match &self.action {
            Action::Delete => format!("DELETE FROM {}", self.data_name()),
            Action::Update(f, v) => format!("UPDATE {} SET {} = '{v}'", self.data_name(), f.name()),
        };
        for (i, (f, cmp, v)) in self.filters.iter().enumerate() {
            s.push_str(if i == 0 { " WHERE " } else { " AND " });
            s.push_str(&format!("{} {} '{v}'", f.name(), cmp.symbol()));
        }
        s
    }

    fn matches(&self, n: &Node) -> bool {
        self.filters.iter().all(|(f, cmp, v)| cmp.holds(&f.value(n), v))
    }

    /// `UpdateAction.apply`
    fn updated(&self, n: &Node) -> Node {
        let Action::Update(field, value) = &self.action else { return n.clone() };
        match field {
            Field::Permission => Node::new(value.clone(), n.value, n.expiry, n.contexts.clone()),
            Field::Server | Field::World => {
                let mut contexts: ContextSet = n.contexts.clone();
                let key = field.name();
                contexts.remove_key(key);
                if value != "global" {
                    contexts.add(key, value);
                }
                Node::new(n.key.clone(), n.value, n.expiry, contexts)
            }
        }
    }

    /// Applies the operation to one holder's nodes; how many nodes it touched.
    fn apply(&self, data: &mut NodeSet) -> usize {
        let old = std::mem::take(&mut data.nodes);
        let mut touched = 0;
        for n in old {
            if !self.matches(&n) {
                data.nodes.push(n);
                continue;
            }
            touched += 1;
            if matches!(self.action, Action::Update(..)) {
                data.nodes.push(self.updated(&n));
            }
        }
        touched
    }
}

static PENDING: Mutex<Option<HashMap<String, (Op, Instant)>>> = Mutex::new(None);
const PENDING_TTL: Duration = Duration::from_secs(30);

fn pending<R>(f: impl FnOnce(&mut HashMap<String, (Op, Instant)>) -> R) -> R {
    let mut g = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    let map = g.get_or_insert_with(HashMap::new);
    map.retain(|_, (_, at)| at.elapsed() < PENDING_TTL);
    f(map)
}

/// Parses `bulkupdate <type> <delete|update <field> <value>> [constraint...]`; the sender is told what is wrong.
fn parse(c: &mut Ctx<'_>, a: &Args) -> Option<Op> {
    let mut v: Vec<String> = (0..a.len()).map(|i| a.get(i)).collect();
    let stats = match v.iter().position(|x| x == "-s") {
        Some(i) => {
            v.remove(i);
            false
        }
        None => true,
    };
    if v.len() < 2 {
        c.detailed_usage("BulkUpdate", "bulk-update");
        return None;
    }
    let (users, groups) = match v.remove(0).to_lowercase().as_str() {
        "all" => (true, true),
        "users" => (true, false),
        "groups" => (false, true),
        _ => {
            c.bad(lang::T_COMMAND_BULKUPDATE_INVALID_DATA_TYPE, &[aq("'all', 'users' or 'groups'")]);
            return None;
        }
    };
    let action = match v.remove(0).to_lowercase().as_str() {
        "delete" => Action::Delete,
        "update" => {
            if v.len() < 2 {
                c.detailed_usage("BulkUpdate", "bulk-update");
                return None;
            }
            let Some(field) = Field::parse(&v.remove(0)) else {
                c.detailed_usage("BulkUpdate", "bulk-update");
                return None;
            };
            Action::Update(field, v.remove(0))
        }
        _ => {
            c.detailed_usage("BulkUpdate", "bulk-update");
            return None;
        }
    };
    let mut filters = Vec::new();
    for constraint in v {
        let parts: Vec<&str> = constraint.split(' ').collect();
        let field = if parts.len() == 3 { Field::parse(parts[0]) } else { None };
        let Some(field) = field else {
            c.msg(format!(
                "{}. {}.",
                tr(lang::T_COMMAND_BULKUPDATE_INVALID_CONSTRAINT, 'c', &[dred(&constraint)]),
                tr(lang::T_COMMAND_BULKUPDATE_INVALID_CONSTRAINT_FORMAT, 'c', &["\u{a7}f'<field> <comparison operator> <value>'".to_owned()])
            ));
            return None;
        };
        let Some(cmp) = Comparison::parse(parts[1]) else {
            c.msg(format!(
                "{}. {}.",
                tr(lang::T_COMMAND_BULKUPDATE_INVALID_COMPARISON, 'c', &[dred(parts[1])]),
                tr(lang::T_COMMAND_BULKUPDATE_INVALID_COMPARISON_FORMAT, 'c', &["\u{a7}f'==', '!=', '~~', '!~'".to_owned()])
            ));
            return None;
        };
        filters.push((field, cmp, parts[2].to_owned()));
    }
    Some(Op { users, groups, action, filters, stats })
}

/// Runs the operation over the groups and the users in storage, saving each holder that changed.
fn run(c: &mut Ctx<'_>, op: &Op) {
    c.ok(lang::T_COMMAND_BULKUPDATE_STARTING, &[]);
    let (mut nodes, mut users, mut groups) = (0usize, 0usize, 0usize);
    let mut failed = false;
    if op.groups {
        let names: Vec<String> = c.model().groups.keys().cloned().collect();
        for name in names {
            let Some(mut l) = c.load_group(&name) else { continue };
            let n = op.apply(l.h.data_mut());
            if n > 0 {
                nodes += n;
                groups += 1;
                failed |= !c.save(l);
            }
        }
    }
    if op.users {
        let mut ids: Vec<u128> = match c.st.search(&KeyMatch::Like("%".into())) {
            Ok(rows) => rows.into_iter().filter_map(|(h, _)| if let HolderKey::User(u) = h { Some(u) } else { None }).collect(),
            Err(_) => {
                c.bad(lang::T_COMMAND_BULKUPDATE_FAILURE, &[]);
                return;
            }
        };
        ids.sort_unstable();
        ids.dedup();
        for id in ids {
            let Some(mut l) = c.load_user(id) else { continue };
            if !matches!(l.h, Holder::User(_)) {
                continue;
            }
            let n = op.apply(l.h.data_mut());
            if n > 0 {
                nodes += n;
                users += 1;
                failed |= !c.save(l);
            }
        }
    }
    c.out.sync = true;
    if failed {
        c.bad(lang::T_COMMAND_BULKUPDATE_FAILURE, &[]);
        return;
    }
    c.ok(lang::T_COMMAND_BULKUPDATE_SUCCESS, &[]);
    if op.stats {
        for (label, n) in [
            (lang::T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_NODES, nodes),
            (lang::T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_USERS, users),
            (lang::T_COMMAND_BULKUPDATE_SUCCESS_STATISTICS_GROUPS, groups),
        ] {
            c.msg(format!("\u{a7}b{label}: \u{a7}a{n}"));
        }
    }
}

/// `/lp bulkupdate ...` (console only, as in LuckPerms)
pub(crate) fn bulkupdate(c: &mut Ctx<'_>, a: Args) {
    if c.cfg.disable_bulkupdate {
        c.bad(lang::T_COMMAND_BULKUPDATE_DISABLED, &[]);
        return;
    }
    if !c.sender.console {
        c.bad(lang::T_COMMAND_BULKUPDATE_MUST_USE_CONSOLE, &[]);
        return;
    }
    if a.len() == 2 && a.get(0).eq_ignore_ascii_case("confirm") {
        let id = a.get(1);
        match pending(|m| m.remove(&id)) {
            Some((op, _)) => run(c, &op),
            None => c.ok(lang::T_COMMAND_BULKUPDATE_UNKNOWN_ID, &[aq(&id)]),
        }
        return;
    }
    if a.len() < 2 {
        c.detailed_usage("BulkUpdate", "bulk-update");
        return;
    }
    let Some(op) = parse(c, &a) else { return };
    if c.cfg.skip_bulkupdate_confirmation {
        run(c, &op);
        return;
    }
    let id = format!("{:04}", unix_ms().unsigned_abs() % 10_000);
    let readable = op.readable();
    pending(|m| m.insert(id.clone(), (op, Instant::now())));
    c.msg(format!("{}. \u{a7}7(\u{a7}f{readable}\u{a7}7)", tr(lang::T_COMMAND_BULKUPDATE_QUEUED, 'a', &[])));
    c.msg(format!("{}.", tr(lang::T_COMMAND_BULKUPDATE_CONFIRM, 'a', &[aq(&format!("/{LABEL} bulkupdate confirm {id}"))])));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(key: &str, server: Option<&str>) -> Node {
        let mut ctx = ContextSet::new();
        if let Some(s) = server {
            ctx.add("server", s);
        }
        Node::permanent(key, true, ctx)
    }

    fn op(action: Action, filters: Vec<(Field, Comparison, &str)>) -> Op {
        Op { users: true, groups: true, action, filters: filters.into_iter().map(|(f, c, v)| (f, c, v.to_owned())).collect(), stats: true }
    }

    #[test]
    fn delete_removes_the_matching_nodes_only() {
        let mut set = NodeSet { nodes: vec![node("a.b", None), node("a.c", None), node("x.y", None)] };
        let n = op(Action::Delete, vec![(Field::Permission, Comparison::Similar, "a.%")]).apply(&mut set);
        assert_eq!(n, 2);
        assert_eq!(set.nodes.len(), 1);
        assert_eq!(set.nodes[0].key, "x.y");
    }

    #[test]
    fn update_permission_keeps_value_and_contexts() {
        let mut set = NodeSet { nodes: vec![node("old.node", Some("lobby"))] };
        let n = op(Action::Update(Field::Permission, "new.node".into()), vec![(Field::Permission, Comparison::Equal, "OLD.node")]).apply(&mut set);
        assert_eq!(n, 1);
        assert_eq!(set.nodes[0].key, "new.node");
        assert!(set.nodes[0].contexts.contains("server", "lobby"));
    }

    #[test]
    fn update_server_to_global_drops_the_context() {
        let mut set = NodeSet { nodes: vec![node("a.b", Some("lobby"))] };
        op(Action::Update(Field::Server, "global".into()), vec![(Field::Server, Comparison::Equal, "lobby")]).apply(&mut set);
        assert!(!set.nodes[0].contexts.contains_key("server"));
    }

    #[test]
    fn a_node_without_a_server_context_is_global() {
        let mut set = NodeSet { nodes: vec![node("a.b", None), node("c.d", Some("lobby"))] };
        let n = op(Action::Delete, vec![(Field::Server, Comparison::Equal, "global")]).apply(&mut set);
        assert_eq!(n, 1);
        assert_eq!(set.nodes[0].key, "c.d");
    }

    #[test]
    fn the_statement_is_readable() {
        let o = op(Action::Update(Field::World, "w".into()), vec![(Field::Permission, Comparison::NotEqual, "a"), (Field::Server, Comparison::Similar, "l%")]);
        assert_eq!(o.readable(), "UPDATE all SET world = 'w' WHERE permission != 'a' AND server ~~ 'l%'");
    }

    #[test]
    fn the_tree_json_nests_full_names() {
        let mut t = Tree::default();
        t.insert("a.b.c");
        t.insert("a.d");
        let v = t.json("");
        assert_eq!(v["a"]["a.b"]["a.b.c"], serde_json::json!({}));
        assert!(v["a"].get("a.d").is_some());
        assert!(t.find("a.b").is_some());
        assert!(t.find("a.x").is_none());
    }
}
