//! What the Horizon helpers call: everything they do, minus the engine types, so it can be
//! tested here. The helpers (in `glue/` next to `horizon-mixin.toml`) only convert.

use crate::command::ROOTS;
use crate::config::{Config, DEFAULT_TOML, FILE_NAME};
use crate::original::{self, Reads, CONFIG_YML, CONTEXTS_JSON};
use crate::runtime::{self, CommandSender, OnlinePlayer, Outgoing};
use crate::storage::{MemoryStorage, Storage};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// The LuckPerms folder of the Paper server, as the engine's working directory (the server folder) holds it.
pub const ORIGINAL_DIR: &str = "plugins/LuckPerms";

/// Where the settings come from: the original files (`plugins/LuckPerms/config.yml` and `contexts.json`),
/// or the port's own TOML in `config/` when the original is missing.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Source {
    Original { dir: PathBuf },
    Toml { path: PathBuf },
}

struct Watch {
    source: Source,
    stamps: Vec<Option<SystemTime>>,
}

fn watch() -> &'static Mutex<Option<Watch>> {
    static W: Mutex<Option<Watch>> = Mutex::new(None);
    &W
}

fn log(s: &str) {
    eprintln!("[LuckPerms] {s}");
}

fn modified(p: &Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

fn files(src: &Source) -> Vec<PathBuf> {
    match src {
        Source::Original { dir } => vec![dir.join(CONFIG_YML), dir.join(CONTEXTS_JSON)],
        Source::Toml { path } => vec![path.clone()],
    }
}

fn stamps(src: &Source) -> Vec<Option<SystemTime>> {
    files(src).iter().map(|p| modified(p)).collect()
}

/// Reads the settings from `src`. Only the original files' own keys are taken from them; a failure says
/// why the service does not start (no values).
fn load(src: &Source) -> Result<Config, String> {
    match src {
        Source::Toml { path } => {
            let text = std::fs::read_to_string(path).unwrap_or_default();
            Config::parse(&text)
        }
        Source::Original { dir } => {
            let text = std::fs::read_to_string(dir.join(CONFIG_YML)).map_err(|e| format!("{CONFIG_YML} cannot be read: {}", e.kind()))?;
            let mut reads = Reads::default();
            let mut cfg = original::config_from_yaml(&text, &mut reads)?;
            let ctx = dir.join(CONTEXTS_JSON);
            if ctx.exists() {
                let text = std::fs::read_to_string(&ctx).map_err(|e| format!("{CONTEXTS_JSON} cannot be read: {}", e.kind()))?;
                let (statics, defaults) = original::contexts_from_json(&text, &mut reads)?;
                cfg.static_contexts = statics;
                cfg.default_contexts = defaults;
            }
            Ok(cfg)
        }
    }
}

/// Starts the service. The original `plugins/LuckPerms/config.yml` is used when it exists (read unchanged,
/// nothing is written there). Otherwise `config/luckperms.toml` is read, and written with the defaults and an
/// empty password when missing; the service then starts only once the password is filled in.
pub fn start(config_dir: &Path, level_name: &str) {
    let src = if Path::new(ORIGINAL_DIR).join(CONFIG_YML).exists() {
        Source::Original { dir: PathBuf::from(ORIGINAL_DIR) }
    } else {
        let path = config_dir.join(FILE_NAME);
        if !path.exists() {
            if let Err(e) = std::fs::write(&path, DEFAULT_TOML) {
                log(&format!("could not write {}: {e}", path.display()));
            } else {
                log(&format!("wrote {}; fill in [storage] and restart", path.display()));
            }
        }
        Source::Toml { path }
    };
    let cfg = match load(&src) {
        Ok(c) => c,
        Err(e) => {
            log(&format!("{e}; not started"));
            return;
        }
    };
    for n in &cfg.unsupported {
        log(&format!("not supported, ignored: {n}"));
    }
    let storage: Box<dyn Storage> = match open_storage(&cfg) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("storage not opened: {e}; not started"));
            return;
        }
    };
    *watch().lock().unwrap_or_else(|e| e.into_inner()) = Some(Watch { stamps: stamps(&src), source: src });
    if let Err(e) = runtime::start(cfg, storage, level_name) {
        log(&format!("not started: {e}"));
    }
}

