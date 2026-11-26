//! `/lp export` and `/lp import`: LuckPerms' export file (gzip JSON, `Exporter` and `Importer`), the
//! bytebin upload (`--upload`), and the web editor's shape when an uploaded file holds an editor reply.
//! Files live in the data directory (`config/luckperms/`), and only direct children of it are used.

use super::args::Args;
use super::editor::{http_failure, node_json, parse_node};
use super::extra::upload;
use super::track::save_track;
use super::{Ctx, Holder, Loaded};
use crate::horizon_luckperms_core::bytebin;
use crate::horizon_luckperms_core::duration;
use crate::horizon_luckperms_core::lang;
use crate::horizon_luckperms_core::model::{Group, Track};
use crate::horizon_luckperms_core::node::{Equality, Node};
use crate::horizon_luckperms_core::uuid;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

/// The data directory of the mod (set when the runtime starts; `config/luckperms` until then).
static DATA_DIR: OnceLock<PathBuf> = OnceLock::new();

/// Sets the data directory: `<config dir>/luckperms`.
pub(crate) fn set_data_dir(config_dir: &Path) {
    let _ = DATA_DIR.set(config_dir.join("luckperms"));
}

fn data_dir() -> PathBuf {
    DATA_DIR.get().cloned().unwrap_or_else(|| PathBuf::from("config").join("luckperms"))
}

/// A file in the data directory (the web editor's keystore lives there).
#[cfg(feature = "wire")]
pub(crate) fn data_file(name: &str) -> PathBuf {
    data_dir().join(name)
}

/// `path.getParent().equals(dataDirectory)`: the file must be a direct child of the data directory.
fn child_of_data(name: &str) -> Option<PathBuf> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return None;
    }
    Some(data_dir().join(name))
}

/// The date `yyyy-MM-dd HH:mm:ss` in UTC for unix seconds (proleptic Gregorian, no time zone data here).
pub(crate) fn utc_stamp(unix: i64) -> String {
    let (y, mo, d, h, mi, s) = civil(unix);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}")
}

/// Unix seconds to (year, month, day, hour, minute, second), days from the civil calendar (H. Hinnant).
fn civil(unix: i64) -> (i64, i64, i64, i64, i64, i64) {
    let days = unix.div_euclid(86_400);
    let secs = unix.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

/// The file name of an export made without one: `luckperms-yyyy-MM-dd-HH-mm.json.gz`.
fn default_export_name(unix: i64) -> String {
    let (y, mo, d, h, mi, _) = civil(unix);
    format!("luckperms-{y:04}-{mo:02}-{d:02}-{h:02}-{mi:02}.json.gz")
}

/// Packs the export text: gzip with the net feature (the default), plain otherwise.
#[cfg(feature = "net")]
fn pack(text: &[u8]) -> Vec<u8> {
    bytebin::gzip(text)
}

#[cfg(not(feature = "net"))]
fn pack(text: &[u8]) -> Vec<u8> {
    text.to_vec()
}

/// Unpacks a file: gzip when it starts with the gzip magic, the bytes as they are otherwise.
#[cfg(feature = "net")]
fn unpack(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return bytebin::gunzip(&bytes, bytebin::MAX_DOWNLOAD * 16);
    }
    Ok(bytes)
}

#[cfg(not(feature = "net"))]
fn unpack(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("gzip needs the net feature".into());
    }
    Ok(bytes)
}

fn log_line(msg: &str) -> String {
    format!("\u{a7}3{} \u{a7}3\u{a7}l> \u{a7}f{msg}", lang::T_LOGS_EXPORT_PREFIX)
}

fn import_line(c: &mut Ctx<'_>, msg: &str) {
    c.msg(format!("\u{a7}f\u{a7}b({}) \u{a7}b-> \u{a7}f{msg}.", lang::T_COMMAND_IMPORT_TERM));
}

fn error_line(c: &mut Ctx<'_>, term: &str, text: String) {
    c.msg(format!("\u{a7}c{term}: \u{a7}c{text}."));
}

