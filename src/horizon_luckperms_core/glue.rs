//! What the Horizon helpers call: everything they do, minus the engine types, so it can be
//! tested here. The helpers (in `glue/` next to `horizon-mixin.toml`) only convert.

use crate::horizon_luckperms_core::command::ROOTS;
use crate::horizon_luckperms_core::config::{Config, DEFAULT_TOML, FILE_NAME};
use crate::horizon_luckperms_core::original::{self, Reads, CONFIG_YML, CONTEXTS_JSON};
use crate::horizon_luckperms_core::runtime::{self, CommandSender, OnlinePlayer, Outgoing, Recipient};
use crate::horizon_luckperms_core::storage::Storage;
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
            let mut cfg = Config::parse(&text)?;
            // The file storages and the SQLite file live in `<config dir>/luckperms`, with the exports.
            cfg.data_dir = path.parent().map(|p| p.join("luckperms")).unwrap_or_default();
            Ok(cfg)
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
            cfg.data_dir = dir.clone();
            Ok(cfg)
        }
    }
}

/// Starts the service. The original `plugins/LuckPerms/config.yml` is used when it exists (read unchanged,
/// nothing is written there). Otherwise `config/luckperms.toml` is read, and written with the defaults and an
/// empty password when missing; the service then starts only once the password is filled in.
pub fn start(config_dir: &Path, level_name: &str) {
    crate::horizon_luckperms_core::command::backup::set_data_dir(config_dir);
    crate::horizon_luckperms_core::translations::set_dir(config_dir);
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
    crate::horizon_luckperms_core::translations::load(&std::env::var("LUCKPERMS_LOCALE").unwrap_or_else(|_| cfg.horizon.locale.clone()));
    let storage: Box<dyn Storage> = match crate::horizon_luckperms_core::storage::factory::open(&cfg) {
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
    /// `/lp verbose command` jobs: the host runs each command as its target.
    pub verbose_jobs: Vec<runtime::VerboseJob>,
    /// Commands a node grants above the player's operator level: the host runs each at the level it needs.
    pub granted: Vec<runtime::GrantedJob>,
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
    TickOut { lines: rt.drain_outbox(), console_commands: rt.drain_console_commands(), verbose_jobs: rt.drain_verbose_jobs(), granted: rt.drain_granted_jobs(), resend_trees }
}

/// `auto-op` is on: operator status follows `luckperms.autoop`.
pub fn auto_op() -> bool {
    runtime::get().is_some_and(|rt| rt.config().auto_op)
}

/// `native_permission_check`: LuckPerms' answer for a node, None when it has no opinion
/// (undefined, or the player is not on this server), so the server's operator level decides.
pub fn permission(player: u128, node: &str) -> Option<bool> {
    crate::horizon_luckperms_core::known::record(node);
    runtime::get()?.check(player, node).as_bool()
}

/// What the permission handler answers when LuckPerms has no opinion: an online player is "op" (`enable-ops`
/// and the configured level) or not; an offline player has no answer. A handler must never leave a query
/// unanswered (that is a failure), so `None` here becomes an error, never a grant.
pub fn handler_default(decision: Option<bool>, online: bool, is_op: bool) -> Option<bool> {
    match decision {
        Some(b) => Some(b),
        None if online => Some(is_op),
        None => None,
    }
}

/// The permission handler's answer for a Boolean node: LuckPerms' own decision, else the default for an
/// online player of operator level `level` (`None`: not started, or an offline player).
pub fn handler_answer(player: u128, node: &str, online: bool, level: u8) -> Option<bool> {
    let rt = runtime::get()?;
    crate::horizon_luckperms_core::known::record(node);
    let decision = rt.check(player, node).as_bool();
    let answer = handler_default(decision, online, rt.is_op(level));
    crate::horizon_luckperms_core::verbose::record(&rt, player, rt.online_player(player).map(|p| p.name), node, decision);
    answer
}

/// What a mod declared as a node's default (Paper's plugin.yml `default:`), as the handler sees it with the
/// engine feature `horizon_handler_default` (horizon-rust/horizon #18755).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclaredDefault {
    /// Granted to every player.
    Everyone,
    /// Granted to nobody, operators included.
    Nobody,
    /// Granted to operators of at least this level.
    Level(u8),
}

