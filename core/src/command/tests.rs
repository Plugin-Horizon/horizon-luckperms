//! `/lp` against the in-memory storage, with a runtime that has no worker.

use super::execute;
use crate::calc::Tristate;
use crate::config::Config;
use crate::model::Model;
use crate::runtime::{detached, CommandSender, OnlinePlayer, Runtime};
use crate::storage::{MemoryStorage, Storage};
use crate::text::split_args;

const ALICE: u128 = 0x1111;
const BOB: u128 = 0x2222;

fn setup() -> (Runtime, MemoryStorage) {
    let mut st = MemoryStorage::default();
    st.init().unwrap();
    let mut m = Model::default();
    m.ensure_default_group();
    let rt = detached(Config::default(), m);
    rt.update_players(vec![
        OnlinePlayer { uuid: ALICE, name: "Alice".into(), op_level: 0, dimension: "minecraft:overworld".into(), game_mode: 0 },
        OnlinePlayer { uuid: BOB, name: "Bob".into(), op_level: 4, dimension: "minecraft:the_nether".into(), game_mode: 1 },
    ]);
    st.save_player_data(ALICE, "Alice").unwrap();
    st.save_player_data(BOB, "Bob").unwrap();
    (rt, st)
}

fn run(rt: &Runtime, st: &mut MemoryStorage, sender: &CommandSender, line: &str) -> Vec<String> {
    let out = execute(rt, st, sender, &split_args(line));
    for a in &out.actions {
        st.log_action(a).unwrap();
    }
    out.lines.iter().map(|l| crate::text::plain(l)).collect()
}

fn console(rt: &Runtime, st: &mut MemoryStorage, line: &str) -> Vec<String> {
    run(rt, st, &CommandSender::console(), line)
}

#[test]
fn groups_permissions_and_inheritance() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "creategroup mod 50");
    console(&rt, &mut st, "group mod permission set essentials.fly true");
    console(&rt, &mut st, "group mod permission set essentials.nuke false server=global world=world_nether");
    let out = console(&rt, &mut st, "user Alice parent add mod");
    assert!(out[0].contains("now inherits permissions from mod in context global."), "{out:?}");
    // Alice is not loaded in the model (no login happened), so check through a fresh resolve
    // after putting her there the way a login would.
    let (nodes, _) = st.load_user(ALICE).unwrap();
    assert!(nodes.iter().any(|n| n.key == "group.mod"));
    let mut u = crate::model::User::new(ALICE);
    u.data.nodes = nodes.clone();
    u.saved = nodes;
    rt.model.write().unwrap().users.insert(ALICE, u);
    rt.model.write().unwrap().touch();
    assert_eq!(rt.check(ALICE, "essentials.fly"), Tristate::True);
    assert_eq!(rt.check(BOB, "essentials.nuke"), Tristate::True); // op, no node for Bob
    // Duplicate set is refused.
    let out = console(&rt, &mut st, "group mod permission set essentials.fly true");
    assert!(out[0].contains("already has"), "{out:?}");
    // The log describes it as LuckPerms does.
    let log = st.actions(10, None).unwrap();
    assert!(log.iter().any(|a| a.description == "permission set essentials.fly true" && a.target_name == "mod"));
    assert!(log.iter().any(|a| a.description == "parent add mod" && a.target_uuid == Some(ALICE)));
}

#[test]
fn players_need_permission() {
    let (rt, mut st) = setup();
    let alice = CommandSender { uuid: ALICE, name: "Alice".into(), console: false };
    // A sender with no command permission gets the command list (here: just the banner), as in LuckPerms.
    let out = run(&rt, &mut st, &alice, "creategroup x");
    assert_eq!(out, vec!["[LP] Running LuckPerms v5.5.0."], "{out:?}");
    // An op may (commands-allow-op).
    let bob = CommandSender { uuid: BOB, name: "Bob".into(), console: false };
    let out = run(&rt, &mut st, &bob, "creategroup x");
    assert!(out[0].contains("successfully created"), "{out:?}");
    // A granted node lets Alice in.
    console(&rt, &mut st, "group default permission set luckperms.listgroups true");
    let out = run(&rt, &mut st, &alice, "listgroups");
    assert!(out[0].contains("Showing group entries:"), "{out:?}");
    assert!(out[1].contains("Groups: (name, weight, tracks)"), "{out:?}");
}