/// The export document, as `Exporter.run` builds it. The user list comes from storage.
fn build_export(c: &mut Ctx<'_>, with_users: bool, with_groups: bool) -> Option<Value> {
    let mut root = Map::new();
    root.insert("metadata".into(), json!({ "generatedBy": c.sender.name, "generatedAt": utc_stamp(duration::now()) }));
    if with_groups {
        c.msg(log_line("Gathering group data..."));
        let mut groups = Map::new();
        {
            let m = c.model();
            let mut list: Vec<&Group> = m.groups.values().collect();
            list.sort_by(|a, b| b.weight().unwrap_or(0).cmp(&a.weight().unwrap_or(0)).then_with(|| a.name.cmp(&b.name)));
            for g in list {
                let nodes: Vec<Value> = g.data.nodes.iter().map(node_json).collect();
                groups.insert(g.name.clone(), json!({ "nodes": nodes }));
            }
        }
        root.insert("groups".into(), Value::Object(groups));

        c.msg(log_line("Gathering track data..."));
        let mut tracks = Map::new();
        {
            let m = c.model();
            let mut list: Vec<&Track> = m.tracks.values().collect();
            list.sort_by(|a, b| a.name.cmp(&b.name));
            for t in list {
                tracks.insert(t.name.clone(), json!({ "groups": t.groups }));
            }
        }
        root.insert("tracks".into(), Value::Object(tracks));
    }
    if with_users {
        c.msg(log_line("Gathering user data..."));
        c.msg(log_line("Finding a list of unique users to export."));
        let ids = match c.st.unique_users() {
            Ok(ids) => ids,
            Err(_) => {
                c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER, &[]);
                return None;
            }
        };
        c.msg(log_line(&format!("Found {} unique users to export.", ids.len())));
        let mut users = Map::new();
        for id in ids {
            let Some(l) = c.load_user(id) else { continue };
            let Holder::User(u) = l.h else { continue };
            let mut entry = Map::new();
            if let Some(name) = &u.username {
                entry.insert("username".into(), json!(name));
            }
            if !u.primary_group.eq_ignore_ascii_case(crate::horizon_luckperms_core::model::DEFAULT_GROUP) {
                entry.insert("primaryGroup".into(), json!(u.primary_group));
            }
            let nodes: Vec<Value> = u.data.nodes.iter().map(node_json).collect();
            entry.insert("nodes".into(), Value::Array(nodes));
            users.insert(uuid::format(id), Value::Object(entry));
        }
        root.insert("users".into(), Value::Object(users));
    }
    Some(Value::Object(root))
}

/// `/lp export [file] [--without-users] [--without-groups] [--upload]`
pub(crate) fn export(c: &mut Ctx<'_>, mut a: Args) {
    let without_users = a.remove_flag("--without-users");
    let without_groups = a.remove_flag("--without-groups");
    let upload_web = a.remove_flag("--upload");

    let path = if upload_web {
        None
    } else {
        let name = if a.is_empty() { default_export_name(duration::now()) } else { format!("{}.json.gz", a.get(0)) };
        let Some(p) = child_of_data(&name) else {
            return error_line(c, lang::T_COMMAND_IMPORT_ERROR_TERM, lang::T_COMMAND_MISC_FILE_MUST_BE_IN_DATA.replace("{0}", &name));
        };
        if p.exists() {
            return error_line(c, lang::T_COMMAND_EXPORT_ERROR_TERM, lang::T_COMMAND_EXPORT_FILE_ALREADY_EXISTS.replace("{0}", &name));
        }
        Some(p)
    };

    let Some(doc) = build_export(c, !without_users, !without_groups) else { return };
    c.msg(log_line("Finished gathering data, writing file..."));
    let text = doc.to_string();

    match path {
        None => {
            let Some(code) = upload(c, text) else { return };
            c.msg(format!("\u{a7}a{}: \u{a7}7{code}", lang::T_COMMAND_EXPORT_WEB_EXPORT_CODE));
            c.raw(format!("\u{a7}7{}:", lang::T_COMMAND_EXPORT_WEB_IMPORT_COMMAND_DESCRIPTION));
            c.raw(format!("\u{a7}a/{} import {code} --upload", super::label()));
        }
        Some(p) => {
            if let Err(e) = std::fs::write(&p, pack(text.as_bytes())) {
                eprintln!("[LuckPerms] export: writing {} failed: {e}", p.display());
                return c.bad(lang::T_COMMAND_EXPORT_FILE_UNEXPECTED_ERROR_WRITING, &[]);
            }
            let shown = std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p);
            c.ok(lang::T_COMMAND_EXPORT_FILE_SUCCESS, &[format!("\u{a7}b{}", shown.display())]);
        }
    }
}

