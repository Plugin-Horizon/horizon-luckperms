//! LuckPerms as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-luckperms-core` crate (copied to `core/`); this file maps the glue points of the
//! horizon-rust port (`glue/*.rs`, `horizon-mixin.toml`) onto arismoko's hooks and host methods.
//! Everything arismoko lacks is a `TODO(arismoko-api: <name>)`; see PORT_NOTES.md.
//! Permission provider: this mod gathers the handler `HANDLER_ID`; the server answers node queries
//! from LuckPerms once `permission-handler=luckperms:permissions` is set in server.properties.

pub mod horizon_luckperms_core;
use crate::horizon_luckperms_core::command::ROOTS;
use crate::horizon_luckperms_core::glue::{self, Gate};
use crate::horizon_luckperms_core::runtime::OnlinePlayer;
use crate::horizon_luckperms_core::text as lp_text;
use crate::horizon_luckperms_core::Recipient;
use mod_api::{
    Command, CommandArgumentValue, CommandSource, Flow, Hooks, PermissionError,
    ModServiceCallEvent, PermissionGatherHandlerEvent, PermissionHandlerQueryEvent, PermissionLevel, PermissionValue, PermissionValueType,
    PlayerCommandEvent, PlayerCommandsSentEvent, PlayerCommandsSentRestoreEvent, PlayerLoggedInEvent, PlayerLoggedOutEvent, PlayerLoginEvent,
    RegisterCommandsEvent, RunModCommandEvent, ServerAboutToStartEvent,
    ServerHost, ServerStoppingEvent, ServerTickEvent, ServerTickPhase, Suggestion, TabCompleteEvent,
    Text,
};
use mod_api::Identifier;
use mod_api::text::{TextClick, TextHover, TextStyle};
use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a player may play with `default` rights while their data loads; after that they are kicked.
/// Checked in `server_tick`: the server thread never waits for the load.
const LOAD_TIMEOUT: Duration = Duration::from_secs(5);
/// A pending entry whose player never got placed is dropped after this long.
const PENDING_FORGET: Duration = Duration::from_secs(60);

/// Logins whose data load is still running or whose answer was not acted on yet.
struct Pending {
    name: String,
    since: Instant,
    answer: Option<Result<(), String>>,
}

/// Set while a granted command runs, so its own `player_command` is not granted again.
static GRANTED_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

static PENDING: Mutex<Option<HashMap<u128, Pending>>> = Mutex::new(None);

fn pending<R>(f: impl FnOnce(&mut HashMap<u128, Pending>) -> R) -> R {
    let mut guard = PENDING.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// One server tick's look at the pending loads: done ones are dropped, failed or late ones kicked.
/// Never blocks: the worker writes answers into the map, the tick only reads it.
fn settle_logins(host: &mut dyn ServerHost) {
    let now = Instant::now();
    let mut kick = Vec::new();
    pending(|map| {
        map.retain(|uuid, p| {
            let online = host.player(*uuid).is_some();
            match &p.answer {
                Some(Ok(())) => false,
                Some(Err(e)) if online => {
                    kick.push((*uuid, p.name.clone(), e.clone()));
                    false
                }
                None if online && now - p.since > LOAD_TIMEOUT => {
                    kick.push((*uuid, p.name.clone(), format!("no answer within {}s", LOAD_TIMEOUT.as_secs())));
                    false
                }
                _ => now - p.since < PENDING_FORGET,
            }
        });
    });
    for (uuid, name, failure) in kick {
        // The reason shown to the player stays generic: the error text can name the database.
        eprintln!("[LuckPerms] data of {name} not loaded, player kicked: {failure}");
        if let Some(p) = host.player(uuid) {
            let _ = p.disconnect(host, Text::literal("LuckPerms could not load your data, please try again."));
        }
    }
}

/// The id of the permission handler this mod provides (`permission-handler` in server.properties).
const HANDLER_ID: &str = "luckperms:permissions";

#[derive(Default)]
struct Mod;

/// The level name Paper's world contexts are built from (`server.properties` `level-name`).
/// The file is read for that one key only; nothing is printed.
fn level_name() -> String {
    std::fs::read_to_string("server.properties")
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("level-name=").map(|v| v.trim().to_owned()))
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "world".into())
}