fn open_storage(cfg: &Config) -> Result<Box<dyn Storage>, String> {
    match cfg.storage.method.to_lowercase().as_str() {
        "memory" => Ok(Box::new(MemoryStorage::default())),
        "mariadb" | "mysql" => {
            if cfg.storage.password.is_empty() {
                return Err("storage.password is empty (fill it in, or use method = \"memory\" for a lone test server)".into());
            }
            sql_storage(cfg)
        }
        other => Err(format!("storage method {other} is not supported (mariadb, mysql, memory)")),
    }
}

#[cfg(feature = "mysql")]
fn sql_storage(cfg: &Config) -> Result<Box<dyn Storage>, String> {
    Ok(Box::new(crate::storage::sql::SqlStorage::new(&cfg.storage)?))
}

#[cfg(not(feature = "mysql"))]
fn sql_storage(_cfg: &Config) -> Result<Box<dyn Storage>, String> {
    Err("built without the mysql feature".into())
}

pub fn stop() {
    runtime::stop();
}

/// Once a second at most: a saved config is reloaded (storage changes need a restart). Off with `watch-files: false`.
pub fn check_config() {
    if !runtime::get().is_none_or(|rt| rt.config().watch_files) {
        return;
    }
    let mut w = watch().lock().unwrap_or_else(|e| e.into_inner());
    let Some(w) = w.as_mut() else { return };
    let now = stamps(&w.source);
    if now == w.stamps {
        return;
    }
    w.stamps = now;
    match load(&w.source) {
        Ok(c) => {
            for n in &c.unsupported {
                log(&format!("not supported, ignored: {n}"));
            }
            if let Some(rt) = runtime::get() {
                rt.reload_config(c);
                log("config reloaded");
            }
        }
        Err(e) => log(&format!("config not reloaded: {e}")),
    }
}

/// What a tick hands back to the game.
#[derive(Debug, Default)]
pub struct TickOut {
    pub lines: Vec<Outgoing>,
    pub console_commands: Vec<String>,
    /// Groups, users or tracks changed since the last tick: the players' command trees are
    /// sent again (`native_resend_commands`), so granted or revoked commands show at once.
    pub resend_trees: bool,
}

/// True when `now` differs from the version last seen in `last` (and remembers it).
fn changed_since(last: &std::sync::atomic::AtomicU64, now: u64) -> bool {
    last.swap(now, std::sync::atomic::Ordering::Relaxed) != now
}

/// The players now (when the helper read them this tick), then what to send and run.
pub fn tick(players: Option<Vec<OnlinePlayer>>) -> TickOut {
    let Some(rt) = runtime::get() else { return TickOut::default() };
    if let Some(p) = players {
        rt.update_players(p);
    }
    static SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let resend_trees = changed_since(&SEEN, rt.model_version());
    TickOut { lines: rt.drain_outbox(), console_commands: rt.drain_console_commands(), resend_trees }
}

/// `native_permission_check`: LuckPerms' answer for a node, None when it has no opinion
/// (undefined, or the player is not on this server), so the server's operator level decides.
pub fn permission(player: u128, node: &str) -> Option<bool> {
    crate::known::record(node);
    runtime::get()?.check(player, node).as_bool()
}

/// The level a granted command runs at: a node set true lifts the player to the level the
/// vanilla command needs (`native_player_command_level`), anything else keeps `level`.
pub fn granted_level(decision: Option<bool>, root: &str, level: u8) -> u8 {
    match decision {
        Some(true) => level.max(vanilla_level(root)),
        _ => level,
    }
}

/// `native_player_command_level`: the level a typed command runs at.
pub fn command_level(player: u128, command: &str, level: u8) -> u8 {
    let Some(rt) = runtime::get() else { return level };
    let root = root_of(command);
    granted_level(rt.command_decision(player, &root), &root, level)
}

/// `native_player_commands_sent_v2`: hidden roots whose node is set true go back into the tree.
pub fn unhide_granted(player: u128, names: &mut Vec<String>, names_hidden: &[String]) {
    let Some(rt) = runtime::get() else { return };
    crate::known::record_commands(&rt.config(), names_hidden);
    move_granted(names, names_hidden, |root| rt.command_decision(player, root));
}

