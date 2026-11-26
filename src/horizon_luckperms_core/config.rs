//! `config/luckperms.toml`: the LuckPerms options that matter on a server, under their
//! LuckPerms names, plus a `[horizon]` table for the parts Horizon does differently (operator
//! levels instead of Bukkit's op flag).

use crate::horizon_luckperms_core::model::TempMerge;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

pub const FILE_NAME: &str = "luckperms.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// The `server` context of this server; `global` adds none.
    pub server: String,
    pub storage: Storage,
    /// `split-storage`: user, group, track, uuid and log data in different storages.
    pub split_storage: SplitStorage,
    /// `auto`, `sql`, `redis`, `nats`, `rabbitmq`, `pluginmsg`, `notsql`, `custom` or `none` (LuckPerms reads `none` as
    /// `auto`). `auto` picks Redis, RabbitMQ or NATS when one is enabled, else SQL with MariaDB/MySQL storage.
    pub messaging_service: String,
    /// `redis.*`
    pub redis: Redis,
    /// `nats.*`
    pub nats: Nats,
    /// `rabbitmq.enabled` (the rest of `rabbitmq.*` is only read: that messenger needs an AMQP client).
    pub rabbitmq: RabbitMq,
    /// Full resync from storage every N minutes; -1 never.
    pub sync_minutes: i64,
    pub auto_push_updates: bool,
    pub push_log_entries: bool,
    pub broadcast_received_log_entries: bool,
    pub log_notify: bool,
    pub temporary_add_behaviour: TempMerge,
    /// `stored`, `parents-by-weight` or `all-parents-by-weight`.
    pub primary_group_calculation: String,
    /// `depth-first-pre-order`, `depth-first-post-order` or `breadth-first`.
    pub inheritance_traversal_algorithm: String,
    pub post_traversal_inheritance_sort: bool,
    pub include_global: bool,
    pub include_global_world: bool,
    pub apply_global_groups: bool,
    pub apply_global_world_groups: bool,
    pub apply_wildcards: bool,
    pub apply_regex: bool,
    pub apply_shorthand: bool,
    pub prevent_primary_group_removal: bool,
    pub meta_formatting: MetaFormatting,
    /// Overrides of group weights (`group-weight`).
    pub group_weight: BTreeMap<String, i32>,
    /// World name rewrites (`world-rewrite`): `world_nether = "world"`.
    pub world_rewrite: BTreeMap<String, String>,
    /// Context keys never calculated (`disabled-contexts`).
    pub disabled_contexts: Vec<String>,
    /// `contexts.json`'s `static-contexts`: added to every player on this server.
    pub static_contexts: BTreeMap<String, String>,
    /// `allow-invalid-usernames`: user arguments that are no valid Minecraft name are looked up anyway.
    pub allow_invalid_usernames: bool,
    /// `disable-luckperms-commands`: `/lp` answers "LuckPerms commands are disabled" for players and/or the console.
    pub disable_luckperms_commands: PlayersConsole,
    /// `commands-read-only-mode`: only the read-only commands work for players and/or the console.
    pub commands_read_only_mode: PlayersConsole,
    /// `log-notify-filtered-descriptions`: regexes; a log entry whose description matches one is not announced.
    pub log_notify_filtered_descriptions: Vec<String>,
    /// `contexts.json`'s `default-contexts` (read; LuckPerms applies them when no context is known,
    /// the port does not yet: see GAPS.md).
    pub default_contexts: BTreeMap<String, String>,
    /// `apply-bukkit-default-permissions`: nodes registered as Bukkit default-true (`api::register_default_true`)
    /// are true for everyone their permissions leave undefined.
    pub apply_bukkit_default_permissions: bool,
    /// `auto-op`: operator status follows the node `luckperms.autoop`; `enable-ops` is then off.
    pub auto_op: bool,
    /// `require-sender-group-membership-to-modify`: a sender may only change groups they are a member of.
    pub require_sender_group_membership: bool,
    /// `argument-based-command-permissions`: `/lp` sub-commands also check the node of the argument.
    pub argument_based_command_permissions: bool,
    /// `apply-default-negated-permissions-before-wildcards`: a negated default node beats a wildcard.
    pub apply_default_negated_before_wildcards: bool,
    /// `meta-value-selection-default`: `inheritance`, `highest-number` or `lowest-number`.
    pub meta_value_selection_default: String,
    /// `meta-value-selection`: the strategy per meta key.
    pub meta_value_selection: BTreeMap<String, String>,
    /// `resolve-command-selectors`: `@p`, `@a[...]` in `/lp` arguments name the one player they select.
    pub resolve_command_selectors: bool,
    /// `watch-files`: a saved config is reloaded within a second (on by default).
    pub watch_files: bool,
    /// The directory the file storages and the SQLite file live in (`plugins/LuckPerms/` for the original
    /// config, the mod's config directory otherwise). Set by the loader.
    #[serde(skip)]
    pub data_dir: PathBuf,
    /// Settings of the original file this port reads but does not act on, as key paths with the value
    /// (no values of secrets). Logged once at start. Set by the original-file reader only.
    #[serde(skip)]
    pub unsupported: Vec<String>,
    /// `bytebin-url`: where the web editor's data is uploaded to and downloaded from.
    pub bytebin_url: String,
    /// `web-editor-url`: the editor's address; the bytebin key is appended.
    pub web_editor_url: String,
    /// `bytesocks-url`: the service the web editor's live connection runs on (`/lp trusteditor`).
    pub bytesocks_url: String,
    /// `verbose-viewer-url`: the verbose viewer's address; the bytebin key is appended.
    pub verbose_viewer_url: String,
    /// `tree-viewer-url`: the permission tree viewer's address; the bytebin key is appended.
    pub tree_viewer_url: String,
    /// `disable-bulkupdate`: `/lp bulkupdate` answers that it is disabled.
    pub disable_bulkupdate: bool,
    /// `skip-bulkupdate-confirmation`: `/lp bulkupdate` runs at once, without the `confirm` step.
    pub skip_bulkupdate_confirmation: bool,
    /// `context-satisfy-mode: all-values-per-key` (otherwise at-least-one-value-per-key).
    pub context_satisfy_all: bool,
    /// `disabled-context-calculators`: class-name prefixes (Bukkit names) whose contexts are not calculated.
    pub disabled_context_calculators: Vec<String>,
    /// `debug-logins`: a log line when the housekeeper unloads a user.
    pub debug_logins: bool,
    /// `commands-rate-limit`: a player's `/lp` within 500 ms of their previous one is ignored.
    pub commands_rate_limit: bool,
    /// `group-name-rewrite`: group name -> display name used when a group has no display-name node.
    pub group_name_rewrite: BTreeMap<String, String>,
    /// `use-vault-server`: the Vault provider reads and writes under the server context `vault-server`.
    pub use_vault_server: bool,
    /// `vault-server` (lower case).
    pub vault_server: String,
    /// `vault-include-global`: Vault lookups also count nodes without a server context.
    pub vault_include_global: bool,
    /// `vault-ignore-world`: Vault calls ignore the world they are given.
    pub vault_ignore_world: bool,
    /// `vault-group-use-displaynames`: Vault answers group display names instead of names.
    pub vault_group_use_displaynames: bool,
    /// `vault-npc-group`: the group whose data an NPC (a version 2 UUID) gets.
    pub vault_npc_group: String,
    /// `vault-npc-op-status`: NPCs count as operators.
    pub vault_npc_op_status: bool,
    /// `vault-unsafe-lookups`: read; the provider never reads storage on the caller's thread, so it has no effect.
    pub vault_unsafe_lookups: bool,
    pub horizon: Horizon,
}

