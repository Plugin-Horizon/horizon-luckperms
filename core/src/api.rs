//! What other mods ask (Vault's permission and chat API, in short): permission checks, prefix,
//! suffix, meta and groups of an online player in their current contexts. All read from the
//! cache; none touches the database. Everything is "nothing" while the service is not running.

use crate::calc::{self, MetaData, Tristate};
use crate::runtime;
use std::collections::BTreeSet;
use std::sync::{Mutex, OnceLock};

fn default_true_nodes() -> &'static Mutex<BTreeSet<String>> {
    static NODES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    NODES.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Registers a node that Bukkit's plugin.yml would give `default: true`: a plugin port (or any mod) calls
/// this once at start for each such node it owns (`stage.use`, `voicechat.speak`). With
/// `apply-bukkit-default-permissions` on, the node is true for every player whose permissions leave it
/// undefined; a permission set to false still wins. Nothing else changes: the operator level and the
/// engine's own default apply where no node is registered.
pub fn register_default_true(node: &str) {
    let node = node.trim().to_lowercase();
    if !node.is_empty() {
        default_true_nodes().lock().unwrap_or_else(|e| e.into_inner()).insert(node);
    }
}

/// Whether `node` was registered with [`register_default_true`].
pub fn is_default_true(node: &str) -> bool {
    default_true_nodes().lock().unwrap_or_else(|e| e.into_inner()).contains(&node.to_lowercase())
}

/// The state of a permission (true, false or undefined).
pub fn check(player: u128, permission: &str) -> Tristate {
    runtime::get().map(|rt| rt.check(player, permission)).unwrap_or(Tristate::Undefined)
}

/// `Player.hasPermission`: true only when the node resolves true (ops: unless set false).
pub fn has_permission(player: u128, permission: &str) -> bool {
    check(player, permission) == Tristate::True
}

/// The node's value for this player: Some(true) or Some(false) when set (or registered as default-true),
/// None when undefined. Use it where a permissions mod answers "no opinion" (TAB's `has_permission`).
pub fn permission(player: u128, permission: &str) -> Option<bool> {
    check(player, permission).as_bool()
}

/// `hasPermission` with a fallback for undefined: the operator level a vanilla-style command
/// would need. This is the check Horizon's command layer would make (GAPS.md).
pub fn has_permission_or_level(player: u128, permission: &str, level: u8, player_level: u8) -> bool {
    match check(player, permission) {
        Tristate::True => true,
        Tristate::False => false,
        Tristate::Undefined => player_level >= level,
    }
}

fn meta(player: u128) -> Option<MetaData> {
    let rt = runtime::get()?;
    let p = rt.online_player(player)?;
    let ctx = rt.contexts_of(&p);
    Some(rt.resolved(player, &ctx, rt.is_op(p.op_level)).meta.clone())
}

/// The stacked prefix (`meta-formatting.prefix`), with its `&` codes as stored.
pub fn prefix(player: u128) -> Option<String> {
    meta(player)?.prefix
}

pub fn suffix(player: u128) -> Option<String> {
    meta(player)?.suffix
}

/// A meta value (`meta.<key>.<value>`), the first in inheritance order.
pub fn meta_value(player: u128, key: &str) -> Option<String> {
    meta(player)?.meta_value(key).map(str::to_owned)
}

pub fn primary_group(player: u128) -> Option<String> {
    meta(player)?.primary_group
}

/// The weight of the player's primary group (LuckPerms' `weight` in its placeholders): its highest
/// `weight.<n>` node, else the config's `group-weight`, else 0. None when the player is not on this server
/// or has no primary group.
pub fn weight(player: u128) -> Option<i32> {
    let rt = runtime::get()?;
    let primary = meta(player)?.primary_group?;
    let model = rt.model.read().ok()?;
    Some(calc::group_weight(&model, &rt.config(), &primary))
}

/// Every group the player inherits here, in inheritance order.
pub fn groups(player: u128) -> Vec<String> {
    let Some(rt) = runtime::get() else { return Vec::new() };
    let Some(p) = rt.online_player(player) else { return Vec::new() };
    let ctx = rt.contexts_of(&p);
    rt.resolved(player, &ctx, false).groups.clone()
}

pub fn in_group(player: u128, group: &str) -> bool {
    groups(player).iter().any(|g| g.eq_ignore_ascii_case(group))
}
