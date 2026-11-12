//! `config/luckperms.toml`: the LuckPerms options that matter on a server, under their
//! LuckPerms names, plus a `[horizon]` table for the parts Horizon does differently (operator
//! levels instead of Bukkit's op flag).

use crate::model::TempMerge;
use serde::Deserialize;
use std::collections::BTreeMap;

pub const FILE_NAME: &str = "luckperms.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    /// The `server` context of this server; `global` adds none.
    pub server: String,
    pub storage: Storage,
    /// `sql` (the `{prefix}messenger` table), `auto` (the same with SQL storage) or `none`.
    pub messaging_service: String,
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
    /// `watch-files`: a saved config is reloaded within a second (on by default).
    pub watch_files: bool,
    /// The non-SQL messenger the original file would use (`redis`, `nats`, `rabbitmq`): the port does not
    /// speak those, so messaging stays off. Set by the original-file reader only.
    #[serde(skip)]
    pub messenger_refused: Option<String>,
    /// Settings of the original file this port reads but does not act on, as key paths with the value
    /// (no values of secrets). Logged once at start. Set by the original-file reader only.
    #[serde(skip)]
    pub unsupported: Vec<String>,
    /// `bytebin-url`: where the web editor's data is uploaded to and downloaded from.
    pub bytebin_url: String,
    /// `web-editor-url`: the editor's address; the bytebin key is appended.
    pub web_editor_url: String,
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
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            server: "global".into(),
            storage: Storage::default(),
            messaging_service: "auto".into(),
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
            watch_files: true,
            messenger_refused: None,
            unsupported: Vec::new(),
            bytebin_url: "https://usercontent.luckperms.net/".into(),
            web_editor_url: "https://luckperms.net/editor/".into(),
            horizon: Horizon::default(),
        }
    }
}

/// The file written when none exists. The password stays empty: fill it in by hand.
pub const DEFAULT_TOML: &str = include_str!("../luckperms.example.toml");

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

    /// Whether the SQL messenger runs (`messaging-service: auto` picks it with SQL storage).
    pub fn sql_messaging(&self) -> bool {
        if self.messenger_refused.is_some() {
            return false;
        }
        match self.messaging_service.to_lowercase().as_str() {
            "sql" => true,
            "auto" => matches!(self.storage.method.to_lowercase().as_str(), "mariadb" | "mysql"),
            _ => false,
        }
    }

    pub fn global_flags(&self) -> crate::context::GlobalFlags {
        crate::context::GlobalFlags {
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
        // The shipped example keeps everything in memory, so a fresh install reaches no database;
        // SQL messaging only applies once a server switches the storage to MariaDB/MySQL.
        assert!(!c.sql_messaging());
        let sql = DEFAULT_TOML.replace("method = \"memory\"", "method = \"mariadb\"");
        assert!(Config::parse(&sql).expect("mariadb variant parses").sql_messaging());
        assert_eq!(c.temporary_add_behaviour, TempMerge::Deny);
        assert_eq!(c.meta_formatting.prefix.format, vec!["highest".to_string()]);
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