#[test]
fn prefixes_with_spaces_and_meta() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "creategroup admin 100");
    let out = console(&rt, &mut st, r#"group admin meta addprefix 100 "&c[Admin] ""#);
    assert!(out[0].contains("admin had prefix '[Admin] ' set at a priority of 100 in context global."), "{out:?}");
    console(&rt, &mut st, "group admin meta set color red");
    console(&rt, &mut st, "group admin meta set color blue");
    let g = rt.model.read().unwrap().groups.get("admin").cloned().unwrap();
    assert!(g.data.nodes.iter().any(|n| n.key == "prefix.100.&c[Admin] "));
    assert_eq!(g.data.nodes.iter().filter(|n| n.key.starts_with("meta.color.")).count(), 1);
    console(&rt, &mut st, "user Bob parent set admin");
    let (nodes, row) = st.load_user(BOB).unwrap();
    assert!(nodes.iter().any(|n| n.key == "group.admin"));
    assert_eq!(row.unwrap().primary_group, "admin");
    let mut u = crate::model::User::new(BOB);
    u.data.nodes = nodes;
    u.primary_group = "admin".into();
    rt.model.write().unwrap().users.insert(BOB, u);
    rt.model.write().unwrap().touch();
    let ctx = rt.contexts_of(&rt.online_player(BOB).unwrap());
    let r = rt.resolved(BOB, &ctx, false);
    assert_eq!(r.meta.prefix.as_deref(), Some("&c[Admin] "));
    assert_eq!(r.meta.meta_value("color"), Some("blue"));
    // removeprefix without a value removes every prefix at that priority.
    console(&rt, &mut st, "group admin meta removeprefix 100");
    assert!(!rt.model.read().unwrap().groups["admin"].data.nodes.iter().any(|n| n.key.starts_with("prefix.")));
}

