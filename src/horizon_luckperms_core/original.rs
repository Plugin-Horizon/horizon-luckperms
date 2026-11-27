//! The files of a Paper LuckPerms install, read as they are: `plugins/LuckPerms/config.yml` (YAML, with
//! the vendored parser in `yaml`) and `plugins/LuckPerms/contexts.json`. Every key the port acts on is
//! read here under its original name. A key read on purpose but not acted on is listed in [`IGNORED`]
//! with the reason. A key that is acted on but not supported is put in `Config::unsupported` (logged at
//! start). The rest is reported by [`inspect_yaml`] / [`inspect_json`] (the `check_original` example).
//!
//! Only key paths and counts are ever reported, never values: a value can be a password.

use crate::horizon_luckperms_core::config::{Config, PlayersConsole, StackFormat};
use crate::horizon_luckperms_core::model::TempMerge;
use crate::horizon_luckperms_core::yaml::{self, Node};
use std::collections::{BTreeMap, BTreeSet};

/// The name of the LuckPerms config file in `plugins/LuckPerms/`.
pub const CONFIG_YML: &str = "config.yml";
/// The contexts file next to it.
pub const CONTEXTS_JSON: &str = "contexts.json";

/// Keys the port reads on purpose and does not act on, with the reason. The `check_original` report
/// does not count them as missing. Mirrored in GAPS.md.
pub const IGNORED: &[(&str, &str)] = &[
    ("apply-bukkit-child-permissions", "Bukkit permission attachments do not exist on Horizon"),
    ("apply-bukkit-attachment-permissions", "Bukkit permission attachments do not exist on Horizon"),
    ("apply-sponge-implicit-wildcards", "Sponge only"),
    ("auto-install-translations", "English only (GAPS.md)"),
    ("register-command-list-data", "Bukkit command list data; Horizon builds its own tree"),
    ("cancel-failed-logins", "not read by LuckPerms' Bukkit module either (Velocity, Bungee and Forge only); a failed data load always refuses the login here (test a_failed_load_refuses_the_login_with_the_generic_reason)"),
    ("log-synchronously-in-commands", "commands run on the worker thread and write their log entry before the answer is sent; there is no asynchronous log dispatch to wait for"),
    ("chat-formatter.enabled", "deprecated; only LuckPerms' Hytale module reads it, Paper has no chat formatter"),
    ("integrated-server-owner-bypasses-checks", "concerns the owner of an integrated (singleplayer) server; a dedicated Horizon server has none, and LuckPerms' Paper module never reads it"),
    ("editor-lazily-generate-key", "the editor key is made when first needed (editorsocket.rs), which is what the lazy setting asks for"),
    ("use-server-uuid-cache", "Horizon's own player list is used"),
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

        // Split storage: which storage holds which data. A method the port cannot run is refused when it opens.
        set!(k, "split-storage.enabled", c.split_storage.enabled);
        if let Some(v) = k.text("split-storage.methods.user") {
            c.split_storage.methods.user = v;
        }
        if let Some(v) = k.text("split-storage.methods.group") {
            c.split_storage.methods.group = v;
        }
        if let Some(v) = k.text("split-storage.methods.track") {
            c.split_storage.methods.track = v;
        }
        if let Some(v) = k.text("split-storage.methods.uuid") {
            c.split_storage.methods.uuid = v;
        }
        if let Some(v) = k.text("split-storage.methods.log") {
            c.split_storage.methods.log = v;
        }

        // Messaging: SQL, Redis and NATS run; RabbitMQ, PostgreSQL and plugin messaging are refused.
        if let Some(v) = k.text("messaging-service") {
            c.messaging_service = v.trim().to_lowercase();
        }
        set!(k, "redis.enabled", c.redis.enabled);
        if let Some(v) = k.text("redis.address") {
            c.redis.address = v;
        }
        if let Some(v) = k.list("redis.addresses") {
            c.redis.addresses = v;
        }
        if let Some(v) = k.text("redis.username") {
            c.redis.username = v;
        }
        if let Some(v) = k.text("redis.password") {
            c.redis.password = v;
        }
        set!(k, "redis.ssl", c.redis.ssl);
        set!(k, "redis.sentinel.enabled", c.redis.sentinel.enabled);
        if let Some(v) = k.text("redis.sentinel.master") {
            c.redis.sentinel.master = v;
        }
        if let Some(v) = k.list("redis.sentinel.addresses") {
            c.redis.sentinel.addresses = v;
        }
        if let Some(v) = k.text("redis.sentinel.username") {
            c.redis.sentinel.username = v;
        }
        if let Some(v) = k.text("redis.sentinel.password") {
            c.redis.sentinel.password = v;
        }
        set!(k, "nats.enabled", c.nats.enabled);
        if let Some(v) = k.text("nats.address") {
            c.nats.address = v;
        }
        if let Some(v) = k.text("nats.username") {
            c.nats.username = v;
        }
        if let Some(v) = k.text("nats.password") {
            c.nats.password = v;
        }
        if let Some(v) = k.text("nats.token") {
            c.nats.token = v;
        }
        set!(k, "nats.ssl", c.nats.ssl);
        set!(k, "rabbitmq.enabled", c.rabbitmq.enabled);
        if let Some(v) = k.text("rabbitmq.address") {
            c.rabbitmq.address = v;
        }
        if let Some(v) = k.text("rabbitmq.vhost") {
            c.rabbitmq.vhost = v;
        }
        if let Some(v) = k.text("rabbitmq.username") {
            c.rabbitmq.username = v;
        }
        if let Some(v) = k.text("rabbitmq.password") {
            c.rabbitmq.password = v;
        }
        if let Some(v) = k.text("data.mongodb-collection-prefix") {
            c.storage.mongodb_collection_prefix = v;
        }
        if let Some(v) = k.text("data.mongodb-connection-uri") {
            c.storage.mongodb_connection_uri = v;
        }

        // `SYNC_TIME`: `sync-minutes`, else `data.sync-minutes`.
        if let Some(v) = k.number("sync-minutes") {
            c.sync_minutes = v;
        }
        if c.sync_minutes < 0 {
            if let Some(v) = k.number("data.sync-minutes") {
                c.sync_minutes = v;
            }
        }
        // `data.pool-size` is the old name of `data.pool-settings.maximum-pool-size`.
        if k.node("data.pool-settings.maximum-pool-size").is_none() {
            if let Some(v) = k.number("data.pool-size") {
                c.storage.maximum_pool_size = v.max(1) as usize;
            }
        }
        if let Some(v) = k.text("data.rest-url") {
            c.storage.rest_url = v;
        }
        if let Some(v) = k.text("data.rest-auth-key") {
            c.storage.rest_auth_key = v;
        }
        if let Some(v) = k.text("bytebin-url") {
            c.bytebin_url = v;
        }
        if let Some(v) = k.text("web-editor-url") {
            c.web_editor_url = v;
        }
        set!(k, "commands-rate-limit", c.commands_rate_limit);
        c.group_name_rewrite = k.texts("group-name-rewrite").into_iter().map(|(a, b)| (a.to_lowercase(), b)).collect();
        set!(k, "use-vault-server", c.use_vault_server);
        if let Some(v) = k.text("vault-server") {
            c.vault_server = v.to_lowercase();
        }
        set!(k, "vault-include-global", c.vault_include_global);
        set!(k, "vault-ignore-world", c.vault_ignore_world);
        set!(k, "vault-group-use-displaynames", c.vault_group_use_displaynames);
        if let Some(v) = k.text("vault-npc-group") {
            c.vault_npc_group = v;
        }
        set!(k, "vault-npc-op-status", c.vault_npc_op_status);
        set!(k, "vault-unsafe-lookups", c.vault_unsafe_lookups);
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
        set!(k, "disable-bulkupdate", c.disable_bulkupdate);
        set!(k, "skip-bulkupdate-confirmation", c.skip_bulkupdate_confirmation);
        set!(k, "debug-logins", c.debug_logins);
        set!(k, "allow-invalid-usernames", c.allow_invalid_usernames);
        if let Some(v) = k.text("context-satisfy-mode") {
            c.context_satisfy_all = v.trim().eq_ignore_ascii_case("all-values-per-key");
        }
        if let Some(v) = k.list("disabled-context-calculators") {
            c.disabled_context_calculators = v;
        }
        if let Some(v) = k.text("bytesocks-url") {
            c.bytesocks_url = v;
        }
        if let Some(v) = k.text("verbose-viewer-url") {
            c.verbose_viewer_url = v;
        }
        if let Some(v) = k.text("tree-viewer-url") {
            c.tree_viewer_url = v;
        }
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

        set!(k, "auto-op", c.auto_op);
        set!(k, "require-sender-group-membership-to-modify", c.require_sender_group_membership);
        set!(k, "argument-based-command-permissions", c.argument_based_command_permissions);
        set!(k, "apply-default-negated-permissions-before-wildcards", c.apply_default_negated_before_wildcards);
        if let Some(v) = k.text("meta-value-selection-default") {
            c.meta_value_selection_default = v.to_lowercase();
        }
        c.meta_value_selection = k.texts("meta-value-selection").into_iter().map(|(k, v)| (k.to_lowercase(), v.to_lowercase())).collect();
        if c.auto_op {
            // `OPS_ENABLED`: `!auto-op && enable-ops`.
            c.horizon.enable_ops = false;
        }

        // Read and not supported: a non-default value is noted (a key name only, never its value).
        set!(k, "resolve-command-selectors", c.resolve_command_selectors);
        if k.flag("update-client-command-list") == Some(false) {
            notes.push("update-client-command-list (set false; the port always updates the list)".into());
        }
    }
    for (path, _) in IGNORED {
        reads.mark(path);
    }
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
    fn messaging_service_follows_the_original_factory() {
        use crate::horizon_luckperms_core::config::Messaging;
        // Redis enabled under auto: Redis runs, with the address read.
        let (c, _) = load("redis:\n  enabled: true\n  address: example.test:6380\n");
        assert_eq!(c.messaging(), Messaging::Redis);
        assert!(!c.sql_messaging());
        assert_eq!(c.redis.address, "example.test:6380");
        // NATS likewise; Redis wins when both are on (auto order: redis, rabbitmq, nats).
        let (c, _) = load("nats:\n  enabled: true\n  address: example.test\n  token: not-a-token\n");
        assert_eq!(c.messaging(), Messaging::Nats);
        assert_eq!(c.nats.token, "not-a-token");
        let (c, _) = load("redis:\n  enabled: true\nnats:\n  enabled: true\n");
        assert_eq!(c.messaging(), Messaging::Redis);
        let (c, _) = load("rabbitmq:\n  enabled: true\n");
        assert_eq!(c.messaging(), Messaging::Rabbit);
        // An explicit sql service ignores the enabled flags.
        let (c, _) = load("messaging-service: sql\nredis:\n  enabled: true\n");
        assert_eq!(c.messaging(), Messaging::Sql);
        assert!(c.sql_messaging());
        // An explicit service whose flag is off is no service (LuckPerms warns and starts none).
        let (c, _) = load("messaging-service: redis\n");
        assert_eq!(c.messaging(), Messaging::Off);
        // `none` is read as `auto` (MessagingFactory); with SQL storage that is the SQL messenger, `notsql` is off.
        let (c, _) = load("messaging-service: none\n");
        assert_eq!(c.messaging(), Messaging::Sql);
        let (c, _) = load("messaging-service: notsql\n");
        assert_eq!(c.messaging(), Messaging::Off);
        // File storage has no SQL messenger under auto.
        let (c, _) = load("storage-method: yaml\n");
        assert_eq!(c.messaging(), Messaging::Off);
        let (c, _) = load("storage-method: postgresql\n");
        assert_eq!(c.messaging(), Messaging::Postgres);
        let (c, _) = load("messaging-service: pluginmsg\n");
        assert_eq!(c.messaging(), Messaging::PluginMsg);
    }

    #[test]
    fn redis_and_nats_credentials_are_read_and_never_debug_printed() {
        let (c, _) = load("redis:\n  enabled: true\n  address: r.example\n  username: u\n  password: hunter2-not-real\n  ssl: true\n  sentinel:\n    enabled: true\n    master: m1\n    addresses:\n      - s1.example:26380\n    password: sentinel-not-real\n");
        assert_eq!(c.redis.username, "u");
        assert_eq!(c.redis.password, "hunter2-not-real");
        assert!(c.redis.ssl && c.redis.sentinel.enabled);
        assert_eq!(c.redis.sentinel.master, "m1");
        assert_eq!(c.redis.sentinel.addresses, vec!["s1.example:26380"]);
        let shown = format!("{c:?}");
        assert!(!shown.contains("hunter2") && !shown.contains("sentinel-not-real"), "a password reached Debug output");
    }

    #[test]
    fn split_storage_methods_are_read() {
        let (c, _) = load("split-storage:\n  enabled: true\n  methods:\n    user: mariadb\n    group: yaml\n    track: yaml\n    uuid: sqlite\n    log: json\n");
        assert!(c.split_storage.enabled);
        assert_eq!(c.split_storage.methods.user, "mariadb");
        assert_eq!(c.split_storage.methods.group, "yaml");
        assert_eq!(c.split_storage.methods.uuid, "sqlite");
        assert_eq!(c.split_storage.methods.log, "json");
        assert_eq!(c.storage_methods(), vec!["mariadb", "yaml", "yaml", "sqlite", "json"]);
        // A MariaDB part makes `auto` pick the SQL messenger.
        assert!(c.sql_messaging());
        let (c, _) = load("split-storage:\n  enabled: true\n");
        assert_eq!(c.split_storage.methods.user, "h2", "unset methods default to h2 as in LuckPerms");
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
        assert!(c.auto_op && !c.horizon.enable_ops, "auto-op turns enable-ops off");
        assert!(c.use_vault_server);
        assert_eq!(c.meta_value_selection_default, "weight");
        let (c, _) = load("meta-value-selection:\n  Level: Highest-Number\nrequire-sender-group-membership-to-modify: true\n");
        assert_eq!(c.meta_value_selection.get("level").map(String::as_str), Some("highest-number"));
        assert!(c.require_sender_group_membership);
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
    fn satisfy_mode_calculators_and_debug_logins_are_read() {
        let (c, _) = load(
            "context-satisfy-mode: all-values-per-key\ndisabled-context-calculators:\n  - me.lucko.luckperms.bukkit.context.BukkitPlayerCalculator\ndebug-logins: true\n",
        );
        assert!(c.context_satisfy_all);
        assert!(c.calculator_disabled("me.lucko.luckperms.bukkit.context.bukkitplayercalculator"));
        assert!(!c.calculator_disabled("me.lucko.luckperms.common.context.calculator.ConfigurationContextCalculator"));
        assert!(c.debug_logins);
        assert!(c.unsupported.is_empty());
    }

    #[test]
    fn satisfy_mode_defaults_to_at_least_one() {
        let (c, _) = load("context-satisfy-mode: at-least-one-value-per-key\n");
        assert!(!c.context_satisfy_all);
        assert!(!c.debug_logins);
        assert!(c.disabled_context_calculators.is_empty());
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

#[cfg(test)]
mod round6_tests {
    use super::*;

    fn load(text: &str) -> Config {
        let mut reads = Reads::default();
        config_from_yaml(text, &mut reads).unwrap()
    }

    #[test]
    fn the_keys_the_audit_found_missing_are_read() {
        let c = load(
            "data:\n  sync-minutes: 9\n  pool-size: 7\n  rest-url: http://rest.example:8080/\n  rest-auth-key: k\nbytebin-url: https://b.example/\nweb-editor-url: https://e.example/\ncommands-rate-limit: false\ngroup-name-rewrite:\n  Admin: Administrator\nuse-vault-server: true\nvault-server: Hub\nvault-include-global: false\nvault-ignore-world: true\nvault-group-use-displaynames: false\nvault-npc-group: npcs\nvault-npc-op-status: true\n",
        );
        assert_eq!(c.sync_minutes, 9);
        assert_eq!(c.storage.maximum_pool_size, 7);
        assert_eq!(c.storage.rest_url, "http://rest.example:8080/");
        assert_eq!(c.storage.rest_auth_key, "k");
        assert_eq!(c.bytebin_url, "https://b.example/");
        assert_eq!(c.web_editor_url, "https://e.example/");
        assert!(!c.commands_rate_limit);
        assert_eq!(c.group_name_rewrite.get("admin").map(String::as_str), Some("Administrator"));
        assert!(c.use_vault_server && !c.vault_include_global && c.vault_ignore_world && !c.vault_group_use_displaynames && c.vault_npc_op_status);
        assert_eq!((c.vault_server.as_str(), c.vault_npc_group.as_str()), ("hub", "npcs"));
        let d = load("server: global\n");
        assert!(d.commands_rate_limit && d.vault_include_global && d.vault_group_use_displaynames);
    }
}