/// The `players`/`console` pairs of LuckPerms' config.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct PlayersConsole {
    pub players: bool,
    pub console: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Storage {
    /// `mariadb` / `mysql`; `memory` keeps everything in RAM (tests, a lone server).
    pub method: String,
    /// `host:port`.
    pub address: String,
    pub database: String,
    pub username: String,
    pub password: String,
    pub table_prefix: String,
    pub maximum_pool_size: usize,
    pub minimum_idle: usize,
    pub connection_timeout_ms: u64,
    /// TLS to the database (rustls).
    pub use_ssl: bool,
    /// `pool-settings.keepalive-time` in ms, 0 off: the TCP keepalive of each connection.
    pub keepalive_time_ms: u64,
    /// `pool-settings.maximum-lifetime` in ms. Read but not applied: the MySQL client's pool has no lifetime.
    pub maximum_lifetime_ms: u64,
    /// `pool-settings.properties` (`useUnicode`, `characterEncoding`). Read but not applied: the port always
    /// connects as utf8mb4, a superset of the original utf8.
    pub pool_properties: BTreeMap<String, String>,
    /// `data.rest-url` (`storage-method: rest`): the base URL of a LuckPerms REST API.
    pub rest_url: String,
    /// `data.rest-auth-key`: sent as a Bearer token when not empty.
    pub rest_auth_key: String,
    /// `data.mongodb-collection-prefix`.
    pub mongodb_collection_prefix: String,
    /// `data.mongodb-connection-uri`: used instead of address and credentials when not empty.
    pub mongodb_connection_uri: String,
}


