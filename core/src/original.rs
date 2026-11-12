//! The files of a Paper LuckPerms install, read as they are: `plugins/LuckPerms/config.yml` (YAML, with
//! the vendored parser in `yaml`) and `plugins/LuckPerms/contexts.json`. Every key the port acts on is
//! read here under its original name. A key read on purpose but not acted on is listed in [`IGNORED`]
//! with the reason. A key that is acted on but not supported is put in `Config::unsupported` (logged at
//! start). The rest is reported by [`inspect_yaml`] / [`inspect_json`] (the `check_original` example).
//!
//! Only key paths and counts are ever reported, never values: a value can be a password.

use crate::config::{Config, PlayersConsole, StackFormat};
use crate::model::TempMerge;
use crate::yaml::{self, Node};
use std::collections::{BTreeMap, BTreeSet};

/// The name of the LuckPerms config file in `plugins/LuckPerms/`.
pub const CONFIG_YML: &str = "config.yml";
/// The contexts file next to it.
pub const CONTEXTS_JSON: &str = "contexts.json";

/// Keys the port reads on purpose and does not act on, with the reason. The `check_original` report
/// does not count them as missing. Mirrored in GAPS.md.
pub const IGNORED: &[(&str, &str)] = &[
    ("data.mongodb-collection-prefix", "no MongoDB storage"),
    ("data.mongodb-connection-uri", "no MongoDB storage"),
    ("split-storage.methods", "only used with split storage, which is refused when enabled"),
    ("redis.address", "Redis messenger not spoken; refused when enabled"),
    ("nats.address", "NATS messenger not spoken; refused when enabled"),
    ("rabbitmq.address", "RabbitMQ messenger not spoken; refused when enabled"),
    ("rabbitmq.vhost", "RabbitMQ messenger not spoken; refused when enabled"),
    ("apply-bukkit-child-permissions", "Bukkit permission attachments do not exist on Horizon"),
    ("apply-bukkit-attachment-permissions", "Bukkit permission attachments do not exist on Horizon"),
    ("apply-sponge-implicit-wildcards", "Sponge only"),
    ("disable-bulkupdate", "/lp bulkupdate is not ported"),
    ("skip-bulkupdate-confirmation", "/lp bulkupdate is not ported"),
    ("debug-logins", "only adds log lines in LuckPerms"),
    ("auto-install-translations", "English only (GAPS.md)"),
    ("register-command-list-data", "Bukkit command list data; Horizon builds its own tree"),
    ("use-server-uuid-cache", "Horizon's own player list is used"),
    ("vault-unsafe-lookups", "Vault is not ported"),
    ("vault-group-use-displaynames", "Vault is not ported"),
    ("vault-npc-group", "Vault is not ported"),
    ("vault-npc-op-status", "Vault is not ported"),
    ("vault-server", "Vault is not ported"),
    ("vault-include-global", "Vault is not ported"),
    ("vault-ignore-world", "Vault is not ported"),
];

/// Map keys whose entries are data (group or world names): a report shows them as `<map>.*`.
const DYNAMIC: &[&str] = &[
    "group-weight",
    "world-rewrite",
    "static-contexts",
    "default-contexts",
    "meta-value-selection",
    "data.pool-settings.properties",
];

/// The key paths a loader read. A path is read when it, or one of its parents, was read (a map read
/// as a whole covers its entries).
#[derive(Clone, Debug, Default)]
pub struct Reads {
    seen: BTreeSet<String>,
}

impl Reads {
    pub fn mark(&mut self, path: &str) {
        self.seen.insert(path.to_owned());
    }

    pub fn covers(&self, path: &str) -> bool {
        let mut p = path;
        loop {
            if self.seen.contains(p) {
                return true;
            }
            match p.rfind('.') {
                Some(i) => p = &p[..i],
                None => return false,
            }
        }
    }
}

/// A read-only view of a parsed file that records every path it looks at.
struct Keys<'a, 'r> {
    root: &'a Node,
    reads: &'r mut Reads,
}