#[test]
fn tracks_promote_demote() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "creategroup helper 10");
    console(&rt, &mut st, "creategroup mod 20");
    console(&rt, &mut st, "createtrack staff");
    console(&rt, &mut st, "track staff append helper");
    console(&rt, &mut st, "track staff append mod");
    let out = console(&rt, &mut st, "user Alice promote staff");
    assert!(out[0].contains("added to the first group, helper in context global."), "{out:?}");
    let out = console(&rt, &mut st, "user Alice promote staff");
    assert!(out[0].contains("from helper to mod"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice promote staff");
    assert!(out[0].contains("end of track"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice demote staff");
    assert!(out[0].contains("from mod to helper"), "{out:?}");
    let (nodes, _) = st.load_user(ALICE).unwrap();
    assert!(nodes.iter().any(|n| n.key == "group.helper"));
    assert!(!nodes.iter().any(|n| n.key == "group.mod"));
    let log = st.actions(20, Some((crate::actionlog::TargetType::User, &crate::uuid::format(ALICE)))).unwrap();
    assert!(log.iter().any(|a| a.description == "promote staff"), "{log:?}");
}

#[test]
fn temporary_nodes() {
    let (rt, mut st) = setup();
    let out = console(&rt, &mut st, "user Alice permission settemp fly.temp true 1h");
    assert!(out[0].contains("for a duration of"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice permission settemp fly.temp true 1h");
    assert!(out[0].contains("already has"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice permission settemp fly.temp true 1h accumulate");
    assert!(out[0].contains("1 hour 59 minutes") || out[0].contains("2 hours"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice permission unsettemp fly.temp");
    assert!(out[0].contains("Unset temporary"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice parent addtemp default 10m");
    assert!(out[0].contains("for a duration"), "{out:?}");
}

#[test]
fn search_and_members() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "creategroup vip");
    console(&rt, &mut st, "user Alice parent add vip");
    console(&rt, &mut st, "user Bob permission set voicechat.speak true");
    let out = console(&rt, &mut st, "group vip listmembers");
    assert!(out.iter().any(|l| l.contains("alice")), "{out:?}");
    let out = console(&rt, &mut st, "search ~~ voicechat.%");
    assert!(out.iter().any(|l| l.contains("bob") && l.contains("voicechat.speak")), "{out:?}");
    let out = console(&rt, &mut st, "search !~ voicechat.%");
    assert!(!out.iter().any(|l| l.contains("voicechat.speak")), "{out:?}");
}

#[test]
fn check_reports_processor() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "group default permission set essentials.* true");
    let out = console(&rt, &mut st, "user Alice permission check essentials.home");
    assert!(out.iter().any(|l| l.contains("Result: true")), "{out:?}");
    assert!(out.iter().any(|l| l.contains("Processor: common.WildcardProcessor")), "{out:?}");
}

#[test]
fn completions() {
    let (rt, _st) = setup();
    let c = rt.complete(&CommandSender::console(), &split_args("user Al"));
    assert_eq!(c, vec!["Alice".to_string()]);
    let c = rt.complete(&CommandSender::console(), &["group".into(), "default".into(), "perm".into()]);
    assert_eq!(c, vec!["permission".to_string()]);
}

#[test]
fn command_gate_decisions() {
    let (rt, mut st) = setup();
    console(&rt, &mut st, "group default permission set minecraft.command.gamemode true");
    console(&rt, &mut st, "user Bob permission set minecraft.command.stop false");
    // Users loaded as a login would.
    for u in [ALICE, BOB] {
        let (nodes, _) = st.load_user(u).unwrap();
        let mut user = crate::model::User::new(u);
        user.data.nodes = nodes;
        user.give_default_if_needed();
        rt.model.write().unwrap().users.insert(u, user);
    }
    rt.model.write().unwrap().touch();
    assert_eq!(rt.command_decision(ALICE, "gamemode"), Some(true));
    assert_eq!(rt.command_decision(ALICE, "tp"), None);
    assert_eq!(rt.command_decision(BOB, "stop"), Some(false));
    assert_eq!(rt.command_decision(BOB, "lp"), None);
}

#[test]
fn operator_sees_help_and_info() {
    let (rt, mut st) = setup();
    let bob = CommandSender { uuid: BOB, name: "Bob".into(), console: false };
    let help = run(&rt, &mut st, &bob, "");
    assert!(!help.iter().any(|l| l.contains("no permissions have been set up")), "{help:?}");
    let info = run(&rt, &mut st, &bob, "info");
    assert!(!info.iter().any(|l| l.contains("permission")), "{info:?}");
}

#[test]
fn unknown_command_lists_the_commands() {
    let (rt, mut st) = setup();
    let out = console(&rt, &mut st, "foo");
    assert_eq!(out[0], "[LP] Running LuckPerms v5.5.0.");
    assert!(out.iter().any(|l| l == "> /lp user <user>"), "{out:?}");
    assert!(out.iter().any(|l| l == "> /lp search <permission>"), "{out:?}");
    // `help` is not a command of its own.
    assert_eq!(console(&rt, &mut st, "help"), out);
}

#[test]
fn detailed_usage_and_sub_command_list() {
    let (rt, mut st) = setup();
    let out = console(&rt, &mut st, "creategroup");
    assert_eq!(out[0], "[LP] Command Usage - CreateGroup");
    assert_eq!(out[1], "[LP] > Create a new group");
    assert_eq!(out[2], "[LP] Arguments:");
    assert_eq!(out[3], "[LP] - <name> -> the name of the group");
    let out = console(&rt, &mut st, "user Alice");
    assert_eq!(out[0], "[LP] User Sub Commands: (/lp user <user> ...)");
    assert!(out.iter().any(|l| l == "> info"), "{out:?}");
    assert!(out.iter().any(|l| l == "> permission"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice permission");
    assert_eq!(out[0], "[LP] Permission Sub Commands: (/lp user <user> permission ...)");
    assert!(out.iter().any(|l| l == "> set - <node> [true|false] [context...]"), "{out:?}");
    let out = console(&rt, &mut st, "user Alice nothing");
    assert_eq!(out, vec!["[LP] Command not recognised."]);
}

#[test]
fn convenience_aliases_and_errors() {
    let (rt, mut st) = setup();
    let out = console(&rt, &mut st, "u Alice p set a.b");
    assert_eq!(out[0], "[LP] Set a.b to true for alice in context global.");
    let out = console(&rt, &mut st, "user Nobody info");
    assert_eq!(out, vec!["[LP] A user for Nobody could not be found."]);
    let out = console(&rt, &mut st, "user Alice permission settemp a.c true abc");
    assert_eq!(out, vec!["[LP] Could not parse date abc."]);
    let out = console(&rt, &mut st, "user Alice permission settemp a.c true 5");
    assert_eq!(out, vec!["[LP] You cannot set a date in the past!"]);
    let out = console(&rt, &mut st, "user Alice meta addprefix x pre");
    assert_eq!(out, vec!["[LP] Invalid priority x. Expected a number."]);
    let out = console(&rt, &mut st, "group nothere info");
    assert_eq!(out, vec!["[LP] A group named nothere could not be found."]);
    let out = console(&rt, &mut st, "creategroup default");
    assert_eq!(out, vec!["[LP] default already exists!."]);
    let out = console(&rt, &mut st, "deletegroup default");
    assert_eq!(out, vec!["[LP] You cannot delete the default group."]);
}

#[test]
fn contexts_and_clear_message() {
    let (rt, mut st) = setup();
    let out = console(&rt, &mut st, "group default permission set a.b true survival nether");
    assert_eq!(out[0], "[LP] Set a.b to true for default in context server=survival, world=nether.");
    let out = console(&rt, &mut st, "group default permission clear");
    assert_eq!(out[0], "[LP] default's permissions were cleared in context global. (1 node was removed.)");
}