/// `split-storage`: which storage holds which data. The methods default to `h2`, as in LuckPerms.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SplitStorage {
    pub enabled: bool,
    pub methods: SplitMethods,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct SplitMethods {
    pub user: String,
    pub group: String,
    pub track: String,
    pub uuid: String,
    pub log: String,
}

impl Default for SplitStorage {
    fn default() -> Self {
        SplitStorage { enabled: false, methods: SplitMethods::default() }
    }
}

impl Default for SplitMethods {
    fn default() -> Self {
        let h2 = || "h2".to_owned();
        SplitMethods { user: h2(), group: h2(), track: h2(), uuid: h2(), log: h2() }
    }
}

/// `redis.sentinel.*`
#[derive(Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Sentinel {
    pub enabled: bool,
    pub master: String,
    pub addresses: Vec<String>,
    pub username: String,
    pub password: String,
}

impl Default for Sentinel {
    fn default() -> Self {
        Sentinel { enabled: false, master: "mymaster".into(), addresses: Vec::new(), username: String::new(), password: String::new() }
    }
}

/// `redis.*`
#[derive(Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Redis {
    pub enabled: bool,
    /// `host:port`, port 6379 when left out; IPv6 hosts in brackets.
    pub address: String,
    /// A Redis cluster's nodes. Pub/sub reaches every node of a cluster, so the first node is used.
    pub addresses: Vec<String>,
    pub username: String,
    pub password: String,
    pub ssl: bool,
    pub sentinel: Sentinel,
}

impl Default for Redis {
    fn default() -> Self {
        Redis {
            enabled: false,
            address: String::new(),
            addresses: Vec::new(),
            username: String::new(),
            password: String::new(),
            ssl: false,
            sentinel: Sentinel::default(),
        }
    }
}

/// `nats.*`
#[derive(Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Nats {
    pub enabled: bool,
    /// `host:port`, port 4222 when left out.
    pub address: String,
    pub username: String,
    pub password: String,
    pub token: String,
    pub ssl: bool,
}

impl Default for Nats {
    fn default() -> Self {
        Nats { enabled: false, address: String::new(), username: String::new(), password: String::new(), token: String::new(), ssl: false }
    }
}

#[derive(Clone, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct RabbitMq {
    pub enabled: bool,
    /// `host[:port]` (5672).
    pub address: String,
    pub vhost: String,
    pub username: String,
    pub password: String,
}

// Credentials never reach a log through `{:?}`.
impl fmt::Debug for RabbitMq {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RabbitMq").field("enabled", &self.enabled).finish_non_exhaustive()
    }
}

impl Default for RabbitMq {
    fn default() -> Self {
        RabbitMq { enabled: false, address: "localhost".into(), vhost: "/".into(), username: "guest".into(), password: "guest".into() }
    }
}