impl<'a> Keys<'a, '_> {
    fn node(&mut self, path: &str) -> Option<&'a Node> {
        self.reads.mark(path);
        self.root.path(path)
    }

    fn text(&mut self, path: &str) -> Option<String> {
        self.node(path)?.as_str().map(str::to_owned)
    }

    fn flag(&mut self, path: &str) -> Option<bool> {
        self.node(path)?.as_bool()
    }

    fn number(&mut self, path: &str) -> Option<i64> {
        self.node(path)?.as_i64()
    }

    fn list(&mut self, path: &str) -> Option<Vec<String>> {
        self.node(path).map(|n| n.str_list())
    }

    /// A map of scalars as text (numbers included), in file order.
    fn texts(&mut self, path: &str) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        if let Some(n) = self.node(path) {
            for e in n.entries() {
                if let Some(v) = e.value.as_str() {
                    out.insert(e.key.clone(), v.to_owned());
                }
            }
        }
        out
    }

    fn ints(&mut self, path: &str) -> BTreeMap<String, i32> {
        let mut out = BTreeMap::new();
        if let Some(n) = self.node(path) {
            for e in n.entries() {
                if let Some(v) = e.value.as_i64() {
                    out.insert(e.key.clone(), v as i32);
                }
            }
        }
        out
    }
}

macro_rules! set {
    ($k:ident, $key:literal, $dst:expr) => {
        if let Some(v) = $k.flag($key) {
            $dst = v;
        }
    };
}

fn stack(k: &mut Keys, base: &str, dst: &mut StackFormat) {
    if let Some(v) = k.list(&format!("{base}.format")).filter(|v| !v.is_empty()) {
        dst.format = v;
    }
    if let Some(v) = k.text(&format!("{base}.duplicates")) {
        dst.duplicates = v;
    }
    if let Some(v) = k.text(&format!("{base}.start-spacer")) {
        dst.start_spacer = v;
    }
    if let Some(v) = k.text(&format!("{base}.middle-spacer")) {
        dst.middle_spacer = v;
    }
    if let Some(v) = k.text(&format!("{base}.end-spacer")) {
        dst.end_spacer = v;
    }
}

fn players_console(k: &mut Keys, base: &str, dst: &mut PlayersConsole) {
    if let Some(v) = k.flag(&format!("{base}.players")) {
        dst.players = v;
    }
    if let Some(v) = k.flag(&format!("{base}.console")) {
        dst.console = v;
    }
}