/// What an import file holds, read the way `Importer` reads it.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct Import {
    pub groups: Vec<(String, Vec<Node>)>,
    pub tracks: Vec<(String, Vec<String>)>,
    /// uuid, username, primary group, nodes.
    pub users: Vec<(u128, Option<String>, Option<String>, Vec<Node>)>,
    /// Nodes refused by the node parser (malformed or out of range); counted, not applied.
    pub refused: usize,
    /// The file came from the web editor (`knownPermissions`).
    pub web_editor: bool,
}

fn nodes_of(v: &Value, now: i64, refused: &mut usize) -> Result<Vec<Node>, &'static str> {
    let arr = v.as_array().ok_or("nodes is not an array")?;
    let mut out: Vec<Node> = Vec::with_capacity(arr.len());
    for e in arr {
        match parse_node(e, now) {
            Ok(Some(n)) => {
                if !out.contains(&n) {
                    out.push(n);
                }
            }
            Ok(None) => {}
            Err(_) => *refused += 1,
        }
    }
    Ok(out)
}

fn names_of(v: &Value) -> Result<Vec<String>, &'static str> {
    let arr = v.as_array().ok_or("groups is not an array")?;
    arr.iter().map(|g| g.as_str().map(|s| s.to_lowercase()).ok_or("track group is not a string")).collect()
}

/// `Importer.parseExportData` and `parseWebEditorData`. Malformed input is an error (the file is not used).
pub(crate) fn parse_import(doc: &Value, now: i64) -> Result<Import, &'static str> {
    let root = doc.as_object().ok_or("not an object")?;
    let mut out = Import::default();
    let refused = &mut out.refused;

    if root.contains_key("knownPermissions") {
        out.web_editor = true;
        let holders = root.get("permissionHolders").and_then(Value::as_array).ok_or("no permissionHolders")?;
        for h in holders {
            let o = h.as_object().ok_or("holder is not an object")?;
            let kind = o.get("type").and_then(Value::as_str).ok_or("holder has no type")?;
            let id = o.get("id").and_then(Value::as_str).ok_or("holder has no id")?;
            let nodes = nodes_of(o.get("nodes").ok_or("holder has no nodes")?, now, refused)?;
            if kind.eq_ignore_ascii_case("group") {
                out.groups.push((id.to_lowercase(), nodes));
            } else {
                let u = uuid::parse(id).ok_or("holder id is not a uuid")?;
                let display = o.get("displayName").and_then(Value::as_str).unwrap_or("");
                let username = if uuid::parse(display).is_none() && !display.is_empty() { Some(display.to_owned()) } else { None };
                out.users.push((u, username, None, nodes));
            }
        }
        for t in root.get("tracks").and_then(Value::as_array).ok_or("no tracks")? {
            let o = t.as_object().ok_or("track is not an object")?;
            let id = o.get("id").and_then(Value::as_str).ok_or("track has no id")?;
            out.tracks.push((id.to_lowercase(), names_of(o.get("groups").ok_or("track has no groups")?)?));
        }
        return Ok(out);
    }

    if let Some(groups) = root.get("groups").and_then(Value::as_object) {
        for (name, g) in groups {
            let nodes = nodes_of(g.get("nodes").ok_or("group has no nodes")?, now, refused)?;
            out.groups.push((name.to_lowercase(), nodes));
        }
    }
    if let Some(tracks) = root.get("tracks").and_then(Value::as_object) {
        for (name, t) in tracks {
            out.tracks.push((name.to_lowercase(), names_of(t.get("groups").ok_or("track has no groups")?)?));
        }
    }
    if let Some(users) = root.get("users").and_then(Value::as_object) {
        for (id, u) in users {
            let uid = uuid::parse(id).ok_or("user key is not a uuid")?;
            let o = u.as_object().ok_or("user is not an object")?;
            let username = o.get("username").and_then(Value::as_str).map(str::to_owned);
            let primary = o.get("primaryGroup").and_then(Value::as_str).map(str::to_owned);
            let nodes = nodes_of(o.get("nodes").ok_or("user has no nodes")?, now, refused)?;
            out.users.push((uid, username, primary, nodes));
        }
    }
    Ok(out)
}