/// The players the core's cache needs: the ones on this server, bots without a client left out.
// VERIFY: handle methods `is_fake`, `dimension`, `game_mode` (host players area: player_is_fake,
// player_dimension, player_game_mode); `name` and `permission_level` are used the same way by the sibling ports.
fn online(host: &dyn ServerHost) -> Vec<OnlinePlayer> {
    host.players()
        .server_players()
        .into_iter()
        .filter(|p| p.is_fake(host) != Some(true))
        .map(|p| OnlinePlayer {
            uuid: p.uuid(),
            name: p.name(host).unwrap_or_default(),
            op_level: p.permission_level(host).unwrap_or(0),
            dimension: p.dimension(host).map(|d| d.to_string()).unwrap_or_default(),
            // VERIFY: GameMode::id (0 survival, 1 creative, 2 adventure, 3 spectator).
            game_mode: p.game_mode(host).map(|g| g.id()).unwrap_or(0),
        })
        .collect()
}

/// `resolve-command-selectors`: an `@...` word of a typed `/lp` line becomes the name of the one player it selects.
fn resolve_selectors(host: &mut dyn ServerHost, player: u128, line: &str) -> String {
    if !glue::resolve_selectors() || !line.contains('@') || !ROOTS_PREFIX_CHECK.iter().any(|r| line.trim_start_matches('/').split(' ').next() == Some(*r)) {
        return line.to_owned();
    }
    let Some(sender) = host.player(player) else { return line.to_owned() };
    let mut out = Vec::new();
    for word in line.split(' ') {
        if word.starts_with('@') {
            if let Some(found) = sender.select_entities(host, word) {
                let names: Vec<String> = found
                    .into_iter()
                    .filter_map(|e| host.players().server_players().into_iter().find(|p| p.entity() == e))
                    .filter_map(|p| p.name(&*host))
                    .collect();
                if names.len() == 1 {
                    out.push(names[0].clone());
                    continue;
                }
            }
        }
        out.push(word.to_owned());
    }
    out.join(" ")
}

const ROOTS_PREFIX_CHECK: [&str; 6] = ["lp", "luckperms", "perm", "perms", "permission", "permissions"];

fn color_name(code: char) -> &'static str {
    match code {
        '0' => "black",
        '1' => "dark_blue",
        '2' => "dark_green",
        '3' => "dark_aqua",
        '4' => "dark_red",
        '5' => "dark_purple",
        '6' => "gold",
        '7' => "gray",
        '8' => "dark_gray",
        '9' => "blue",
        'a' => "green",
        'b' => "aqua",
        'c' => "red",
        'd' => "light_purple",
        'e' => "yellow",
        _ => "white",
    }
}

/// A line with `§` codes as text: colours and formats become styles, the rest is literal.
fn legacy(line: &str) -> Text {
    let mut out = Text::literal("");
    let mut style = TextStyle::default();
    let mut buf = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c != '§' {
            buf.push(c);
            continue;
        }
        let Some(code) = chars.next() else { break };
        if !buf.is_empty() {
            out.children.push(Text::literal(std::mem::take(&mut buf)).with_style(style.clone()));
        }
        match code.to_ascii_lowercase() {
            c @ ('0'..='9' | 'a'..='f') => {
                // A colour code resets the formats, as in the game.
                style = TextStyle::default();
                style.color = Some(color_name(c).to_owned());
            }
            'l' => style.bold = Some(true),
            'o' => style.italic = Some(true),
            'n' => style.underlined = Some(true),
            'm' => style.strikethrough = Some(true),
            'k' => style.obfuscated = Some(true),
            'r' => style = TextStyle::default(),
            _ => {}
        }
    }
    if !buf.is_empty() {
        out.children.push(Text::literal(buf).with_style(style));
    }
    out
}