/// `config.yml` as LuckPerms reads it, under its own key names. Records every key it reads in `reads`.
/// `Err` when the file cannot be used at all (a parse error, or split storage, whose files are not read):
/// the service then does not start. The message names the key or line, never a value.
pub fn config_from_yaml(text: &str, reads: &mut Reads) -> Result<Config, String> {
    let root = yaml::parse(text).map_err(|e| format!("{CONFIG_YML}: parse error at line {}", e.line))?;
    let mut c = Config::default();
    let mut notes: Vec<String> = Vec::new();
    {
        let mut k = Keys { root: &root, reads };

        if let Some(v) = k.text("server") {
            let v = v.trim().to_lowercase();
            c.server = if v.is_empty() { "global".into() } else { v };
        }
        if let Some(v) = k.text("storage-method") {
            c.storage.method = v;
        }
        if let Some(v) = k.text("data.address") {
            c.storage.address = v;
        }
        if let Some(v) = k.text("data.database") {
            c.storage.database = v;
        }
        if let Some(v) = k.text("data.username") {
            c.storage.username = v;
        }
        if let Some(v) = k.text("data.password") {
            c.storage.password = v;
        }
        if let Some(v) = k.text("data.table-prefix") {
            c.storage.table_prefix = v.trim().to_owned();
        }
        if let Some(v) = k.number("data.pool-settings.maximum-pool-size") {
            c.storage.maximum_pool_size = v.max(1) as usize;
        }
        if let Some(v) = k.number("data.pool-settings.minimum-idle") {
            c.storage.minimum_idle = v.max(0) as usize;
        }
        if let Some(v) = k.number("data.pool-settings.connection-timeout") {
            c.storage.connection_timeout_ms = v.max(0) as u64;
        }
        if let Some(v) = k.number("data.pool-settings.keepalive-time") {
            c.storage.keepalive_time_ms = v.max(0) as u64;
        }
        if let Some(v) = k.number("data.pool-settings.maximum-lifetime") {
            c.storage.maximum_lifetime_ms = v.max(0) as u64;
            if v > 0 {
                notes.push("data.pool-settings.maximum-lifetime (not applied: the MySQL client pool has no lifetime)".into());
            }
        }
        c.storage.pool_properties = k.texts("data.pool-settings.properties");

        // Split storage keeps its data in H2 files that this port does not read.
        if k.flag("split-storage.enabled") == Some(true) {
            return Err(format!("{CONFIG_YML}: split-storage is enabled (its H2 files are not read); not started"));
        }

        // Messaging: the SQL messenger is the only one spoken. A non-SQL messenger the original would use
        // is refused: messaging stays off and the service says so.
        let service = k.text("messaging-service").unwrap_or_else(|| "auto".into()).trim().to_lowercase();
        let mut enabled: Vec<&str> = Vec::new();
        for (name, path) in [("redis", "redis.enabled"), ("nats", "nats.enabled"), ("rabbitmq", "rabbitmq.enabled")] {
            if k.flag(path) == Some(true) {
                enabled.push(name);
            }
        }
        c.messenger_refused = match service.as_str() {
            "sql" | "none" => None,
            "redis" | "nats" | "rabbitmq" => Some(service.clone()),
            _ => enabled.first().map(|s| s.to_string()),
        };
        if let Some(m) = &c.messenger_refused {
            notes.push(format!("messaging-service: {m} is not supported (messaging off)"));
        }
        c.messaging_service = service;

        if let Some(v) = k.number("sync-minutes") {
            c.sync_minutes = v;
        }
        set!(k, "auto-push-updates", c.auto_push_updates);
        set!(k, "push-log-entries", c.push_log_entries);
        set!(k, "broadcast-received-log-entries", c.broadcast_received_log_entries);
        set!(k, "log-notify", c.log_notify);
        set!(k, "watch-files", c.watch_files);
        if let Some(v) = k.text("temporary-add-behaviour") {
            c.temporary_add_behaviour = TempMerge::parse(&v).ok_or_else(|| format!("{CONFIG_YML}: temporary-add-behaviour is not accumulate, replace or deny"))?;
        }
        if let Some(v) = k.text("primary-group-calculation") {
            c.primary_group_calculation = v;
        }
        if let Some(v) = k.text("inheritance-traversal-algorithm") {
            c.inheritance_traversal_algorithm = v;
        }
        set!(k, "post-traversal-inheritance-sort", c.post_traversal_inheritance_sort);
        set!(k, "include-global", c.include_global);
        set!(k, "include-global-world", c.include_global_world);
        set!(k, "apply-global-groups", c.apply_global_groups);
        set!(k, "apply-global-world-groups", c.apply_global_world_groups);
        set!(k, "apply-wildcards", c.apply_wildcards);
        set!(k, "apply-regex", c.apply_regex);
        set!(k, "apply-shorthand", c.apply_shorthand);
        set!(k, "prevent-primary-group-removal", c.prevent_primary_group_removal);
        set!(k, "apply-bukkit-default-permissions", c.apply_bukkit_default_permissions);
        set!(k, "enable-ops", c.horizon.enable_ops);
        set!(k, "commands-allow-op", c.horizon.commands_allow_op);
        stack(&mut k, "meta-formatting.prefix", &mut c.meta_formatting.prefix);
        stack(&mut k, "meta-formatting.suffix", &mut c.meta_formatting.suffix);
        c.group_weight = k.ints("group-weight");
        c.world_rewrite = k.texts("world-rewrite");
        if let Some(v) = k.list("disabled-contexts") {
            c.disabled_contexts = v;
        }
        if let Some(v) = k.list("log-notify-filtered-descriptions") {
            c.log_notify_filtered_descriptions = v;
        }
        players_console(&mut k, "disable-luckperms-commands", &mut c.disable_luckperms_commands);
        players_console(&mut k, "commands-read-only-mode", &mut c.commands_read_only_mode);

        // Read and not supported: a non-default value is noted (a key name only, never its value).
        if k.flag("auto-op") == Some(true) {
            notes.push("auto-op (not supported)".into());
        }
        if k.flag("argument-based-command-permissions") == Some(true) {
            notes.push("argument-based-command-permissions (not supported)".into());
        }
        if k.flag("require-sender-group-membership-to-modify") == Some(true) {
            notes.push("require-sender-group-membership-to-modify (not supported)".into());
        }
        if k.flag("apply-default-negated-permissions-before-wildcards") == Some(true) {
            notes.push("apply-default-negated-permissions-before-wildcards (not supported)".into());
        }
        if k.flag("resolve-command-selectors") == Some(true) {
            notes.push("resolve-command-selectors (not supported)".into());
        }
        if k.flag("update-client-command-list") == Some(false) {
            notes.push("update-client-command-list (set false; the port always updates the list)".into());
        }
        if k.flag("use-vault-server") == Some(true) {
            notes.push("use-vault-server (not supported)".into());
        }
        if k.node("disabled-context-calculators").is_some_and(|n| !n.items().is_empty()) {
            notes.push("disabled-context-calculators (not supported)".into());
        }
        if k.text("context-satisfy-mode").is_some_and(|v| v != "at-least-one-value-per-key") {
            notes.push("context-satisfy-mode (the port always uses at-least-one-value-per-key)".into());
        }
        if k.text("meta-value-selection-default").is_some_and(|v| v != "inheritance") {
            notes.push("meta-value-selection-default (only inheritance is supported)".into());
        }
        if k.node("meta-value-selection").is_some_and(|n| !n.entries().is_empty()) {
            notes.push("meta-value-selection (not supported)".into());
        }
    }
    for (path, _) in IGNORED {
        reads.mark(path);
    }
    reads.mark("use-vault-server");
    c.unsupported = notes;
    Ok(c)
}