// Credentials never reach a log through `{:?}`.
impl fmt::Debug for Sentinel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sentinel").field("enabled", &self.enabled).field("master", &self.master).finish_non_exhaustive()
    }
}
impl fmt::Debug for Redis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Redis").field("enabled", &self.enabled).field("ssl", &self.ssl).finish_non_exhaustive()
    }
}
impl fmt::Debug for Nats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Nats").field("enabled", &self.enabled).field("ssl", &self.ssl).finish_non_exhaustive()
    }
}

/// The messenger a config selects (`MessagingFactory.getInstance`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Messaging {
    /// No messaging service.
    Off,
    /// The `{prefix}messenger` table of a MariaDB/MySQL storage.
    Sql,
    Redis,
    Nats,
    /// AMQP, LISTEN/NOTIFY and plugin messages.
    Rabbit,
    Postgres,
    PluginMsg,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct MetaFormatting {
    pub prefix: StackFormat,
    pub suffix: StackFormat,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct StackFormat {
    pub format: Vec<String>,
    /// `first-only`, `last-only` or `retain-all`.
    pub duplicates: String,
    pub start_spacer: String,
    pub middle_spacer: String,
    pub end_spacer: String,
}

impl Default for StackFormat {
    fn default() -> Self {
        StackFormat {
            format: vec!["highest".into()],
            duplicates: "first-only".into(),
            start_spacer: String::new(),
            middle_spacer: " ".into(),
            end_spacer: String::new(),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Horizon {
    /// Players of this operator level or more are "op" (Bukkit's `isOp`): every permission not
    /// set false is true for them (`enable-ops`).
    pub op_level: u8,
    pub enable_ops: bool,
    /// Ops may use `/lp` without `luckperms.*` (`commands-allow-op`).
    pub commands_allow_op: bool,
    /// Gate every typed command through `minecraft.command.<name>` (as Paper names vanilla's
    /// commands) or the node `command-nodes` gives it: false refuses it.
    pub command_permissions: bool,
    /// Mod and alias commands mapped to the node that guards them (`tpa = "essentials.tpa"`).
    pub command_nodes: BTreeMap<String, String>,
    /// A command whose node is true but whose operator level the player lacks is run through
    /// `execute as <player> run ...` from the console at the next tick. Its answer goes to the
    /// log, not the player: a stopgap until Horizon can raise a command's level (GAPS.md).
    pub grant_via_execute: bool,
    /// The line a refused command answers with.
    pub no_permission_message: String,
    /// The `world` context per dimension; missing ones are derived from the level name
    /// (`world`, `world_nether`, `world_the_end`, as Paper names them).
    pub world_names: BTreeMap<String, String>,
    /// Seconds a user stays loaded after they leave (LuckPerms: about a minute).
    pub unload_delay_seconds: u64,
    /// Milliseconds the login waits for the player's data (Paper refuses the connection when the load fails).
    /// A failed or late load refuses the login with a reason. 0: the player is let in at once with `default`
    /// rights and kicked after five seconds when the load fails (the older behaviour).
    pub login_wait_ms: u64,
    /// The language of the messages (`translations/<locale>.properties`); `en` is the shipped English.
    pub locale: String,
}

impl Default for Horizon {
    fn default() -> Self {
        Horizon {
            op_level: 1,
            enable_ops: true,
            commands_allow_op: true,
            command_permissions: true,
            command_nodes: BTreeMap::new(),
            grant_via_execute: false,
            no_permission_message: "§cI'm sorry, but you do not have permission to perform this command.".into(),
            world_names: BTreeMap::new(),
            unload_delay_seconds: 60,
            login_wait_ms: 3000,
            locale: "en".into(),
        }
    }
}

impl Default for Storage {
    fn default() -> Self {
        Storage {
            method: "mariadb".into(),
            address: "localhost".into(),
            database: "minecraft".into(),
            username: "root".into(),
            password: String::new(),
            table_prefix: "luckperms_".into(),
            maximum_pool_size: 4,
            minimum_idle: 1,
            connection_timeout_ms: 5000,
            use_ssl: false,
            keepalive_time_ms: 0,
            maximum_lifetime_ms: 0,
            pool_properties: BTreeMap::new(),
            rest_url: "http://localhost:8080/".into(),
            rest_auth_key: String::new(),
            mongodb_collection_prefix: String::new(),
            mongodb_connection_uri: String::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: "global".into(),
            storage: Storage::default(),
            split_storage: SplitStorage::default(),
            messaging_service: "auto".into(),
            redis: Redis::default(),
            nats: Nats::default(),
            rabbitmq: RabbitMq::default(),
            sync_minutes: -1,
            auto_push_updates: true,
            push_log_entries: true,
            broadcast_received_log_entries: true,
            log_notify: true,
            temporary_add_behaviour: TempMerge::Deny,
            primary_group_calculation: "parents-by-weight".into(),
            inheritance_traversal_algorithm: "depth-first-pre-order".into(),
            post_traversal_inheritance_sort: false,
            include_global: true,
            include_global_world: true,
            apply_global_groups: true,
            apply_global_world_groups: true,
            apply_wildcards: true,
            apply_regex: true,
            apply_shorthand: true,
            prevent_primary_group_removal: false,
            meta_formatting: MetaFormatting::default(),
            group_weight: BTreeMap::new(),
            world_rewrite: BTreeMap::new(),
            disabled_contexts: Vec::new(),
            static_contexts: BTreeMap::new(),
            allow_invalid_usernames: false,
            disable_luckperms_commands: PlayersConsole::default(),
            commands_read_only_mode: PlayersConsole::default(),
            log_notify_filtered_descriptions: Vec::new(),
            default_contexts: BTreeMap::new(),
            apply_bukkit_default_permissions: true,
            auto_op: false,
            resolve_command_selectors: false,
            require_sender_group_membership: false,
            argument_based_command_permissions: false,
            apply_default_negated_before_wildcards: false,
            meta_value_selection_default: "inheritance".into(),
            meta_value_selection: BTreeMap::new(),
            watch_files: true,
            data_dir: PathBuf::new(),
            unsupported: Vec::new(),
            bytebin_url: "https://usercontent.luckperms.net/".into(),
            web_editor_url: "https://luckperms.net/editor/".into(),
            bytesocks_url: "https://usersockets.luckperms.net/".into(),
            verbose_viewer_url: "https://luckperms.net/verbose/".into(),
            tree_viewer_url: "https://luckperms.net/treeview/".into(),
            disable_bulkupdate: false,
            skip_bulkupdate_confirmation: false,
            context_satisfy_all: false,
            disabled_context_calculators: Vec::new(),
            debug_logins: false,
            commands_rate_limit: true,
            group_name_rewrite: BTreeMap::new(),
            use_vault_server: false,
            vault_server: "global".into(),
            vault_include_global: true,
            vault_ignore_world: false,
            vault_group_use_displaynames: true,
            vault_npc_group: "default".into(),
            vault_npc_op_status: false,
            vault_unsafe_lookups: false,
            horizon: Horizon::default(),
        }
    }
}

/// The file written when none exists. The password stays empty: fill it in by hand.
pub const DEFAULT_TOML: &str = include_str!("../../data/luckperms/luckperms.example.toml");

impl Config {
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut c: Config = toml::from_str(text).map_err(|e| e.to_string())?;
        c.server = c.server.trim().to_lowercase();
        if c.server.is_empty() {
            c.server = "global".into();
        }
        c.storage.table_prefix = c.storage.table_prefix.trim().to_owned();
        Ok(c)
    }

    /// The storage methods in use: the five split methods, or the one `storage-method`.
    pub fn storage_methods(&self) -> Vec<String> {
        let m = &self.split_storage.methods;
        if self.split_storage.enabled {
            [&m.user, &m.group, &m.track, &m.uuid, &m.log].iter().map(|s| s.trim().to_lowercase()).collect()
        } else {
            vec![self.storage.method.trim().to_lowercase()]
        }
    }

    /// `MessagingFactory.getInstance`: the messenger the config selects.
    pub fn messaging(&self) -> Messaging {
        let mut kind = self.messaging_service.trim().to_lowercase();
        if kind == "none" {
            kind = "auto".into();
        }
        let methods = self.storage_methods();
        if kind == "auto" {
            kind = if self.redis.enabled {
                "redis".into()
            } else if self.rabbitmq.enabled {
                "rabbitmq".into()
            } else if self.nats.enabled {
                "nats".into()
            } else if methods.iter().any(|m| matches!(m.as_str(), "mariadb" | "mysql")) {
                "sql".into()
            } else if methods.iter().any(|m| m == "postgresql") {
                "postgresql".into()
            } else {
                "auto".into()
            };
        }
        match kind.as_str() {
            "redis" if self.redis.enabled => Messaging::Redis,
            "nats" if self.nats.enabled => Messaging::Nats,
            "rabbitmq" if self.rabbitmq.enabled => Messaging::Rabbit,
            "pluginmsg" | "bungee" | "velocity" => Messaging::PluginMsg,
            "postgresql" => Messaging::Postgres,
            "sql" => Messaging::Sql,
            _ => Messaging::Off,
        }
    }

    /// Whether the SQL messenger runs (`messaging-service: auto` picks it with MariaDB/MySQL storage).
    pub fn sql_messaging(&self) -> bool {
        self.messaging() == Messaging::Sql
    }

    pub fn global_flags(&self) -> crate::horizon_luckperms_core::context::GlobalFlags {
        crate::horizon_luckperms_core::context::GlobalFlags {
            include_global: self.include_global,
            include_global_world: self.include_global_world,
            apply_global_groups: self.apply_global_groups,
            apply_global_world_groups: self.apply_global_world_groups,
        }
    }

    /// The `world` context for a dimension of a level called `level_name`.
    pub fn world_name(&self, dimension: &str, level_name: &str) -> String {
        if let Some(n) = self.horizon.world_names.get(dimension) {
            return n.clone();
        }
        let name = match dimension {
            "minecraft:overworld" => level_name.to_owned(),
            "minecraft:the_nether" => format!("{level_name}_nether"),
            "minecraft:the_end" => format!("{level_name}_the_end"),
            other => {
                let path = other.rsplit(':').next().unwrap_or(other);
                format!("{level_name}_{path}")
            }
        };
        name
    }

    /// The real world name followed by its `world-rewrite` aliases, applied recursively (each
    /// name once): LuckPerms adds the aliases in addition to the real name.
    /// Whether `disabled-context-calculators` names this calculator class (a prefix, ignoring case).
    pub fn calculator_disabled(&self, class: &str) -> bool {
        let class = class.to_lowercase();
        self.disabled_context_calculators.iter().any(|p| class.starts_with(&p.to_lowercase()))
    }

    pub fn world_names(&self, dimension: &str, level_name: &str) -> Vec<String> {
        let mut out = vec![self.world_name(dimension, level_name)];
        while let Some(next) = self.world_rewrite.get(out.last().map(String::as_str).unwrap_or_default()) {
            if out.iter().any(|n| n.eq_ignore_ascii_case(next)) {
                break;
            }
            out.push(next.clone());
        }
        out
    }

    /// The node that guards a typed command's root word.
    pub fn command_node(&self, root: &str) -> String {
        let root = root.to_lowercase();
        let root = root.strip_prefix("minecraft:").unwrap_or(&root).to_owned();
        self.horizon.command_nodes.get(&root).cloned().unwrap_or_else(|| format!("minecraft.command.{root}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_parses_and_matches_defaults() {
        let c = Config::parse(DEFAULT_TOML).expect("example config parses");
        assert_eq!(c.server, "global");
        assert_eq!(c.storage.table_prefix, "luckperms_");
        // The shipped example uses the database (it waits for the password); memory is opt-in.
        assert_eq!(c.storage.method, "mariadb");
        assert!(c.storage.password.is_empty());
        assert!(c.sql_messaging());
        let mem = DEFAULT_TOML.replace("method = \"mariadb\"", "method = \"memory\"");
        assert!(!Config::parse(&mem).expect("memory variant parses").sql_messaging());
        assert_eq!(c.temporary_add_behaviour, TempMerge::Deny);
        assert_eq!(c.meta_formatting.prefix.format, vec!["highest".to_string()]);
    }

    #[test]
    fn redis_nats_and_split_storage_sections_parse() {
        let c = Config::parse(
            "messaging-service = \"auto\"\n[redis]\nenabled = true\naddress = \"r.example:6380\"\npassword = \"not-real\"\n[redis.sentinel]\nenabled = true\nmaster = \"m\"\naddresses = [\"s.example\"]\n[nats]\nenabled = true\naddress = \"n.example\"\ntoken = \"t\"\n[split-storage]\nenabled = true\n[split-storage.methods]\nuser = \"yaml\"\nuuid = \"sqlite\"\n",
        )
        .unwrap();
        assert_eq!(c.messaging(), Messaging::Redis, "auto prefers Redis over NATS and SQL");
        assert_eq!((c.redis.address.as_str(), c.redis.password.as_str()), ("r.example:6380", "not-real"));
        assert!(c.redis.sentinel.enabled && c.redis.sentinel.addresses == ["s.example"]);
        assert_eq!(c.nats.token, "t");
        assert_eq!(c.storage_methods(), vec!["yaml", "h2", "h2", "sqlite", "h2"]);
        assert!(!format!("{c:?}").contains("not-real"), "no password in Debug output");
        // The shipped example has the sections and keeps them off.
        let ex = Config::parse(DEFAULT_TOML).unwrap();
        assert!(!ex.redis.enabled && !ex.nats.enabled && !ex.split_storage.enabled && !ex.redis.sentinel.enabled);
        assert_eq!(ex.bytesocks_url, "https://usersockets.luckperms.net/");
        assert_eq!(ex.messaging(), Messaging::Sql);
    }

    #[test]
    fn messaging_choices() {
        let mut c = Config::default();
        assert_eq!(c.messaging(), Messaging::Sql);
        c.storage.method = "sqlite".into();
        assert_eq!(c.messaging(), Messaging::Off, "SQLite cannot carry the SQL messenger");
        c.messaging_service = "sql".into();
        assert_eq!(c.messaging(), Messaging::Sql, "an explicit sql is tried (the runtime refuses it without a MariaDB storage)");
        c.messaging_service = "notsql".into();
        assert_eq!(c.messaging(), Messaging::Off);
        c.messaging_service = "custom".into();
        assert_eq!(c.messaging(), Messaging::Off);
        c.messaging_service = "Redis".into();
        c.redis.enabled = true;
        assert_eq!(c.messaging(), Messaging::Redis);
        c.messaging_service = "nats".into();
        assert_eq!(c.messaging(), Messaging::Off, "named, but not enabled");
        c.nats.enabled = true;
        assert_eq!(c.messaging(), Messaging::Nats);
    }

    #[test]
    fn empty_is_default() {
        let c = Config::parse("").unwrap();
        assert_eq!(c.primary_group_calculation, "parents-by-weight");
        assert_eq!(c.horizon.op_level, 1);
    }

    #[test]
    fn worlds_and_commands() {
        let mut c = Config::parse("server = \"Survival\"").unwrap();
        assert_eq!(c.server, "survival");
        assert_eq!(c.world_name("minecraft:the_nether", "world"), "world_nether");
        assert_eq!(c.world_name("minecraft:overworld", "world"), "world");
        assert_eq!(c.world_name("mymod:mining", "world"), "world_mining");
        c.world_rewrite.insert("world_nether".into(), "hell".into());
        c.world_rewrite.insert("hell".into(), "world_nether".into());
        assert_eq!(c.world_name("minecraft:the_nether", "world"), "world_nether");
        assert_eq!(c.world_names("minecraft:the_nether", "world"), vec!["world_nether", "hell"]);
        assert_eq!(c.world_names("minecraft:overworld", "world"), vec!["world"]);
        assert_eq!(c.command_node("gamemode"), "minecraft.command.gamemode");
        assert_eq!(c.command_node("minecraft:tp"), "minecraft.command.tp");
        c.horizon.command_nodes.insert("tpa".into(), "essentials.tpa".into());
        assert_eq!(c.command_node("TPA"), "essentials.tpa");
    }
}
