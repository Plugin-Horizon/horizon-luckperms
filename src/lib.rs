//! LuckPerms as an arismoko native trait mod (`mod_api`). The logic is the unchanged
//! `horizon-luckperms-core` crate (copied to `core/`); this file maps the glue points of the
//! horizon-rust port (`glue/*.rs`, `horizon-mixin.toml`) onto arismoko's hooks and host methods.
//! Everything arismoko lacks is a `TODO(arismoko-api: <name>)`; see PORT_NOTES.md.
//! Permission nodes: this mod is the provider (`player_permission_check`), so every mod's
//! `player.has_permission` asks LuckPerms first.
use horizon_luckperms_core::command::ROOTS;
use horizon_luckperms_core::glue::{self, Gate};
use horizon_luckperms_core::runtime::OnlinePlayer;
use horizon_luckperms_core::text as lp_text;
use horizon_luckperms_core::Recipient;
use mod_api::{
    Command, CommandArgumentValue, CommandSource, Flow, Host, Hooks, PlayerCommandEvent,
    PlayerCommandsSentEvent, PlayerLoggedInEvent, PlayerLoggedOutEvent, PlayerLoginEvent,
    PlayerPermissionCheckEvent, RegisterCommandsEvent, RunModCommandEvent, ServerAboutToStartEvent,
    ServerHost, ServerStoppingEvent, ServerTickEvent, ServerTickPhase, Suggestion, TabCompleteEvent,
    Text,
};
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
            let _ = p.kick(host, Text::literal("LuckPerms could not load your data, please try again."));
        }
    }
}

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

#[mod_api::horizon_mod]
impl Hooks for Mod {
    fn server_about_to_start(&self, _: &mut dyn ServerHost, _: &mut ServerAboutToStartEvent) {
        // The core reads plugins/LuckPerms/config.yml when it exists (unchanged, never written), else
        // config/luckperms.toml (written with the defaults on first start, the password empty).
        glue::start(Path::new("config"), &level_name());
    }

    fn server_stopping(&self, _: &mut dyn ServerHost, _: &mut ServerStoppingEvent) {
        glue::stop();
    }

    fn server_tick(&self, host: &mut dyn ServerHost, event: &mut ServerTickEvent<'_>) {
        // The horizon-rust port ran this at both phases; here once per tick, at the start.
        if !matches!(event.phase, ServerTickPhase::ServerTickStart { .. }) {
            return;
        }
        static CALLS: AtomicU32 = AtomicU32::new(0);
        let n = CALLS.fetch_add(1, Ordering::Relaxed);
        // Every 10th tick the players go to the cache; every 40th the config files are looked at.
        let players = (n % 10 == 0).then(|| online(&*host));
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
        // Queued console commands (not used by the core today, kept as in the port).
        for line in out.console_commands {
            if !host.commands().run_command(&line) {
                eprintln!("[LuckPerms] could not run: {line}");
            }
        }
    }

    fn player_login(&self, _: &mut dyn ServerHost, event: &mut PlayerLoginEvent<'_>) -> Flow {
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
        // TODO(arismoko-api: command_raw_line): the typed line of a command, as the console or a player
        // sent it, so per-word usages could be parsed without losing it. The console_line hook does not
        // give it: it hears what the server logs, not the commands typed.
        for root in ROOTS {
            event.commands.push(Command::new(&format!("{root} [args:text]")));
        }
    }

    fn run_mod_command(&self, _: &mut dyn ServerHost, event: &mut RunModCommandEvent<'_>) -> Flow {
        // A player's /lp is taken whole in player_command. Here: the console. Command blocks and functions
        // are not handled (the port did not handle them either).
        if !matches!(event.source, CommandSource::Console { .. }) {
            return Flow::Continue;
        }
        let Some(root) = event.command.literals.first() else {
            return Flow::Continue;
        };
        let rest = match event.command.argument("args") {
            Some(CommandArgumentValue::String(s)) => s.as_str(),
            _ => "",
        };
        let line = if rest.is_empty() { root.clone() } else { format!("{root} {rest}") };
        if glue::lp_command(None, &line) { Flow::Cancel } else { Flow::Continue }
    }

    fn player_command(&self, host: &mut dyn ServerHost, event: &mut PlayerCommandEvent<'_>) -> Flow {
        // The fresh contexts for the decision (command_gate in the port): taken only when the sender is in the
        // list, since the list may leave them out while the server is busy with this command.
        let players = online(&*host);
        if players.iter().any(|p| p.uuid == event.player) {
            glue::players(players);
        }
        // A typed command whose node minecraft.command.<name> is false is refused, with the answer as a reply.
        // The level argument is not used by the core's gate.
        if let Gate::Refuse(line) = glue::gate(event.player, event.name, event.command, 0) {
            event.replies.push(line_text(&line));
            return Flow::Cancel;
        }
        // TODO(arismoko-api: player_command_level): a granted vanilla command (minecraft.command.<name> true) does not
        // run at the level it needs for a non-operator; the port lifted the level here. Refusals still work.
        // (`Command::with_node` covers only commands a mod registers, not vanilla roots.)
        // /lp: the core answers later, through the tick.
        if glue::lp_command(Some((event.player, event.name)), event.command) {
            return Flow::Cancel;
        }
        Flow::Continue
    }

    fn player_commands_sent(&self, _: &mut dyn ServerHost, event: &mut PlayerCommandsSentEvent<'_>) {
        // Refused roots leave the player's tree. TODO(arismoko-api: command_node): the roots the
        // level hides cannot be put back for a granted node (the port's names_hidden), so a granted
        // vanilla command of a non-operator stays out of the tree. `Command::with_node` is real now,
        // but only for roots a mod registers.
        glue::filter_tree(event.player, event.names);
    }

    /// The permission provider: LuckPerms' answer for a node of a player on this server. `None`
    /// leaves the decision to the next provider or the operator level (`player.has_permission`).
    fn player_permission_check(&self, _: &dyn Host, event: &mut PlayerPermissionCheckEvent<'_>) -> Option<bool> {
        glue::permission(event.player, event.node)
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