/// `contexts.json`: the `static-contexts` (added to every player here) and `default-contexts`.
pub fn contexts_from_json(text: &str, reads: &mut Reads) -> Result<(BTreeMap<String, String>, BTreeMap<String, String>), String> {
    let v: serde_json::Value = serde_json::from_str(text).map_err(|_| format!("{CONTEXTS_JSON}: not valid JSON"))?;
    reads.mark("static-contexts");
    reads.mark("default-contexts");
    Ok((json_map(&v, "static-contexts")?, json_map(&v, "default-contexts")?))
}

fn json_map(v: &serde_json::Value, key: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    match v.get(key) {
        None | Some(serde_json::Value::Null) => {}
        Some(serde_json::Value::Object(m)) => {
            for (k, val) in m {
                let text = match val {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Number(_) | serde_json::Value::Bool(_) => val.to_string(),
                    _ => return Err(format!("{CONTEXTS_JSON}: {key} has a value that is not a string, number or boolean")),
                };
                out.insert(k.clone(), text);
            }
        }
        Some(_) => return Err(format!("{CONTEXTS_JSON}: {key} is not an object")),
    }
    Ok(out)
}

/// What the `check_original` report shows: key paths only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Key paths of the file the loader read.
    pub read: Vec<String>,
    /// Key paths of the file it did not read (ignoring [`IGNORED`]: those are read, see `read`).
    pub unread: Vec<String>,
    /// Settings that the port does not act on (a key name, no value).
    pub notes: Vec<String>,
    /// Why the file could not be used at all (no values).
    pub error: Option<String>,
}

fn leaves(node: &Node, prefix: &str, out: &mut Vec<String>) {
    let Node::Map { entries, .. } = node else {
        return;
    };
    for e in entries {
        let path = if prefix.is_empty() { e.key.clone() } else { format!("{prefix}.{}", e.key) };
        let nonempty_map = matches!(&e.value, Node::Map { entries, .. } if !entries.is_empty());
        if nonempty_map && DYNAMIC.contains(&path.as_str()) {
            out.push(format!("{path}.*"));
        } else if nonempty_map {
            leaves(&e.value, &path, out);
        } else {
            out.push(path);
        }
    }
}

/// The report for a `config.yml`: the loader run on the text, then every key path of the file split into
/// read and not read. The ignored keys are counted as read.
pub fn inspect_yaml(text: &str) -> Report {
    let root = match yaml::parse(text) {
        Ok(r) => r,
        Err(e) => return Report { error: Some(format!("parse error at line {}", e.line)), ..Report::default() },
    };
    let mut reads = Reads::default();
    let cfg = match config_from_yaml(text, &mut reads) {
        Ok(c) => c,
        Err(e) => return Report { error: Some(e), ..Report::default() },
    };
    let mut all = Vec::new();
    leaves(&root, "", &mut all);
    let (read, unread) = all.into_iter().partition(|p| reads.covers(p));
    Report { read, unread, notes: cfg.unsupported, error: None }
}