/// A line of the core's output, with its rich segments (click, hover) as arismoko text.
fn line_text(line: &str) -> Text {
    let mut root = Text::literal("");
    for seg in lp_text::segments(line) {
        let part = legacy(&seg.text);
        if seg.click.is_none() && seg.hover.is_none() {
            root.children.push(part);
            continue;
        }
        let mut style = TextStyle::default();
        style.click = seg.click.map(|c| match c {
            lp_text::Click::OpenUrl(v) => TextClick::OpenUrl(v),
            lp_text::Click::SuggestCommand(v) => TextClick::SuggestCommand(v),
            lp_text::Click::RunCommand(v) => TextClick::RunCommand(v),
        });
        style.hover = seg.hover.map(|h| TextHover::Text(Box::new(legacy(&h))));
        root.children.push(part.with_style(style));
    }
    root
}

/// The one place the plugin-message send is called (the engine's `send_plugin_message` on the player handle).
fn send_plugin_frame(host: &mut dyn ServerHost, player: &mod_api::PlayerRef, id: &Identifier, frame: &[u8]) {
    let _ = player.send_plugin_message(&mut *host, id, frame);
}

#[mod_api::horizon_mod]
impl Hooks for Mod {
    #[allow(unused_variables)]
    fn server_about_to_start(&self, host: &mut dyn ServerHost, _: &mut ServerAboutToStartEvent) {
        // The plugin channel the `messaging-service: pluginmsg` messenger runs on.
        let _ = host.register_plugin_channel(&Identifier::parse(crate::horizon_luckperms_core::messenger_wave2::PLUGIN_CHANNEL).expect("a valid channel id"));
        // The core reads plugins/LuckPerms/config.yml when it exists (unchanged, never written), else
        // config/luckperms.toml (written with the defaults on first start, the password empty).
        glue::start(Path::new("config"), &level_name());
    }

    fn server_stopping(&self, _: &mut dyn ServerHost, _: &mut ServerStoppingEvent) {
        glue::stop();
    }

