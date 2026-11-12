//! The editor's autocomplete list (`knownPermissions`): what LuckPerms' `PermissionRegistry` holds on Bukkit.
//! Here that is the vanilla `minecraft.command.<name>` nodes, the nodes mapped in `command-nodes`, LuckPerms'
//! own, and every node the server or a mod asked about or registered a command for since start
//! (the "verbose"-style collector; the glue records them as they come).

use crate::config::Config;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

/// Bound for the recorded set, so a mod asking with generated nodes cannot grow it forever.
const MAX_RECORDED: usize = 20_000;

/// The command roots vanilla 26.x registers (the ones Paper guards as `minecraft.command.<name>`).
pub const VANILLA_COMMANDS: &[&str] = &[
    "advancement", "attribute", "ban", "ban-ip", "banlist", "bossbar", "clear", "clone", "damage", "data", "datapack",
    "debug", "defaultgamemode", "deop", "dialog", "difficulty", "effect", "enchant", "execute", "experience", "fill",
    "fillbiome", "forceload", "function", "gamemode", "gamerule", "give", "help", "item", "jfr", "kick", "kill", "list",
    "locate", "loot", "me", "msg", "op", "pardon", "pardon-ip", "particle", "perf", "place", "playsound", "publish",
    "random", "recipe", "reload", "return", "ride", "rotate", "save-all", "save-off", "save-on", "say", "schedule",
    "scoreboard", "seed", "setblock", "setidletimeout", "setworldspawn", "spawnpoint", "spectate", "spreadplayers", "stop",
    "stopsound", "summon", "tag", "team", "teammsg", "teleport", "tellraw", "tick", "time", "title", "tm", "tp", "transfer",
    "trigger", "waypoint", "weather", "whitelist", "worldborder", "xp", "tell", "w",
];

/// LuckPerms' own command nodes (`CommandPermission`), plus the wildcard.
pub const LUCKPERMS_NODES: &[&str] = &[
    "luckperms.*", "luckperms.sync", "luckperms.info", "luckperms.editor", "luckperms.verbose", "luckperms.tree",
    "luckperms.search", "luckperms.check", "luckperms.import", "luckperms.export", "luckperms.reloadconfig",
    "luckperms.bulkupdate", "luckperms.translations", "luckperms.creategroup", "luckperms.deletegroup",
    "luckperms.listgroups", "luckperms.createtrack", "luckperms.deletetrack", "luckperms.listtracks",
    "luckperms.user.info", "luckperms.user.permission.info", "luckperms.user.permission.set", "luckperms.user.permission.unset",
    "luckperms.user.permission.settemp", "luckperms.user.permission.unsettemp", "luckperms.user.permission.check",
    "luckperms.user.permission.checkinherits", "luckperms.user.permission.clear", "luckperms.user.parent.info",
    "luckperms.user.parent.set", "luckperms.user.parent.add", "luckperms.user.parent.remove", "luckperms.user.parent.settrack",
    "luckperms.user.parent.addtemp", "luckperms.user.parent.removetemp", "luckperms.user.parent.cleartrack",
    "luckperms.user.parent.clear", "luckperms.user.meta.info", "luckperms.user.meta.set", "luckperms.user.meta.unset",
    "luckperms.user.meta.settemp", "luckperms.user.meta.unsettemp", "luckperms.user.meta.addprefix",
    "luckperms.user.meta.addsuffix", "luckperms.user.meta.setprefix", "luckperms.user.meta.setsuffix",
    "luckperms.user.meta.removeprefix", "luckperms.user.meta.removesuffix", "luckperms.user.meta.clear",
    "luckperms.user.editor", "luckperms.user.promote", "luckperms.user.demote", "luckperms.user.show-tracks",
    "luckperms.user.clear", "luckperms.user.clone", "luckperms.group.info", "luckperms.group.permission.info",
    "luckperms.group.permission.set", "luckperms.group.permission.unset", "luckperms.group.parent.info",
    "luckperms.group.parent.add", "luckperms.group.parent.remove", "luckperms.group.meta.info", "luckperms.group.meta.set",
    "luckperms.group.meta.unset", "luckperms.group.editor", "luckperms.group.listmembers", "luckperms.group.setweight",
    "luckperms.group.setdisplayname", "luckperms.group.showtracks", "luckperms.group.clear", "luckperms.group.rename",
    "luckperms.group.clone", "luckperms.track.info", "luckperms.track.editor", "luckperms.track.append",
    "luckperms.track.insert", "luckperms.track.remove", "luckperms.track.clear", "luckperms.track.rename",
    "luckperms.track.clone", "luckperms.log.recent", "luckperms.log.search", "luckperms.log.notify",
    "luckperms.log.userhistory", "luckperms.log.grouphistory", "luckperms.log.trackhistory", "luckperms.log.export",
];

fn recorded() -> &'static Mutex<BTreeSet<String>> {
    static SET: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(BTreeSet::new()))
}

fn plausible(node: &str) -> bool {
    !node.is_empty() && node.len() <= 200 && !node.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// Remembers a node some caller asked about or guards a command with.
pub fn record(node: &str) {
    if !plausible(node) {
        return;
    }
    let node = node.to_lowercase();
    if let Ok(mut s) = recorded().lock() {
        if s.len() < MAX_RECORDED && !s.contains(&node) {
            s.insert(node);
        }
    }
}

/// Remembers the guard node of every command root in a command tree sent to a player.
pub fn record_commands(cfg: &Config, names: &[String]) {
    for n in names {
        let root = crate::glue::root_of(n);
        if !root.is_empty() {
            record(&cfg.command_node(&root));
        }
    }
}

/// Every node the editor may suggest, sorted and without duplicates.
pub fn all(cfg: &Config) -> Vec<String> {
    let mut set: BTreeSet<String> = BTreeSet::new();
    set.extend(VANILLA_COMMANDS.iter().map(|c| cfg.command_node(c)));
    set.extend(cfg.horizon.command_nodes.values().cloned());
    set.extend(LUCKPERMS_NODES.iter().map(|n| (*n).to_owned()));
    if let Ok(s) = recorded().lock() {
        set.extend(s.iter().cloned());
    }
    set.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_vanilla_luckperms_and_recorded_nodes() {
        record("mymod.use");
        record("bad node");
        let k = all(&Config::default());
        assert!(k.contains(&"minecraft.command.gamemode".to_owned()));
        assert!(k.contains(&"luckperms.editor".to_owned()));
        assert!(k.contains(&"mymod.use".to_owned()));
        assert!(!k.iter().any(|n| n.contains(' ')));
        assert!(k.windows(2).all(|w| w[0] < w[1]), "sorted, no duplicates");
    }
}