fn move_granted(names: &mut Vec<String>, hidden: &[String], decide: impl Fn(&str) -> Option<bool>) {
    for h in hidden {
        if decide(&root_of(h)) == Some(true) && !names.iter().any(|n| n == h) {
            names.push(h.clone());
        }
    }
}

pub fn login(player: u128, name: &str) {
    if let Some(rt) = runtime::get() {
        rt.on_login(player, name);
    }
}

/// `native_player_pre_login`: the player's data loads before they are placed. `done` gets the answer on the
/// worker thread (Ok lets them on, Err refuses them with the reason). False when no service runs: nothing is
/// queued, and the login goes on as before.
pub fn pre_login(player: u128, name: &str, done: impl FnOnce(Result<(), String>) + Send + 'static) -> bool {
    match runtime::get() {
        Some(rt) => rt.pre_login(player, name, Box::new(done)),
        None => false,
    }
}

pub fn players(players: Vec<OnlinePlayer>) {
    if let Some(rt) = runtime::get() {
        rt.update_players(players);
    }
}

pub fn logout(player: u128) {
    if let Some(rt) = runtime::get() {
        rt.on_logout(player);
    }
}

/// The root word of a command line, without a slash or a namespace.
pub fn root_of(command: &str) -> String {
    let w = command.trim_start_matches('/').split_whitespace().next().unwrap_or("").to_lowercase();
    w.strip_prefix("minecraft:").map(str::to_owned).unwrap_or(w)
}

/// `/lp ...` from a player or the console: true when it is ours (the answer comes later).
pub fn lp_command(sender: Option<(u128, &str)>, command: &str) -> bool {
    let line = command.trim_start_matches('/');
    let mut parts = line.splitn(2, ' ');
    let root = parts.next().unwrap_or("").to_lowercase();
    if !ROOTS.contains(&root.as_str()) {
        return false;
    }
    let rest = parts.next().unwrap_or("");
    let Some(rt) = runtime::get() else {
        return false;
    };
    let s = match sender {
        Some((uuid, name)) => CommandSender { uuid, name: name.to_owned(), console: false },
        None => CommandSender::console(),
    };
    rt.submit_command(s, rest);
    true
}

/// What to do with a typed command.
#[derive(Debug, PartialEq, Eq)]
pub enum Gate {
    /// The game decides (its operator levels).
    Pass,
    /// Refused: the line to answer with.
    Refuse(String),
}

/// The operator level vanilla needs for a command (`Commands` registrations); 2 for the
/// unknown (most mod-free commands are level 2).
pub fn vanilla_level(root: &str) -> u8 {
    match root {
        "help" | "list" | "me" | "msg" | "tell" | "w" | "teammsg" | "tm" | "trigger" | "random" => 0,
        "ban" | "ban-ip" | "banlist" | "deop" | "kick" | "op" | "pardon" | "pardon-ip" | "whitelist" | "setidletimeout" | "transfer" | "debug" | "tick" => 3,
        "stop" | "save-all" | "save-off" | "save-on" | "publish" | "jfr" | "perf" => 4,
        _ => 2,
    }
}

/// The gate for a player's typed command, with their operator level.
pub fn gate(player: u128, name: &str, command: &str, level: u8) -> Gate {
    let Some(rt) = runtime::get() else { return Gate::Pass };
    let root = root_of(command);
    match rt.command_decision(player, &root) {
        Some(false) => Gate::Refuse(rt.config().horizon.no_permission_message.clone()),
        // Granted commands run at a raised level through `native_player_command_level`.
        _ => {
            let _ = (name, level);
            Gate::Pass
        }
    }
}

/// Removes refused commands from a player's command tree.
pub fn filter_tree(player: u128, names: &mut Vec<String>) {
    let Some(rt) = runtime::get() else { return };
    crate::known::record_commands(&rt.config(), names);
    names.retain(|n| rt.command_decision(player, &root_of(n)) != Some(false));
}