    fn plugin_message(&self, _: &mut dyn ServerHost, event: &mut mod_api::PluginMessageEvent<'_>) {
        if event.channel.to_string() == crate::horizon_luckperms_core::messenger_wave2::PLUGIN_CHANNEL {
            crate::horizon_luckperms_core::messenger_wave2::plugin_message_received(event.data);
        }
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        // The horizon-rust port ran this at both phases; here once per tick, at the start.
        if !matches!(event.phase, ServerTickPhase::ServerTickStart { .. }) {
            return;
        }
        // Frames of the pluginmsg messenger leave through a player; with nobody online they wait.
        {
            let frames = crate::horizon_luckperms_core::messenger_wave2::take_outgoing();
            if !frames.is_empty() {
                let carrier = online(&*host).first().map(|p| p.uuid);
                match carrier.and_then(|u| host.player(u)) {
                    Some(player) => {
                        let id = Identifier::parse(crate::horizon_luckperms_core::messenger_wave2::PLUGIN_CHANNEL).expect("a valid channel id");
                        for f in &frames {
                            let _ = send_plugin_frame(host, &player, &id, f);
                        }
                    }
                    None => crate::horizon_luckperms_core::messenger_wave2::requeue_outgoing(frames),
                }
            }
        }
        static CALLS: AtomicU32 = AtomicU32::new(0);
        let n = CALLS.fetch_add(1, Ordering::Relaxed);
        // Every 10th tick the players go to the cache; every 40th the config files are looked at.
        let players = (n % 10 == 0).then(|| online(&*host));
        // The groups go to the engine for the mods that read them (TAB, Vault-style callers): `permission_groups`.
        if let Some(list) = &players {
            for p in list {
                let (prefix, suffix, group_meta) = glue::meta_of(p.uuid);
                let groups = mod_api::PermissionGroups {
                    primary: glue::primary_group(p.uuid),
                    groups: glue::groups(p.uuid),
                    prefix,
                    suffix,
                    group_meta: group_meta
                        .into_iter()
                        .map(|(group, prefix, suffix)| mod_api::GroupMeta { group, prefix, suffix })
                        .collect(),
                };
                let _ = host.permission_set_groups(p.uuid, &groups);
            }
        }
        // DiscordSRV's context calculator: its contexts for each player, when that mod runs (absent: nothing).
        if let Some(list) = &players {
            for p in list {
                let pairs = host
                    .commands()
                    .call_mod_service("discordsrv:contexts", format!("{:032x}", p.uuid).as_bytes())
                    .ok()
                    .and_then(|r| String::from_utf8(r).ok())
                    .map(|r| glue::parse_contexts(&r))
                    .unwrap_or_default();
                glue::external_contexts(p.uuid, pairs);
            }
        }
        // `auto-op`: operator status follows the node `luckperms.autoop` (Bukkit's `setOp`).
        if let Some(list) = &players {
            if glue::auto_op() {
                for p in list {
                    let want = glue::permission(p.uuid, "luckperms.autoop") == Some(true);
                    if want != (p.op_level > 0) {
                        if let Some(handle) = host.player(p.uuid) {
                            let level = if want { PermissionLevel::Owners } else { PermissionLevel::All };
                            let _ = host.player_set_op_level(handle, level);
                        }
                    }
                }
            }
        }
        if n % 40 == 0 {
            glue::check_config();
        }
        settle_logins(host);
        let out = glue::tick(players);
        for o in out.lines {
            match o.to {
                Recipient::Player(uuid) => {
                    if let Some(p) = host.player(uuid) {
                        let _ = p.tell(host, line_text(&o.line));
                    }
                }
                Recipient::Console => {
                    for l in lp_text::plain(&o.line).lines() {
                        eprintln!("[LuckPerms] {l}");
                    }
                }
            }
        }
        // A group, user or track changed: every mod registers again and every player's tree is sent
        // again (arismoko's refresh is server-wide, not per player as native_resend_commands was).
        if out.resend_trees {
            host.commands().refresh_commands();
        }
        // `/lp verbose command`: each target runs its command as itself while the sender's listener records.
        for job in out.verbose_jobs {
            glue::verbose_job_start(&job);
            if let Some(p) = host.player(job.target) {
                let level = p.permission_level(&*host).unwrap_or(0);
                let _ = p.entity().run_command_with_output(host, &job.command, PermissionLevel::from_level(i32::from(level)));
            }
            glue::verbose_job_done(&job);
        }
        // Granted commands: run as the player at the level the command needs.
        for job in out.granted {
            if let Some(p) = host.player(job.player) {
                GRANTED_RUNNING.store(true, Ordering::Relaxed);
                let _ = p.entity().run_command(host, &job.line, PermissionLevel::from_level(i32::from(job.level)));
                GRANTED_RUNNING.store(false, Ordering::Relaxed);
            }
        }
        // Queued console commands (not used by the core today, kept as in the port).
        for line in out.console_commands {
            if !host.commands().run_command(&line) {
                eprintln!("[LuckPerms] could not run: {line}");
            }
        }
    }