/// Reads an import file (a direct child of the data directory), gunzipped when it is gzip.
fn read_file(path: &Path) -> Result<Value, &'static str> {
    let bytes = std::fs::read(path).map_err(|_| "read")?;
    let text = unpack(bytes).map_err(|_| "unpack")?;
    serde_json::from_slice::<Value>(&text).map_err(|_| "json")
}

fn import_line_start(c: &mut Ctx<'_>) {
    c.msg(format!("\u{a7}f\u{a7}b({}) \u{a7}b-> \u{a7}f{}.", lang::T_COMMAND_IMPORT_TERM, lang::T_COMMAND_IMPORT_STARTING));
}

/// Writes the parsed data: groups (created when missing), tracks, then users. `merge` adds the nodes,
/// otherwise a holder's nodes are replaced. A failed save is told to the sender by the save itself;
/// the import goes on, as `Importer` does.
fn apply(c: &mut Ctx<'_>, imp: &Import, merge: bool) {
    for (name, nodes) in &imp.groups {
        if !c.group_exists(name) && c.st.create_group(name).is_err() {
            continue;
        }
        let mut g = c.model().groups.get(name).cloned().unwrap_or_else(|| Group::new(name));
        let before = g.data.nodes.clone();
        apply_nodes(&mut g.data, nodes, merge);
        c.save(Loaded { h: Holder::Group(g), before });
    }
    for (name, groups) in &imp.tracks {
        save_track(c, Track { name: name.clone(), groups: groups.clone() });
    }
    for (uid, username, primary, nodes) in &imp.users {
        if let Some(name) = username {
            let _ = c.st.save_player_data(*uid, name);
        }
        let Some(mut l) = c.load_user(*uid) else { continue };
        if let Holder::User(u) = &mut l.h {
            if let Some(name) = username {
                u.username = Some(name.clone());
            }
            if let Some(p) = primary {
                u.primary_group = p.to_lowercase();
            }
            apply_nodes(&mut u.data, nodes, merge);
        }
        c.save(l);
    }
}

fn apply_nodes(set: &mut crate::horizon_luckperms_core::model::NodeSet, nodes: &[Node], merge: bool) {
    if !merge {
        set.nodes.clear();
    }
    for n in nodes {
        if set.has(n, Equality::Exact).is_none() {
            set.set(n.clone());
        }
    }
}