/// The report for a `contexts.json`: the two maps, and any other top-level key as not read.
pub fn inspect_json(text: &str) -> Report {
    let v: serde_json::Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return Report { error: Some(format!("{CONTEXTS_JSON}: not valid JSON")), ..Report::default() },
    };
    let mut reads = Reads::default();
    if let Err(e) = contexts_from_json(text, &mut reads) {
        return Report { error: Some(e), ..Report::default() };
    }
    let (mut read, mut unread) = (Vec::new(), Vec::new());
    if let Some(m) = v.as_object() {
        for key in m.keys() {
            if reads.covers(key) {
                read.push(key.clone());
            } else {
                unread.push(key.clone());
            }
        }
    }
    Report { read, unread, notes: Vec::new(), error: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Invented values with the same structure as the original file.
    const FIXTURE: &str = r#"
server: Lobby
storage-method: mariadb
data:
  address: db.example:3307
  database: exampledb
  username: exampleuser
  password: not-a-real-password
  table-prefix: 'lp_'
  pool-settings:
    maximum-pool-size: 12
    minimum-idle: 3
    maximum-lifetime: 900000
    keepalive-time: 0
    connection-timeout: 4000
    properties:
      useUnicode: true
      characterEncoding: utf8
  mongodb-collection-prefix: ''
split-storage:
  enabled: false
  methods:
    user: h2
sync-minutes: 15
messaging-service: auto
auto-push-updates: false
redis:
  enabled: false
  address: localhost
nats:
  enabled: false
rabbitmq:
  enabled: false
temporary-add-behaviour: replace
primary-group-calculation: stored
log-notify: false
meta-formatting:
  prefix:
    format:
      - "highest"
      - "_inherited"
    duplicates: retain-all
    start-spacer: "["
    middle-spacer: ""
    end-spacer: "]"
  suffix:
    format:
      - "highest"
disabled-contexts:
  - world
apply-bukkit-default-permissions: true
apply-bukkit-child-permissions: true
context-satisfy-mode: at-least-one-value-per-key
enable-ops: true
commands-allow-op: false
group-weight:
  admin: 100
  member: 10
world-rewrite:
  world_nether: nether
disable-luckperms-commands:
  players: false
  console: true
vault-server: global
auto-op: false
"#;

    fn load(text: &str) -> (Config, Reads) {
        let mut r = Reads::default();
        let c = config_from_yaml(text, &mut r).expect("fixture loads");
        (c, r)
    }

    #[test]
    fn keys_map_to_the_port_fields() {
        let (c, _) = load(FIXTURE);
        assert_eq!(c.server, "lobby");
        assert_eq!(c.storage.method, "mariadb");
        assert_eq!(c.storage.address, "db.example:3307");
        assert_eq!(c.storage.database, "exampledb");
        assert_eq!(c.storage.username, "exampleuser");
        assert_eq!(c.storage.password, "not-a-real-password");
        assert_eq!(c.storage.table_prefix, "lp_");
        assert_eq!(c.storage.maximum_pool_size, 12);
        assert_eq!(c.storage.minimum_idle, 3);
        assert_eq!(c.storage.connection_timeout_ms, 4000);
        assert_eq!(c.storage.keepalive_time_ms, 0);
        assert_eq!(c.storage.maximum_lifetime_ms, 900_000);
        assert_eq!(c.storage.pool_properties.get("characterEncoding").map(String::as_str), Some("utf8"));
        assert_eq!(c.sync_minutes, 15);
        assert!(!c.auto_push_updates);
        assert_eq!(c.temporary_add_behaviour, TempMerge::Replace);
        assert_eq!(c.primary_group_calculation, "stored");
        assert!(!c.log_notify);
        assert_eq!(c.meta_formatting.prefix.format, vec!["highest", "_inherited"]);
        assert_eq!(c.meta_formatting.prefix.duplicates, "retain-all");
        assert_eq!(c.meta_formatting.prefix.start_spacer, "[");
        assert_eq!(c.meta_formatting.prefix.end_spacer, "]");
        assert_eq!(c.meta_formatting.suffix.format, vec!["highest"]);
        assert_eq!(c.disabled_contexts, vec!["world"]);
        assert!(c.apply_bukkit_default_permissions);
        assert_eq!(c.group_weight.get("admin"), Some(&100));
        assert_eq!(c.group_weight.get("member"), Some(&10));
        assert_eq!(c.world_rewrite.get("world_nether").map(String::as_str), Some("nether"));
        assert!(c.disable_luckperms_commands.console);
        assert!(!c.disable_luckperms_commands.players);
        assert!(c.horizon.enable_ops);
        assert!(!c.horizon.commands_allow_op);
        assert!(c.sql_messaging());
    }

    #[test]
    fn non_sql_messengers_are_refused_not_spoken() {
        let (c, _) = load("redis:\n  enabled: true\n  address: example.test\n");
        assert_eq!(c.messenger_refused.as_deref(), Some("redis"));
        assert!(!c.sql_messaging());
        assert!(c.unsupported.iter().any(|n| n.starts_with("messaging-service: redis")));
        // An explicit sql service ignores the enabled flags.
        let (c, _) = load("messaging-service: sql\nredis:\n  enabled: true\n");
        assert_eq!(c.messenger_refused, None);
        assert!(c.sql_messaging());
        // An explicit none stays none.
        let (c, _) = load("messaging-service: none\n");
        assert!(!c.sql_messaging());
    }

    #[test]
    fn split_storage_is_refused() {
        let mut r = Reads::default();
        let err = config_from_yaml("split-storage:\n  enabled: true\n", &mut r).unwrap_err();
        assert!(err.contains("split-storage"), "{err}");
    }

    #[test]
    fn parse_errors_do_not_echo_the_text() {
        let mut r = Reads::default();
        let err = config_from_yaml("password: [unclosed\n", &mut r).unwrap_err();
        assert!(err.starts_with("config.yml: parse error at line"), "{err}");
        assert!(!err.contains("unclosed"));
    }

    #[test]
    fn unsupported_settings_are_noted_by_key_only() {
        let (c, _) = load(FIXTURE);
        assert!(c.unsupported.iter().any(|n| n.starts_with("data.pool-settings.maximum-lifetime")));
        assert!(!c.unsupported.iter().any(|n| n.contains("not-a-real-password")));
        assert!(!c.unsupported.iter().any(|n| n.starts_with("auto-op")), "auto-op false is default");
        let (c, _) = load("auto-op: true\nuse-vault-server: true\nmeta-value-selection-default: weight\n");
        assert!(c.unsupported.iter().any(|n| n.starts_with("auto-op")));
        assert!(c.unsupported.iter().any(|n| n.starts_with("use-vault-server")));
        assert!(c.unsupported.iter().any(|n| n.starts_with("meta-value-selection-default")));
    }

    #[test]
    fn empty_file_is_the_defaults() {
        let (c, _) = load("");
        assert_eq!(c.server, "global");
        assert_eq!(c.storage.method, "mariadb");
        assert!(c.apply_bukkit_default_permissions);
        assert!(c.unsupported.is_empty());
    }

    #[test]
    fn contexts_json_maps() {
        let mut r = Reads::default();
        let (s, d) = contexts_from_json(r#"{"static-contexts": {"region": "north"}, "default-contexts": {}}"#, &mut r).unwrap();
        assert_eq!(s.get("region").map(String::as_str), Some("north"));
        assert!(d.is_empty());
        assert!(contexts_from_json("[]", &mut r).is_ok(), "no maps is empty");
        assert!(contexts_from_json("{", &mut r).is_err());
        assert!(contexts_from_json(r#"{"static-contexts": {"a": []}}"#, &mut r).is_err());
    }

    #[test]
    fn report_splits_read_from_unread_by_path() {
        let rep = inspect_yaml(FIXTURE);
        assert!(rep.error.is_none());
        assert!(rep.read.contains(&"data.address".to_string()));
        assert!(rep.read.contains(&"data.pool-settings.properties.*".to_string()), "a map of data is one path");
        assert!(rep.read.contains(&"group-weight.*".to_string()), "a map read as a whole counts as read");
        assert!(rep.read.contains(&"data.mongodb-collection-prefix".to_string()), "ignored keys count as read");
        assert!(rep.read.contains(&"apply-bukkit-child-permissions".to_string()));
        // Nothing in the fixture is left out: every leaf is either read or named.
        assert!(rep.unread.is_empty(), "{:?}", rep.unread);
        // Values never appear in the report.
        let all = format!("{:?}", rep);
        assert!(!all.contains("not-a-real-password"));
        assert!(!all.contains("exampleuser"));
    }

    #[test]
    fn a_key_the_loader_does_not_know_is_unread() {
        let rep = inspect_yaml("some-new-key: 1\nserver: x\n");
        assert_eq!(rep.unread, vec!["some-new-key".to_string()]);
        assert!(rep.read.contains(&"server".to_string()));
    }

    #[test]
    fn json_report_lists_top_level_keys() {
        let rep = inspect_json(r#"{"static-contexts": {}, "default-contexts": {}, "extra": 1}"#);
        let mut read = rep.read.clone();
        read.sort();
        assert_eq!(read, vec!["default-contexts", "static-contexts"]);
        assert_eq!(rep.unread, vec!["extra"]);
        assert!(inspect_json("{").error.is_some());
    }
}