    fn player_login(&self, _: &mut dyn ServerHost, event: &mut PlayerLoginEvent<'_>) -> Flow {
        // Like Paper's AsyncPlayerPreLoginEvent, a failed load refuses the connection: wait (bounded) for the
        // worker's answer. `horizon.login-wait-ms = 0` keeps the older grace window below.
        let wait = glue::login_wait_ms();
        if wait > 0 {
            let (tx, rx) = std::sync::mpsc::channel();
            let queued = glue::pre_login(event.player, event.name, move |answer| {
                let _ = tx.send(answer);
            });
            if !queued {
                return Flow::Continue;
            }
            let refusal = match rx.recv_timeout(Duration::from_millis(wait)) {
                Ok(Ok(())) => return Flow::Continue,
                Ok(Err(reason)) => reason,
                Err(_) => format!("no answer within {wait} ms"),
            };
            eprintln!("[LuckPerms] login of {} refused, data not loaded: {refusal}", event.name);
            *event.reason = Some(Text::literal(crate::horizon_luckperms_core::runtime::PRE_LOGIN_REFUSAL));
            return Flow::Cancel;
        }
        // Paper loads the data in AsyncPlayerPreLoginEvent and refuses on failure. `player_login` runs on
        // the server thread, so it must not wait: the load starts on the worker, the player is let on (and
        // resolves as `default` only until the data is there), and `server_tick` kicks them when the load
        // fails or takes longer than LOAD_TIMEOUT.
        let uuid = event.player;
        pending(|map| {
            map.insert(uuid, Pending { name: event.name.to_owned(), since: Instant::now(), answer: None });
        });
        let queued = glue::pre_login(uuid, event.name, move |answer| {
            pending(|map| {
                if let Some(p) = map.get_mut(&uuid) {
                    p.answer = Some(answer);
                }
            });
        });
        if !queued {
            pending(|map| map.remove(&uuid));
        }
        Flow::Continue
    }

    fn player_logged_in(&self, host: &mut dyn ServerHost, event: &mut PlayerLoggedInEvent<'_>) {
        // Bots without a client are not loaded (no database work for them), as in the port.
        if host.player(event.player).and_then(|p| p.is_fake(&*host)) == Some(true) {
            return;
        }
        // The port loads the data in native_player_login (before placement); here the player is placed.
        glue::login(event.player, event.name);
        glue::players(online(&*host));
    }

    fn player_logged_out(&self, _: &mut dyn ServerHost, event: &mut PlayerLoggedOutEvent<'_>) {
        pending(|map| map.remove(&event.player));
        glue::logout(event.player);
    }

    fn register_commands(&self, _: &mut dyn ServerHost, event: &mut RegisterCommandsEvent<'_>) {
        // Only one usage per root, with a free text argument: a console line then reaches run_mod_command
        // whole. The port's per-word usages (for the client's completion) are not registered: arismoko would
        // parse them into typed literals and arguments that cannot be turned back into the typed line.
        // The typed line of a player's /lp is taken whole in `player_command` and its completion is answered by
        // `tab_complete`; the console reaches `run_mod_command` through the free text argument.
        for root in ROOTS {
            event.commands.push(Command::new(&format!("{root} [args:text]")));
        }
    }

    fn run_mod_command(&self, host: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
        // A player's typed /lp is taken whole in player_command. Here: the console and every other source
        // (command blocks, functions, `execute as`); a player source is that player.
        let sender = match event.source {
            CommandSource::Player { player, .. } => {
                Some((player.uuid(), player.name(host).unwrap_or_default()))
            }
            _ => None,
        };
        let Some(root) = event.command.literals.first() else {
            return Flow::Continue;
        };
        let rest = match event.command.argument("args") {
            Some(CommandArgumentValue::String(s)) => s.as_str(),
            _ => "",
        };
        let line = if rest.is_empty() { root.clone() } else { format!("{root} {rest}") };
        let sent = match &sender {
            Some((uuid, name)) => glue::lp_command(Some((*uuid, name.as_str())), &line),
            None => glue::lp_command(None, &line),
        };
        if sent { Flow::Cancel } else { Flow::Continue }
    }