/// `/lp import <file> [--replace]` and `/lp import <code> --upload [--replace]`
pub(crate) fn import(c: &mut Ctx<'_>, mut a: Args) {
    let merge = !a.remove_flag("--replace");
    let from_web = a.remove_flag("--upload");
    let now = duration::now();

    let doc = if from_web {
        let code = a.get(0);
        if !bytebin::valid_key(&code) {
            return c.msg(format!("\u{a7}c{}. \u{a7}7({code})", lang::T_COMMAND_MISC_INVALID_CODE));
        }
        let url = c.cfg.bytebin_url.clone();
        let body = match bytebin::with_client(&url, |b| b.get(&code)) {
            Ok(bytes) => bytes,
            Err(e) => return http_failure(c, e),
        };
        match serde_json::from_slice::<Value>(&body) {
            Ok(v) if v.is_object() => v,
            _ => return c.msg(format!("\u{a7}c{}. \u{a7}7({code})", lang::T_COMMAND_IMPORT_WEB_UNABLE_TO_READ)),
        }
    } else {
        let name = a.get(0);
        let Some(mut path) = child_of_data(&name) else {
            return error_line(c, lang::T_COMMAND_IMPORT_ERROR_TERM, lang::T_COMMAND_MISC_FILE_MUST_BE_IN_DATA.replace("{0}", &name));
        };
        if !path.exists() && !name.contains('.') {
            if let Some(with_ext) = child_of_data(&format!("{name}.json.gz")) {
                if with_ext.exists() {
                    path = with_ext;
                }
            }
        }
        if !path.exists() {
            return error_line(c, lang::T_COMMAND_IMPORT_ERROR_TERM, lang::T_COMMAND_IMPORT_FILE_DOESNT_EXIST.replace("{0}", &path.display().to_string()));
        }
        if std::fs::File::open(&path).is_err() {
            return error_line(c, lang::T_COMMAND_IMPORT_ERROR_TERM, lang::T_COMMAND_IMPORT_FILE_NOT_READABLE.replace("{0}", &path.display().to_string()));
        }
        match read_file(&path) {
            Ok(v) => v,
            Err(_) => {
                return error_line(
                    c,
                    lang::T_COMMAND_IMPORT_ERROR_TERM,
                    format!("{} (\u{a7}7{}\u{a7}c)", lang::T_COMMAND_IMPORT_FILE_UNEXPECTED_ERROR_READING, lang::T_COMMAND_IMPORT_FILE_CORRECT_FORMAT),
                )
            }
        }
    };

    let started = Instant::now();
    import_line_start(c);
    import_line(c, "Reading data...");
    let imp = match parse_import(&doc, now) {
        Ok(imp) => imp,
        Err(_) => {
            return error_line(
                c,
                lang::T_COMMAND_IMPORT_ERROR_TERM,
                format!("{} (\u{a7}7{}\u{a7}c)", lang::T_COMMAND_IMPORT_FILE_UNEXPECTED_ERROR_READING, lang::T_COMMAND_IMPORT_FILE_CORRECT_FORMAT),
            )
        }
    };
    if imp.web_editor {
        import_line(c, "The data appears to be from a web editor upload - attempting to recover from it");
    }
    if imp.refused > 0 {
        import_line(c, &format!("{} node(s) were refused and not imported", imp.refused));
    }
    import_line(c, "Waiting for initial update task to complete");
    import_line(c, "Setting up data processor");
    import_line(c, "All data entries have been processed and scheduled for import - now waiting for the execution to complete");
    c.out.sync = true;
    apply(c, &imp, merge);
    let secs = format!("\u{a7}b{:.3}\u{a7}f", started.elapsed().as_secs_f64());
    c.msg(format!(
        "\u{a7}7(\u{a7}bImport\u{a7}7) \u{a7}a\u{a7}l{} \u{a7}7- {}.",
        lang::T_COMMAND_IMPORT_COMPLETED,
        lang::T_COMMAND_IMPORT_DURATION.replace("{0}", &secs)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_zero_is_the_epoch() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00");
    }

    #[test]
    fn a_known_instant() {
        // 2023-11-14 22:13:20 UTC
        assert_eq!(utc_stamp(1_700_000_000), "2023-11-14 22:13:20");
        assert_eq!(default_export_name(1_700_000_000), "luckperms-2023-11-14-22-13.json.gz");
    }

    #[test]
    fn a_leap_day() {
        // 2024-02-29 12:00:00 UTC
        assert_eq!(utc_stamp(1_709_208_000), "2024-02-29 12:00:00");
    }

    #[test]
    fn names_must_be_direct_children() {
        assert!(child_of_data("a.json.gz").is_some());
        assert!(child_of_data("../x").is_none());
        assert!(child_of_data("sub/x").is_none());
        assert!(child_of_data("..").is_none());
        assert!(child_of_data("").is_none());
    }

    #[test]
    fn export_file_is_read_back() {
        let doc = json!({
            "metadata": {"generatedBy": "Console", "generatedAt": "2023-11-14 22:13:20"},
            "groups": {"admin": {"nodes": [{"type": "permission", "key": "essentials.gamemode", "value": true}]}},
            "tracks": {"staff": {"groups": ["mod", "admin"]}},
            "users": {"00000000-0000-0000-0000-000000000001": {
                "username": "example",
                "primaryGroup": "admin",
                "nodes": [{"type": "inheritance", "key": "group.admin", "value": true, "expiry": 2000000000}]
            }}
        });
        let imp = parse_import(&doc, 1_000).expect("parses");
        assert!(!imp.web_editor);
        assert_eq!(imp.refused, 0);
        assert_eq!(imp.groups.len(), 1);
        assert_eq!(imp.groups[0].0, "admin");
        assert_eq!(imp.groups[0].1[0].key, "essentials.gamemode");
        assert!(imp.groups[0].1[0].value);
        assert_eq!(imp.tracks, vec![("staff".to_string(), vec!["mod".to_string(), "admin".to_string()])]);
        let (uid, name, primary, nodes) = &imp.users[0];
        assert_eq!(*uid, 1);
        assert_eq!(name.as_deref(), Some("example"));
        assert_eq!(primary.as_deref(), Some("admin"));
        assert_eq!(nodes[0].key, "group.admin");
        assert_eq!(nodes[0].expiry, 2_000_000_000);
    }

    #[test]
    fn a_node_that_already_expired_is_dropped_and_a_bad_one_counted() {
        let doc = json!({
            "groups": {"g": {"nodes": [
                {"type": "permission", "key": "a.b", "value": true, "expiry": 500},
                {"type": "permission", "key": "c.d", "value": "yes"},
                {"type": "permission", "key": "e.f", "value": false}
            ]}}
        });
        let imp = parse_import(&doc, 1_000).expect("parses");
        assert_eq!(imp.refused, 1);
        assert_eq!(imp.groups[0].1.len(), 1);
        assert_eq!(imp.groups[0].1[0].key, "e.f");
    }

    #[test]
    fn web_editor_reply_is_recovered() {
        let doc = json!({
            "knownPermissions": ["a.b"],
            "permissionHolders": [
                {"type": "group", "id": "Mod", "nodes": [{"key": "a.b", "value": true}]},
                {"type": "user", "id": "00000000-0000-0000-0000-000000000002", "displayName": "Steve", "nodes": []}
            ],
            "tracks": [{"id": "staff", "groups": ["Mod"]}]
        });
        let imp = parse_import(&doc, 1_000).expect("parses");
        assert!(imp.web_editor);
        assert_eq!(imp.groups[0].0, "mod");
        assert_eq!(imp.users[0].1.as_deref(), Some("Steve"));
        assert_eq!(imp.tracks[0], ("staff".to_string(), vec!["mod".to_string()]));
    }

    #[test]
    fn a_user_key_that_is_not_a_uuid_is_a_format_error() {
        let doc = json!({"users": {"steve": {"nodes": []}}});
        assert!(parse_import(&doc, 0).is_err());
        assert!(parse_import(&json!([1, 2]), 0).is_err());
    }

    #[test]
    fn merge_keeps_and_replace_drops() {
        use crate::horizon_luckperms_core::context::ContextSet;
        let mut set = crate::horizon_luckperms_core::model::NodeSet::default();
        set.set(Node::new("a.x", true, 0, ContextSet::new()));
        let add = vec![Node::new("b.y", true, 0, ContextSet::new())];
        apply_nodes(&mut set, &add, true);
        assert_eq!(set.nodes.len(), 2);
        apply_nodes(&mut set, &add, false);
        assert_eq!(set.nodes.len(), 1);
        assert_eq!(set.nodes[0].key, "b.y");
    }
}