/// The handler's answer when the node's declared default is known: LuckPerms' decision, else the declared default
/// (`apply-bukkit-default-permissions` off: operators only, as without a declaration). `None` for an offline player
/// without a decision.
pub fn handler_answer_declared(player: u128, node: &str, online: bool, level: u8, declared: DeclaredDefault) -> Option<bool> {
    let rt = runtime::get()?;
    crate::horizon_luckperms_core::known::record(node);
    let mut decision = rt.check(player, node).as_bool();
    let is_op = rt.is_op(level);
    // `apply-default-negated-permissions-before-wildcards`: a node whose declared default is false for this player
    // is not lifted by a wildcard or regex.
    if rt.config().apply_default_negated_before_wildcards
        && decision == Some(true)
        && matches!(rt.check_processor(player, node), "wildcard" | "regex")
        && match declared {
            DeclaredDefault::Nobody => true,
            DeclaredDefault::Level(l) => l > 0 && !(is_op && level >= l),
            DeclaredDefault::Everyone => false,
        }
    {
        decision = Some(false);
    }
    let answer = declared_answer(decision, online, is_op, level, declared, rt.config().apply_bukkit_default_permissions);
    crate::horizon_luckperms_core::verbose::record(&rt, player, rt.online_player(player).map(|p| p.name), node, decision);
    answer
}

/// The pure part of [`handler_answer_declared`].
pub fn declared_answer(decision: Option<bool>, online: bool, is_op: bool, level: u8, declared: DeclaredDefault, apply_defaults: bool) -> Option<bool> {
    if decision.is_some() {
        return decision;
    }
    if !online {
        return None;
    }
    if !apply_defaults {
        return Some(is_op);
    }
    Some(match declared {
        DeclaredDefault::Everyone => true,
        DeclaredDefault::Nobody => false,
        DeclaredDefault::Level(0) => true,
        DeclaredDefault::Level(l) => is_op && level >= l,
    })
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
    crate::horizon_luckperms_core::known::record_commands(&rt.config(), names_hidden);
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

/// Milliseconds a login waits for the player's data (`horizon.login-wait-ms`); 0 when no service runs.
pub fn login_wait_ms() -> u64 {
    runtime::get().map_or(0, |rt| rt.config().horizon.login_wait_ms)
}

/// A `discordsrv:contexts` reply: one `key=value` per line; lines without both parts are skipped.
pub fn parse_contexts(reply: &str) -> Vec<(String, String)> {
    reply
        .lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, v)| !k.trim().is_empty() && !v.trim().is_empty())
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .collect()
}