    fn player_command(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandEvent<'_>) -> Flow {
        // The fresh contexts for the decision (command_gate in the port): taken only when the sender is in the
        // list, since the list may leave them out while the server is busy with this command.
        let players = online(&*host);
        if players.iter().any(|p| p.uuid == event.player) {
            glue::players(players.clone());
        }
        // A typed command whose node minecraft.command.<name> is false is refused, with the answer as a reply.
        // The level argument is not used by the core's gate.
        if let Gate::Refuse(line) = glue::gate(event.player, event.name, event.command, 0) {
            event.replies.push(line_text(&line));
            return Flow::Cancel;
        }
        // A command a node grants above the player's level (`minecraft.command.<name>` true) is cancelled here and
        // run again from the next tick at the level it needs, as the player (their own feedback).
        let level = players.iter().find(|p| p.uuid == event.player).map_or(0, |p| p.op_level);
        if !GRANTED_RUNNING.load(Ordering::Relaxed) && glue::queue_granted(event.player, event.command, level) {
            return Flow::Cancel;
        }
        // /lp: the core answers later, through the tick.
        let resolved = resolve_selectors(host, event.player, event.command);
        if glue::lp_command(Some((event.player, event.name)), &resolved) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn player_commands_sent(&self, _: &mut dyn ServerHost, event: &mut PlayerCommandsSentEvent<'_>) {
        // Refused roots leave the player's tree.
        glue::filter_tree(event.player, event.names);
    }

    fn player_commands_sent_restore(&self, _: &mut dyn ServerHost, event: &mut PlayerCommandsSentRestoreEvent<'_>) {
        // A root the player's level hides but a node grants (`minecraft.command.<name>` true) goes back into
        // the tree the client is shown, so it completes and highlights; the run is answered in `player_command`.
        glue::unhide_granted(event.player, event.restore, event.hidden);
    }

    fn mod_service_call(&self, _: &mut dyn ServerHost, event: &mut ModServiceCallEvent<'_>) -> Option<mod_api::PayloadBytes> {
        let text = std::str::from_utf8(&event.payload.0).ok()?;
        let reply = match event.service {
            "luckperms:placeholder" => glue::placeholder(text),
            // The Vault provider (`LuckPermsVaultPermission` and `LuckPermsVaultChat`): see core/vault.rs.
            s @ (crate::horizon_luckperms_core::vault::PERMISSION_SERVICE | crate::horizon_luckperms_core::vault::CHAT_SERVICE) => {
                crate::horizon_luckperms_core::vault::service(s, text)
            }
            _ => None,
        };
        reply.map(|v| mod_api::PayloadBytes(v.into_bytes()))
    }

    fn permission_gather_handler(&self, _: &mut dyn ServerHost, event: &mut PermissionGatherHandlerEvent<'_>) {
        event.handlers.push(HANDLER_ID.to_owned());
    }

    fn permission_handler_query(
        &self,
        host: &mut dyn ServerHost,
        event: &mut PermissionHandlerQueryEvent<'_>,
    ) -> Option<Result<PermissionValue, PermissionError>> {
        if event.handler != HANDLER_ID {
            return None;
        }
        if event.node.value_type != PermissionValueType::Boolean {
            return Some(Err(PermissionError::WrongType));
        }
        // Answered from the core's cache, never from the database: the server thread does not wait.
        let level = event
            .online
            .and_then(|u| host.player(u))
            .and_then(|p| p.permission_level(&*host))
            .unwrap_or(0);
        // A node nobody set falls back to what the mod that owns it declared, as Paper falls back to plugin.yml.
        let declared = match event.default {
            mod_api::NodeDefault::Level(l) => glue::DeclaredDefault::Level(l.level()),
            mod_api::NodeDefault::Value(mod_api::PermissionValue::Boolean(true)) => glue::DeclaredDefault::Everyone,
            mod_api::NodeDefault::Nobody | mod_api::NodeDefault::Value(_) => glue::DeclaredDefault::Nobody,
        };
        let answer =
            glue::handler_answer_declared(event.uuid, &event.node.name, event.online.is_some(), level, declared);
        Some(match answer {
            Some(b) => Ok(PermissionValue::Boolean(b)),
            None if event.online.is_some() => Err(PermissionError::NotInitialized),
            None => Err(PermissionError::UnknownPlayer),
        })
    }

    fn tab_complete(&self, _: &mut dyn ServerHost, event: &mut TabCompleteEvent<'_>) {
        let Some((start, words)) = glue::complete(event.player, event.name, event.command) else {
            return;
        };
        let end = event.command.len();
        for word in words {
            event.suggestions.push(Suggestion::new((start, end), word));
        }
    }
}