/// Completions for `/lp ...`: (start byte of the replaced word, the words).
pub fn complete(player: u128, name: &str, command: &str) -> Option<(usize, Vec<String>)> {
    let line = command.strip_prefix('/').unwrap_or(command);
    let offset = command.len() - line.len();
    let root = line.split(' ').next().unwrap_or("").to_lowercase();
    if !ROOTS.contains(&root.as_str()) || !line.contains(' ') {
        return None;
    }
    let rt = runtime::get()?;
    let rest = &line[root.len() + 1..];
    let mut args: Vec<String> = rest.split(' ').map(str::to_owned).collect();
    if args.is_empty() {
        args.push(String::new());
    }
    let last_start = offset + root.len() + 1 + rest.rfind(' ').map(|i| i + 1).unwrap_or(0);
    let sender = CommandSender { uuid: player, name: name.to_owned(), console: false };
    Some((last_start, rt.complete(&sender, &args)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_and_levels() {
        assert_eq!(root_of("/minecraft:Gamemode creative"), "gamemode");
        assert_eq!(root_of("lp user x"), "lp");
        assert_eq!(vanilla_level("stop"), 4);
        assert_eq!(vanilla_level("gamemode"), 2);
        assert_eq!(vanilla_level("msg"), 0);
    }

    #[test]
    fn granted_commands_run_at_their_level() {
        assert_eq!(granted_level(Some(true), "gamemode", 0), 2);
        assert_eq!(granted_level(Some(true), "stop", 1), 4);
        assert_eq!(granted_level(Some(true), "msg", 0), 0);
        assert_eq!(granted_level(Some(true), "gamemode", 4), 4, "never lowered by a grant");
        assert_eq!(granted_level(Some(false), "gamemode", 0), 0);
        assert_eq!(granted_level(None, "gamemode", 3), 3);
    }

    #[test]
    fn granted_hidden_roots_come_back() {
        let mut names = vec!["help".to_string()];
        let hidden = vec!["gamemode".to_string(), "op".to_string()];
        move_granted(&mut names, &hidden, |r| (r == "gamemode").then_some(true));
        assert_eq!(names, vec!["help", "gamemode"]);
        move_granted(&mut names, &hidden, |r| (r == "gamemode").then_some(true));
        assert_eq!(names.len(), 2, "no duplicates");
    }

    #[test]
    fn version_change_is_seen_once() {
        let seen = std::sync::atomic::AtomicU64::new(0);
        assert!(changed_since(&seen, 3));
        assert!(!changed_since(&seen, 3));
        assert!(changed_since(&seen, 4));
    }

    #[test]
    fn nothing_running_passes() {
        // No runtime started in this test binary's slot here (other tests may start one; the
        // gate then still passes for an unknown player).
        assert_eq!(gate(0xdead, "x", "gamemode creative", 0), Gate::Pass);
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn original_files_are_read_in_place() {
        let dir = std::env::temp_dir().join(format!("horizon-luckperms-glue-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Invented values, the structure of the original.
        std::fs::write(dir.join(CONFIG_YML), "server: lobby\nstorage-method: mariadb\nsync-minutes: 7\n").unwrap();
        std::fs::write(dir.join(CONTEXTS_JSON), r#"{"static-contexts": {"region": "north"}, "default-contexts": {}}"#).unwrap();
        let cfg = load(&Source::Original { dir: dir.clone() }).unwrap();
        assert_eq!(cfg.server, "lobby");
        assert_eq!(cfg.sync_minutes, 7);
        assert_eq!(cfg.static_contexts.get("region").map(String::as_str), Some("north"));
        // Without contexts.json the static contexts are empty.
        std::fs::remove_file(dir.join(CONTEXTS_JSON)).unwrap();
        assert!(load(&Source::Original { dir: dir.clone() }).unwrap().static_contexts.is_empty());
        // A missing config.yml is an error, not a silent default.
        std::fs::remove_file(dir.join(CONFIG_YML)).unwrap();
        assert!(load(&Source::Original { dir: dir.clone() }).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_toml_fallback_reads_its_own_file() {
        let path = std::env::temp_dir().join(format!("horizon-luckperms-toml-{}.toml", std::process::id()));
        std::fs::write(&path, "server = \"Lab\"\n").unwrap();
        let cfg = load(&Source::Toml { path: path.clone() }).unwrap();
        assert_eq!(cfg.server, "lab");
        let _ = std::fs::remove_file(&path);
    }
}