/// The contexts another mod gave for a player.
pub fn external_contexts(uuid: u128, pairs: Vec<(String, String)>) {
    if let Some(rt) = runtime::get() {
        rt.set_external_contexts(uuid, pairs);
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
    crate::horizon_luckperms_core::known::record_commands(&rt.config(), names);
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
    fn handler_default_never_leaves_a_query_open() {
        assert_eq!(handler_default(Some(false), true, true), Some(false), "an explicit deny beats op");
        assert_eq!(handler_default(Some(true), false, false), Some(true));
        assert_eq!(handler_default(None, true, true), Some(true));
        assert_eq!(handler_default(None, true, false), Some(false));
        assert_eq!(handler_default(None, false, true), None, "offline: no answer, the handler errors");
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

#[cfg(test)]
mod declared_default_tests {
    use super::*;

    #[test]
    fn a_decision_wins_over_the_declared_default() {
        assert_eq!(declared_answer(Some(false), true, true, 4, DeclaredDefault::Everyone, true), Some(false));
        assert_eq!(declared_answer(Some(true), true, false, 0, DeclaredDefault::Nobody, true), Some(true));
    }

    #[test]
    fn contexts_reply_parses_key_value_lines() {
        let got = parse_contexts("discordsrv:linked=true\nbad line\ndiscordsrv:role=Staff Team\n=x\nk=\n");
        assert_eq!(got, vec![("discordsrv:linked".to_owned(), "true".to_owned()), ("discordsrv:role".to_owned(), "Staff Team".to_owned())]);
        assert!(parse_contexts("").is_empty());
    }

    #[test]
    fn without_a_decision_the_declared_default_decides() {
        assert_eq!(declared_answer(None, true, false, 0, DeclaredDefault::Everyone, true), Some(true));
        assert_eq!(declared_answer(None, true, true, 4, DeclaredDefault::Nobody, true), Some(false));
        assert_eq!(declared_answer(None, true, false, 0, DeclaredDefault::Level(0), true), Some(true));
        assert_eq!(declared_answer(None, true, false, 0, DeclaredDefault::Level(2), true), Some(false));
        assert_eq!(declared_answer(None, true, true, 1, DeclaredDefault::Level(2), true), Some(false));
        assert_eq!(declared_answer(None, true, true, 2, DeclaredDefault::Level(2), true), Some(true));
    }

    #[test]
    fn an_offline_player_without_a_decision_has_no_answer() {
        assert_eq!(declared_answer(None, false, false, 0, DeclaredDefault::Everyone, true), None);
    }

    #[test]
    fn with_bukkit_defaults_off_only_operators_pass() {
        assert_eq!(declared_answer(None, true, false, 0, DeclaredDefault::Everyone, false), Some(false));
        assert_eq!(declared_answer(None, true, true, 4, DeclaredDefault::Nobody, false), Some(true));
    }
}

/// Called by the host after it ran a `/lp verbose command` job: stops the sender's listener and says how it went
/// (`VerboseCommand`: complete, or no checks made).
pub fn verbose_job_done(job: &runtime::VerboseJob) {
    let Some(rt) = runtime::get() else { return };
    let matched = crate::horizon_luckperms_core::verbose::unregister(job.sender.uuid).map_or(0, |f| f.matched);
    let to = if job.sender.console { Recipient::Console } else { Recipient::Player(job.sender.uuid) };
    let lines = crate::horizon_luckperms_core::command::verbose_done_lines(matched);
    for l in lines {
        rt.push_out(to.clone(), l);
    }
}

/// Registers the accept-all listener for a job, before its command runs.
pub fn verbose_job_start(job: &runtime::VerboseJob) {
    if let Ok(ast) = crate::horizon_luckperms_core::verbose::compile("") {
        let to = if job.sender.console { Recipient::Console } else { Recipient::Player(job.sender.uuid) };
        crate::horizon_luckperms_core::verbose::register(to, job.sender.uuid, &job.sender.name, job.sender.console, ast, "", true);
    }
}

/// A typed command whose node is true but whose vanilla level the player lacks: queued to run at the level it needs
/// (the player's own feedback, no operator status). True when queued (the original line must then be cancelled).
pub fn queue_granted(player: u128, command: &str, level: u8) -> bool {
    let Some(rt) = runtime::get() else { return false };
    let needed = command_level(player, command, level);
    if needed <= level {
        return false;
    }
    rt.queue_granted_job(runtime::GrantedJob { player, line: command.trim_start_matches('/').to_owned(), level: needed });
    true
}

/// The player's primary group, for the engine's `permission_groups`.
pub fn primary_group(player: u128) -> Option<String> {
    crate::horizon_luckperms_core::api::primary_group(player)
}

/// Every group the player inherits here, primary first when known.
pub fn groups(player: u128) -> Vec<String> {
    let mut all = crate::horizon_luckperms_core::api::groups(player);
    if let Some(p) = primary_group(player) {
        all.retain(|g| *g != p);
        all.insert(0, p);
    }
    all
}

/// The prefix and suffix of the player and of each of their groups, for `permission_groups`.
pub fn meta_of(player: u128) -> (Option<String>, Option<String>, Vec<(String, Option<String>, Option<String>)>) {
    (
        crate::horizon_luckperms_core::api::prefix(player).filter(|p| !p.is_empty()),
        crate::horizon_luckperms_core::api::suffix(player).filter(|p| !p.is_empty()),
        crate::horizon_luckperms_core::api::group_meta(player),
    )
}

/// A group write another mod asked for (`permission_group_write`): the player joins or leaves the group, as
/// `/lp user <uuid> parent add|remove` does (Vault's `userAddGroup`/`userRemoveGroup`). False for a group that does not
/// exist or while no service runs. The published groups follow on the next refresh of the tick.
pub fn group_write(player: u128, group: &str, add: bool) -> bool {
    let Some(rt) = runtime::get() else { return false };
    let v = crate::horizon_luckperms_core::vault::Vault::new(&rt);
    if add { v.user_add_group(None, player, group) } else { v.user_remove_group(None, player, group) }
}

/// `resolve-command-selectors` is on.
pub fn resolve_selectors() -> bool {
    runtime::get().is_some_and(|rt| rt.config().resolve_command_selectors)
}

/// The placeholder service (`luckperms:placeholder`): payload `<uuid as 32 hex digits> <params>`, reply the value.
/// Params: `prefix`, `suffix`, `primary_group_name`, `weight`, `groups`, `meta_<key>`, `in_group_<name>`,
/// `has_permission_<node>`, `check_permission_<node>` (the `%luckperms_*%` names).
pub fn placeholder(payload: &str) -> Option<String> {
    use crate::horizon_luckperms_core::api;
    let (id, params) = payload.split_once(' ')?;
    let uuid = u128::from_str_radix(id.trim(), 16).ok()?;
    let p = params.trim();
    Some(match p {
        "prefix" => api::prefix(uuid).unwrap_or_default(),
        "suffix" => api::suffix(uuid).unwrap_or_default(),
        "primary_group_name" => api::primary_group(uuid).unwrap_or_default(),
        "weight" => api::weight(uuid).map(|w| w.to_string()).unwrap_or_default(),
        "groups" => groups(uuid).join(", "),
        _ => {
            if let Some(k) = p.strip_prefix("meta_") {
                api::meta_value(uuid, k).unwrap_or_default()
            } else if let Some(g) = p.strip_prefix("in_group_") {
                api::in_group(uuid, g).to_string()
            } else if let Some(n) = p.strip_prefix("has_permission_") {
                (permission(uuid, n) == Some(true)).to_string()
            } else if let Some(n) = p.strip_prefix("check_permission_") {
                permission(uuid, n).map(|b| b.to_string()).unwrap_or_default()
            } else {
                return None;
            }
        }
    })
}
