//! The Vault provider: LuckPerms' `LuckPermsVaultPermission` and `LuckPermsVaultChat` (the `Permission` and `Chat`
//! services LuckPerms registers with Vault) as two mod services, `vault:permission` and `vault:chat`.
//!
//! Vault's own registry is a Bukkit `ServicesManager`; on Horizon a mod finds another mod through
//! `call_mod_service(service, payload)` (the way `luckperms:placeholder` is served). The payload is
//! `<method>\t<arg>\t<arg>...` and the reply is text: `true`/`false`, a string, or a list joined with a tab. A
//! missing world is `-` or empty. A player is a UUID (hyphenated or 32 hex digits), or the name of a player who
//! is online or whose data is loaded. A group is its name or display name.
//!
//! Reads come from the cache and never touch storage on the calling thread (LuckPerms' `vault-unsafe-lookups`
//! only matters for a lookup that does; here an unloaded user is a member of `default`, as for any other mod
//! API). Writes are queued on the worker as the equivalent `/lp` commands, run by the sender `Vault`, and answer
//! `true` once queued. Transient permissions do not exist in the port: their methods answer `false`.
//!
//! Config: `use-vault-server`, `vault-server`, `vault-include-global`, `vault-ignore-world`,
//! `vault-group-use-displaynames`, `vault-npc-group`, `vault-npc-op-status` and `vault-unsafe-lookups`.

use crate::horizon_luckperms_core::calc::{self, HolderRef, Resolved};
use crate::horizon_luckperms_core::config::Config;
use crate::horizon_luckperms_core::context::{self, ContextSet};
use crate::horizon_luckperms_core::model::{Group, Model, User};
use crate::horizon_luckperms_core::node::Kind;
use crate::horizon_luckperms_core::runtime::{CommandSender, Runtime};
use crate::horizon_luckperms_core::uuid;
use std::sync::Arc;

/// The services this module answers.
pub const PERMISSION_SERVICE: &str = "vault:permission";
pub const CHAT_SERVICE: &str = "vault:chat";
/// `getName()` of both services.
pub const NAME: &str = "LuckPerms";

/// A version 2 UUID is an NPC (`UniqueIdType.determineType`).
pub fn is_npc(id: u128) -> bool {
    (id >> 76) & 0xf == 2
}

enum Holder {
    User(u128),
    Group(String),
}

/// The Vault view of a runtime.
pub struct Vault<'a> {
    rt: &'a Runtime,
    cfg: Arc<Config>,
}

impl<'a> Vault<'a> {
    pub fn new(rt: &'a Runtime) -> Self {
        Vault { rt, cfg: rt.config() }
    }

    // ---- lookups ------------------------------------------------------------------------------------------

    /// `lookupUuid`: a UUID, an online player, or a user whose data is loaded.
    pub fn lookup_uuid(&self, player: &str) -> Option<u128> {
        let p = player.trim();
        if let Some(online) = self.rt.online_players().into_iter().find(|o| o.name.eq_ignore_ascii_case(p)) {
            return Some(online.uuid);
        }
        if let Some(id) = uuid::parse(p) {
            return Some(id);
        }
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        model.users.values().find(|u| u.username.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(p))).map(|u| u.uuid)
    }

    fn group<'m>(&self, model: &'m Model, name: &str) -> Option<&'m Group> {
        model.group_by_name_or_display(name)
    }

    /// `groupName`: the display name when `vault-group-use-displaynames`, else the name.
    fn group_name(&self, g: &Group) -> String {
        if self.cfg.vault_group_use_displaynames {
            g.display_name().unwrap_or_else(|| g.name.clone())
        } else {
            g.name.clone()
        }
    }

    /// `rewriteGroupName`: a display name becomes the group's name.
    fn rewrite_group(&self, model: &Model, name: &str) -> String {
        self.group(model, name).map(|g| g.name.clone()).unwrap_or_else(|| name.to_owned())
    }

    fn world_arg(&self, world: Option<&str>) -> Option<String> {
        if self.cfg.vault_ignore_world {
            return None;
        }
        world.map(str::trim).filter(|w| !w.is_empty() && *w != "-").map(str::to_owned)
    }

    /// The server context Vault reads and writes under (`getVaultServer`).
    fn vault_server(&self) -> String {
        if self.cfg.use_vault_server {
            self.cfg.vault_server.clone()
        } else {
            self.cfg.server.clone()
        }
    }

    /// `getQueryOptions`: the contexts and the op flag of a lookup.
    fn query(&self, id: Option<u128>, world: Option<&str>) -> (ContextSet, bool) {
        let online = id.and_then(|u| self.rt.online_player(u));
        let mut ctx = match &online {
            Some(p) => self.rt.contexts_of(p),
            None => self.rt.static_contexts(),
        };
        if let Some(w) = world.filter(|w| !w.is_empty()) {
            let in_it = online.is_some() && ctx.values(context::WORLD).iter().any(|v| v.eq_ignore_ascii_case(w));
            if !in_it {
                ctx.remove_key(context::WORLD);
                ctx.add(context::WORLD, &w.to_lowercase());
            }
        }
        if self.cfg.use_vault_server {
            let mut next = ContextSet::new();
            for (k, v) in ctx.pairs() {
                if !(k == context::SERVER && v == self.cfg.server) {
                    next.add(k, v);
                }
            }
            if self.cfg.vault_server != "global" {
                next.add(context::SERVER, &self.cfg.vault_server);
            }
            ctx = next;
        }
        let op = match &online {
            Some(p) => self.rt.is_op(p.op_level),
            None => id.is_some_and(is_npc) && self.cfg.vault_npc_op_status,
        };
        (ctx, op)
    }

    /// Who a UUID stands for: an NPC without loaded data is the `vault-npc-group` (else `default`).
    fn holder_of(&self, id: u128) -> Holder {
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        if model.users.contains_key(&id) || !is_npc(id) {
            return Holder::User(id);
        }
        let name = self.cfg.vault_npc_group.to_lowercase();
        Holder::Group(if model.groups.contains_key(&name) { name } else { "default".to_owned() })
    }

    fn resolve(&self, holder: &Holder, world: Option<&str>) -> Resolved {
        let (id, group) = match holder {
            Holder::User(u) => (Some(*u), None),
            Holder::Group(g) => (None, Some(g.clone())),
        };
        let (ctx, op) = self.query(id, world);
        let flags_differ = self.cfg.vault_include_global != self.cfg.include_global;
        if let (Some(u), false) = (id, flags_differ) {
            return (*self.rt.resolved(u, &ctx, op)).clone();
        }
        let mut cfg = (*self.cfg).clone();
        cfg.include_global = self.cfg.vault_include_global;
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        let now = crate::horizon_luckperms_core::duration::now();
        match (id, group) {
            (Some(u), _) if model.users.contains_key(&u) => calc::resolve(&model, &cfg, &HolderRef::User(u), &ctx, op, now),
            (Some(u), _) => {
                let mut m = model.clone();
                let mut user = User::new(u);
                user.give_default_if_needed();
                m.users.insert(u, user);
                calc::resolve(&m, &cfg, &HolderRef::User(u), &ctx, op, now)
            }
            (None, Some(g)) => calc::resolve(&model, &cfg, &HolderRef::Group(g), &ctx, false, now),
            (None, None) => unreachable!("a holder is a user or a group"),
        }
    }

    fn group_context_holder(&self, name: &str) -> Option<Holder> {
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        self.group(&model, name).map(|g| Holder::Group(g.name.clone()))
    }

    // ---- Permission ---------------------------------------------------------------------------------------

    /// `getGroups()`.
    pub fn groups(&self) -> Vec<String> {
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        model.groups.values().map(|g| self.group_name(g)).collect()
    }

    /// `userHasPermission`.
    pub fn user_has_permission(&self, world: Option<&str>, id: u128, permission: &str) -> bool {
        let world = self.world_arg(world);
        self.resolve(&self.holder_of(id), world.as_deref()).check(permission).result == calc::Tristate::True
    }

    /// `userInGroup`: the `group.<name>` node is true through the direct processor.
    pub fn user_in_group(&self, world: Option<&str>, id: u128, group: &str) -> bool {
        let world = self.world_arg(world);
        let key = {
            let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
            format!("group.{}", self.rewrite_group(&model, group))
        };
        let r = self.resolve(&self.holder_of(id), world.as_deref()).check(&key);
        r.processor == "direct" && r.result == calc::Tristate::True
    }

    /// `userGetGroups`: the groups the user is a direct, applicable member of.
    pub fn user_groups(&self, world: Option<&str>, id: u128) -> Vec<String> {
        let world = self.world_arg(world);
        let (ctx, _) = self.query(Some(id), world.as_deref());
        let now = crate::horizon_luckperms_core::duration::now();
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        let own = match self.holder_of_locked(&model, id) {
            Holder::Group(g) => model.groups.get(&g).map(|g| g.data.nodes.clone()),
            Holder::User(u) => model.users.get(&u).map(|u| u.data.nodes.clone()),
        };
        let names: Vec<String> = match own {
            Some(nodes) => nodes
                .iter()
                .filter_map(|n| match &n.kind {
                    Kind::Inheritance(g) if n.value && !n.has_expired(now) && n.contexts.is_satisfied_by(&ctx, context::satisfy_mode()) => Some(g.clone()),
                    _ => None,
                })
                .collect(),
            None => vec!["default".to_owned()],
        };
        names.into_iter().map(|g| model.groups.get(&g).map(|x| self.group_name(x)).unwrap_or(g)).collect()
    }

    fn holder_of_locked(&self, model: &Model, id: u128) -> Holder {
        if model.users.contains_key(&id) || !is_npc(id) {
            Holder::User(id)
        } else {
            let name = self.cfg.vault_npc_group.to_lowercase();
            Holder::Group(if model.groups.contains_key(&name) { name } else { "default".to_owned() })
        }
    }

    /// `userGetPrimaryGroup`: an NPC answers `vault-npc-group`.
    pub fn user_primary_group(&self, world: Option<&str>, id: u128) -> Option<String> {
        if let Holder::Group(_) = self.holder_of(id) {
            return Some(self.cfg.vault_npc_group.clone());
        }
        let world = self.world_arg(world);
        let value = self.resolve(&Holder::User(id), world.as_deref()).meta.primary_group?;
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        Some(self.group(&model, &value).map(|g| self.group_name(g)).unwrap_or(value))
    }

    /// `groupHasPermission`: false for a group that does not exist.
    pub fn group_has_permission(&self, world: Option<&str>, group: &str, permission: &str) -> bool {
        let world = self.world_arg(world);
        match self.group_context_holder(group) {
            Some(h) => self.resolve(&h, world.as_deref()).check(permission).result == calc::Tristate::True,
            None => false,
        }
    }

    // ---- writes (queued on the worker) --------------------------------------------------------------------

    fn sender() -> CommandSender {
        CommandSender { uuid: uuid::CONSOLE, name: "Vault".into(), console: true }
    }

    /// `server=<vault server>` and `world=<world>`: the contexts every Vault write carries (`global` is no context).
    fn write_contexts(&self, world: Option<&str>) -> Vec<String> {
        let mut out = Vec::new();
        let server = self.vault_server();
        if !server.eq_ignore_ascii_case("global") {
            out.push(format!("server={server}"));
        }
        if let Some(w) = world.filter(|w| !w.is_empty() && !w.eq_ignore_ascii_case("global")) {
            out.push(format!("world={w}"));
        }
        out
    }

    fn holder_args(&self, holder: &Holder) -> Vec<String> {
        match holder {
            Holder::User(u) => vec!["user".into(), uuid::format(*u)],
            Holder::Group(g) => vec!["group".into(), g.clone()],
        }
    }

    fn run(&self, mut args: Vec<String>, tail: &[String]) {
        args.extend_from_slice(tail);
        self.rt.submit_silent(Self::sender(), args);
    }

    fn set_permission(&self, holder: &Holder, world: Option<&str>, permission: &str, on: bool) -> bool {
        if permission.is_empty() {
            return false;
        }
        let world = self.world_arg(world);
        let mut args = self.holder_args(holder);
        args.extend(["permission".into(), if on { "set".into() } else { "unset".into() }, permission.to_owned()]);
        if on {
            args.push("true".into());
        }
        self.run(args, &self.write_contexts(world.as_deref()));
        true
    }

    /// `userAddPermission`; false for an NPC (LuckPerms throws).
    pub fn user_add_permission(&self, world: Option<&str>, id: u128, permission: &str) -> bool {
        matches!(self.holder_of(id), Holder::User(_)) && self.set_permission(&Holder::User(id), world, permission, true)
    }

    pub fn user_remove_permission(&self, world: Option<&str>, id: u128, permission: &str) -> bool {
        matches!(self.holder_of(id), Holder::User(_)) && self.set_permission(&Holder::User(id), world, permission, false)
    }

    /// `userAddGroup`: the group must exist.
    pub fn user_add_group(&self, world: Option<&str>, id: u128, group: &str) -> bool {
        let key = {
            let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
            self.group(&model, group).map(|g| format!("group.{}", g.name))
        };
        key.is_some_and(|k| self.user_add_permission(world, id, &k))
    }

    pub fn user_remove_group(&self, world: Option<&str>, id: u128, group: &str) -> bool {
        let key = {
            let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
            self.group(&model, group).map(|g| format!("group.{}", g.name))
        };
        key.is_some_and(|k| self.user_remove_permission(world, id, &k))
    }

    pub fn group_add_permission(&self, world: Option<&str>, group: &str, permission: &str) -> bool {
        self.group_context_holder(group).is_some_and(|h| self.set_permission(&h, world, permission, true))
    }

    pub fn group_remove_permission(&self, world: Option<&str>, group: &str, permission: &str) -> bool {
        self.group_context_holder(group).is_some_and(|h| self.set_permission(&h, world, permission, false))
    }

    // ---- Chat ---------------------------------------------------------------------------------------------

    /// `getUserChatPrefix`: empty when there is none.
    pub fn user_prefix(&self, world: Option<&str>, id: u128) -> String {
        let world = self.world_arg(world);
        self.resolve(&self.holder_of(id), world.as_deref()).meta.prefix.unwrap_or_default()
    }

    pub fn user_suffix(&self, world: Option<&str>, id: u128) -> String {
        let world = self.world_arg(world);
        self.resolve(&self.holder_of(id), world.as_deref()).meta.suffix.unwrap_or_default()
    }

    /// `getUserMeta`.
    pub fn user_meta(&self, world: Option<&str>, id: u128, key: &str) -> Option<String> {
        let world = self.world_arg(world);
        self.resolve(&self.holder_of(id), world.as_deref()).meta.meta_value(key).map(str::to_owned)
    }

    /// `getGroupChatPrefix`: None for a group that does not exist.
    pub fn group_prefix(&self, world: Option<&str>, group: &str) -> Option<String> {
        let world = self.world_arg(world);
        let h = self.group_context_holder(group)?;
        Some(self.resolve(&h, world.as_deref()).meta.prefix.unwrap_or_default())
    }

    pub fn group_suffix(&self, world: Option<&str>, group: &str) -> Option<String> {
        let world = self.world_arg(world);
        let h = self.group_context_holder(group)?;
        Some(self.resolve(&h, world.as_deref()).meta.suffix.unwrap_or_default())
    }

    pub fn group_meta(&self, world: Option<&str>, group: &str, key: &str) -> Option<String> {
        let world = self.world_arg(world);
        let h = self.group_context_holder(group)?;
        self.resolve(&h, world.as_deref()).meta.meta_value(key).map(str::to_owned)
    }

    /// The context pairs a node was stored with, as command arguments.
    fn node_contexts(c: &ContextSet) -> Vec<String> {
        c.ordered_pairs().into_iter().map(|(k, v)| format!("{k}={v}")).collect()
    }

    /// `setChatMeta`: removes every prefix (or suffix) of the holder, then sets the new one one step above the
    /// highest the holder has in the contexts of the write; an empty value only removes.
    fn set_chat(&self, holder: &Holder, prefix: bool, world: Option<&str>, value: Option<&str>) -> bool {
        let world = self.world_arg(world);
        let old: Vec<(i32, String, ContextSet)> = {
            let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
            let nodes = match holder {
                Holder::User(u) => model.users.get(u).map(|u| u.data.nodes.clone()),
                Holder::Group(g) => model.groups.get(g).map(|g| g.data.nodes.clone()),
            };
            nodes
                .unwrap_or_default()
                .into_iter()
                .filter_map(|n| match n.kind {
                    Kind::Prefix(p, v) if prefix => Some((p, v, n.contexts)),
                    Kind::Suffix(p, v) if !prefix => Some((p, v, n.contexts)),
                    _ => None,
                })
                .collect()
        };
        let verb = if prefix { "prefix" } else { "suffix" };
        for (p, v, c) in &old {
            let mut a = self.holder_args(holder);
            a.extend(["meta".into(), format!("remove{verb}"), p.to_string(), v.clone()]);
            self.run(a, &Self::node_contexts(c));
        }
        let Some(value) = value else { return true };
        let top = {
            let mut ctx = ContextSet::new();
            if let Some(w) = world.as_deref().filter(|w| !w.eq_ignore_ascii_case("global")) {
                ctx.add(context::WORLD, &w.to_lowercase());
            }
            ctx.add(context::SERVER, &self.vault_server());
            let m = self.resolve_in(holder, &ctx);
            let list = if prefix { m.meta.prefixes } else { m.meta.suffixes };
            // The old nodes are gone by the time the new one is set, so only what the other holders give counts.
            list.iter().map(|(p, _)| *p).max().unwrap_or(0)
        };
        let mut a = self.holder_args(holder);
        a.extend(["meta".into(), format!("add{verb}"), (top + 10).to_string(), value.to_owned()]);
        self.run(a, &self.write_contexts(world.as_deref()));
        true
    }

    fn resolve_in(&self, holder: &Holder, ctx: &ContextSet) -> Resolved {
        let mut cfg = (*self.cfg).clone();
        cfg.include_global = self.cfg.vault_include_global;
        let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
        let h = match holder {
            Holder::User(u) => HolderRef::User(*u),
            Holder::Group(g) => HolderRef::Group(g.clone()),
        };
        let now = crate::horizon_luckperms_core::duration::now();
        if let HolderRef::User(u) = &h {
            if !model.users.contains_key(u) {
                let mut m = model.clone();
                let mut user = User::new(*u);
                user.give_default_if_needed();
                m.users.insert(*u, user);
                return calc::resolve(&m, &cfg, &h, ctx, false, now);
            }
        }
        calc::resolve(&model, &cfg, &h, ctx, false, now)
    }

    /// `setMeta`: removes the holder's nodes of that key, then sets the value; `prefix` and `suffix` are chat meta.
    fn set_meta(&self, holder: &Holder, world: Option<&str>, key: &str, value: Option<&str>) -> bool {
        if key.eq_ignore_ascii_case("prefix") || key.eq_ignore_ascii_case("suffix") {
            return self.set_chat(holder, key.eq_ignore_ascii_case("prefix"), world, value);
        }
        let world = self.world_arg(world);
        let old: Vec<ContextSet> = {
            let model = self.rt.model.read().unwrap_or_else(|e| e.into_inner());
            let nodes = match holder {
                Holder::User(u) => model.users.get(u).map(|u| u.data.nodes.clone()),
                Holder::Group(g) => model.groups.get(g).map(|g| g.data.nodes.clone()),
            };
            nodes
                .unwrap_or_default()
                .into_iter()
                .filter_map(|n| match n.kind {
                    Kind::Meta(k, _) if k == key => Some(n.contexts),
                    _ => None,
                })
                .collect()
        };
        for c in &old {
            let mut a = self.holder_args(holder);
            a.extend(["meta".into(), "unset".into(), key.to_owned()]);
            self.run(a, &Self::node_contexts(c));
        }
        if let Some(v) = value {
            let mut a = self.holder_args(holder);
            a.extend(["meta".into(), "set".into(), key.to_owned(), v.to_owned()]);
            self.run(a, &self.write_contexts(world.as_deref()));
        }
        true
    }

    /// `setUserChatPrefix` (None removes it); false for an NPC.
    pub fn set_user_prefix(&self, world: Option<&str>, id: u128, value: Option<&str>) -> bool {
        matches!(self.holder_of(id), Holder::User(_)) && self.set_chat(&Holder::User(id), true, world, value)
    }

    pub fn set_user_suffix(&self, world: Option<&str>, id: u128, value: Option<&str>) -> bool {
        matches!(self.holder_of(id), Holder::User(_)) && self.set_chat(&Holder::User(id), false, world, value)
    }

    pub fn set_user_meta(&self, world: Option<&str>, id: u128, key: &str, value: Option<&str>) -> bool {
        matches!(self.holder_of(id), Holder::User(_)) && self.set_meta(&Holder::User(id), world, key, value)
    }

    pub fn set_group_prefix(&self, world: Option<&str>, group: &str, value: Option<&str>) -> bool {
        self.group_context_holder(group).is_some_and(|h| self.set_chat(&h, true, world, value))
    }

    pub fn set_group_suffix(&self, world: Option<&str>, group: &str, value: Option<&str>) -> bool {
        self.group_context_holder(group).is_some_and(|h| self.set_chat(&h, false, world, value))
    }

    pub fn set_group_meta(&self, world: Option<&str>, group: &str, key: &str, value: Option<&str>) -> bool {
        self.group_context_holder(group).is_some_and(|h| self.set_meta(&h, world, key, value))
    }

    // ---- the service --------------------------------------------------------------------------------------

    /// Answers one call of `vault:permission` or `vault:chat`; None for an unknown service or method, or a call
    /// whose player cannot be found.
    pub fn call(&self, service: &str, payload: &str) -> Option<String> {
        let mut parts = payload.split('\t');
        let method = parts.next()?.trim();
        let a: Vec<&str> = parts.collect();
        let arg = |i: usize| a.get(i).copied();
        let world = arg(0);
        let player = || self.lookup_uuid(arg(1)?);
        let b = |v: bool| Some(v.to_string());
        let opt = |s: Option<&str>| s.filter(|v| !v.is_empty());
        match service {
            PERMISSION_SERVICE => match method {
                "name" => Some(NAME.to_owned()),
                "groups" => Some(self.groups().join("\t")),
                "has" => b(self.user_has_permission(world, player()?, arg(2)?)),
                "in_group" => b(self.user_in_group(world, player()?, arg(2)?)),
                "player_groups" => Some(self.user_groups(world, player()?).join("\t")),
                "primary_group" => Some(self.user_primary_group(world, player()?).unwrap_or_default()),
                "group_has" => b(self.group_has_permission(world, arg(1)?, arg(2)?)),
                "add" => b(self.user_add_permission(world, player()?, arg(2)?)),
                "remove" => b(self.user_remove_permission(world, player()?, arg(2)?)),
                "add_transient" | "remove_transient" => b(false),
                "add_group" => b(self.user_add_group(world, player()?, arg(2)?)),
                "remove_group" => b(self.user_remove_group(world, player()?, arg(2)?)),
                "group_add" => b(self.group_add_permission(world, arg(1)?, arg(2)?)),
                "group_remove" => b(self.group_remove_permission(world, arg(1)?, arg(2)?)),
                _ => None,
            },
            CHAT_SERVICE => match method {
                "name" => Some(NAME.to_owned()),
                "player_prefix" => Some(self.user_prefix(world, player()?)),
                "player_suffix" => Some(self.user_suffix(world, player()?)),
                "set_player_prefix" => b(self.set_user_prefix(world, player()?, opt(arg(2)))),
                "set_player_suffix" => b(self.set_user_suffix(world, player()?, opt(arg(2)))),
                "player_meta" => Some(self.user_meta(world, player()?, arg(2)?).unwrap_or_default()),
                "set_player_meta" => b(self.set_user_meta(world, player()?, arg(2)?, opt(arg(3)))),
                "group_prefix" => Some(self.group_prefix(world, arg(1)?).unwrap_or_default()),
                "group_suffix" => Some(self.group_suffix(world, arg(1)?).unwrap_or_default()),
                "set_group_prefix" => b(self.set_group_prefix(world, arg(1)?, opt(arg(2)))),
                "set_group_suffix" => b(self.set_group_suffix(world, arg(1)?, opt(arg(2)))),
                "group_meta" => Some(self.group_meta(world, arg(1)?, arg(2)?).unwrap_or_default()),
                "set_group_meta" => b(self.set_group_meta(world, arg(1)?, arg(2)?, opt(arg(3)))),
                // `getPlayerInfoString/Integer/Double/Boolean` and the group variants: `<world> <who> <key> <default>`.
                "player_info" | "group_info" => {
                    let value = if method == "player_info" { self.user_meta(world, player()?, arg(2)?) } else { self.group_meta(world, arg(1)?, arg(2)?) };
                    Some(value.unwrap_or_else(|| arg(3).unwrap_or_default().to_owned()))
                }
                "player_info_int" | "group_info_int" => {
                    let value = if method == "player_info_int" { self.user_meta(world, player()?, arg(2)?) } else { self.group_meta(world, arg(1)?, arg(2)?) };
                    let default: i32 = arg(3).and_then(|d| d.trim().parse().ok()).unwrap_or(0);
                    Some(value.and_then(|v| v.trim().parse::<i32>().ok()).unwrap_or(default).to_string())
                }
                "player_info_double" | "group_info_double" => {
                    let value = if method == "player_info_double" { self.user_meta(world, player()?, arg(2)?) } else { self.group_meta(world, arg(1)?, arg(2)?) };
                    let default: f64 = arg(3).and_then(|d| d.trim().parse().ok()).unwrap_or(0.0);
                    Some(value.and_then(|v| v.trim().parse::<f64>().ok()).unwrap_or(default).to_string())
                }
                "player_info_bool" | "group_info_bool" => {
                    let value = if method == "player_info_bool" { self.user_meta(world, player()?, arg(2)?) } else { self.group_meta(world, arg(1)?, arg(2)?) };
                    let default = arg(3).is_some_and(|d| d.trim().eq_ignore_ascii_case("true"));
                    let parsed = value.and_then(|v| match v.to_lowercase().as_str() {
                        "true" => Some(true),
                        "false" => Some(false),
                        _ => None,
                    });
                    Some(parsed.unwrap_or(default).to_string())
                }
                _ => None,
            },
            _ => None,
        }
    }
}

/// `call_mod_service` for `vault:permission` and `vault:chat`; None while no service runs or for another service.
pub fn service(service: &str, payload: &str) -> Option<String> {
    if service != PERMISSION_SERVICE && service != CHAT_SERVICE {
        return None;
    }
    let rt = crate::horizon_luckperms_core::runtime::get()?;
    Vault::new(&rt).call(service, payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_luckperms_core::node::Node;
    use crate::horizon_luckperms_core::runtime::{detached, OnlinePlayer};

    const ALICE: u128 = 0x1111_2222_3333_4444_8555_6666_7777_8888;
    const NPC: u128 = 0x1111_2222_3333_2444_8555_6666_7777_8888;

    fn rt_with(cfg: Config) -> Runtime {
        let mut model = Model::default();
        let mut admin = Group::new("admin");
        admin.data.set(Node::permanent("vaultfake.build", true, ContextSet::new()));
        admin.data.set(Node::permanent("prefix.100.&c[Admin]", true, ContextSet::new()));
        admin.data.set(Node::permanent("meta.level.7", true, ContextSet::new()));
        admin.data.set(Node::permanent("displayname.Administrators", true, ContextSet::new()));
        model.groups.insert("admin".into(), admin);
        model.groups.insert("default".into(), Group::new("default"));
        model.groups.insert("npcs".into(), {
            let mut g = Group::new("npcs");
            g.data.set(Node::permanent("vaultfake.npc", true, ContextSet::new()));
            g
        });
        let mut alice = User::new(ALICE);
        alice.username = Some("Alice".into());
        alice.primary_group = "admin".into();
        alice.data.set(Node::permanent("group.admin", true, ContextSet::new()));
        alice.data.set(Node::permanent("vaultfake.fly", true, ContextSet::of(&[("world", "nether")])));
        model.users.insert(ALICE, alice);
        let rt = detached(cfg, model);
        rt.update_players(vec![OnlinePlayer { uuid: ALICE, name: "Alice".into(), op_level: 0, dimension: "minecraft:overworld".into(), game_mode: 0 }]);
        rt
    }

    #[test]
    fn permission_service_answers_checks_groups_and_worlds() {
        let rt = rt_with(Config::default());
        let v = Vault::new(&rt);
        assert_eq!(v.call(PERMISSION_SERVICE, "name"), Some("LuckPerms".into()));
        assert_eq!(v.call(PERMISSION_SERVICE, "has\t-\tAlice\tvaultfake.build").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "has\t-\tAlice\tvaultfake.nothing").as_deref(), Some("false"));
        // `world` is a context: the node is set for the nether only.
        assert_eq!(v.call(PERMISSION_SERVICE, "has\t-\tAlice\tvaultfake.fly").as_deref(), Some("false"));
        assert_eq!(v.call(PERMISSION_SERVICE, "has\tnether\tAlice\tvaultfake.fly").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "in_group\t-\tAlice\tadmin").as_deref(), Some("true"));
        // The display name works as the group name, and is what `vault-group-use-displaynames` answers.
        assert_eq!(v.call(PERMISSION_SERVICE, "in_group\t-\tAlice\tAdministrators").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "in_group\t-\tAlice\tdefault").as_deref(), Some("false"));
        assert_eq!(v.call(PERMISSION_SERVICE, "primary_group\t-\tAlice").as_deref(), Some("Administrators"));
        assert_eq!(v.call(PERMISSION_SERVICE, "player_groups\t-\tAlice").as_deref(), Some("Administrators"));
        assert_eq!(v.call(PERMISSION_SERVICE, "group_has\t-\tadmin\tvaultfake.build").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "group_has\t-\tmissing\tvaultfake.build").as_deref(), Some("false"));
        let mut names: Vec<String> = v.call(PERMISSION_SERVICE, "groups").unwrap().split('\t').map(str::to_owned).collect();
        names.sort();
        assert_eq!(names, ["Administrators", "default", "npcs"]);
        assert_eq!(v.call(PERMISSION_SERVICE, "add_transient\t-\tAlice\tx.y").as_deref(), Some("false"));
        assert_eq!(v.call(PERMISSION_SERVICE, "has\t-\tNobodyKnown\tx"), None);
        assert_eq!(v.call("vault:other", "name"), None);
    }

    #[test]
    fn config_keys_change_the_answers() {
        let mut cfg = Config::default();
        cfg.vault_group_use_displaynames = false;
        let rt = rt_with(cfg);
        assert_eq!(Vault::new(&rt).call(PERMISSION_SERVICE, "primary_group\t-\tAlice").as_deref(), Some("admin"));

        let mut cfg = Config::default();
        cfg.vault_ignore_world = true;
        let rt = rt_with(cfg);
        assert_eq!(Vault::new(&rt).call(PERMISSION_SERVICE, "has\tnether\tAlice\tvaultfake.fly").as_deref(), Some("false"), "the world is ignored");
    }

    #[test]
    fn npcs_use_the_npc_group_and_cannot_be_modified() {
        let mut cfg = Config::default();
        cfg.vault_npc_group = "npcs".into();
        let rt = rt_with(cfg);
        let v = Vault::new(&rt);
        assert!(is_npc(NPC) && !is_npc(ALICE));
        let id = uuid::format(NPC);
        assert_eq!(v.call(PERMISSION_SERVICE, &format!("has\t-\t{id}\tvaultfake.npc")).as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, &format!("primary_group\t-\t{id}")).as_deref(), Some("npcs"));
        assert_eq!(v.call(PERMISSION_SERVICE, &format!("add\t-\t{id}\tx.y")).as_deref(), Some("false"));
    }

    #[test]
    fn chat_service_reads_prefix_and_meta_with_typed_defaults() {
        let rt = rt_with(Config::default());
        let v = Vault::new(&rt);
        assert_eq!(v.call(CHAT_SERVICE, "player_prefix\t-\tAlice").as_deref(), Some("&c[Admin]"));
        assert_eq!(v.call(CHAT_SERVICE, "player_suffix\t-\tAlice").as_deref(), Some(""));
        assert_eq!(v.call(CHAT_SERVICE, "group_prefix\t-\tadmin").as_deref(), Some("&c[Admin]"));
        assert_eq!(v.call(CHAT_SERVICE, "player_meta\t-\tAlice\tlevel").as_deref(), Some("7"));
        assert_eq!(v.call(CHAT_SERVICE, "player_info_int\t-\tAlice\tlevel\t1").as_deref(), Some("7"));
        assert_eq!(v.call(CHAT_SERVICE, "player_info_int\t-\tAlice\tmissing\t42").as_deref(), Some("42"));
        assert_eq!(v.call(CHAT_SERVICE, "player_info_double\t-\tAlice\tlevel\t0.5").as_deref(), Some("7"));
        assert_eq!(v.call(CHAT_SERVICE, "player_info_bool\t-\tAlice\tlevel\ttrue").as_deref(), Some("true"), "7 is no boolean: the default");
        assert_eq!(v.call(CHAT_SERVICE, "group_info\t-\tadmin\tmissing\tfallback").as_deref(), Some("fallback"));
    }

    #[test]
    fn writes_are_queued_as_commands_of_the_vault_sender() {
        let mut cfg = Config::default();
        cfg.use_vault_server = true;
        cfg.vault_server = "hub".into();
        let rt = rt_with(cfg);
        let v = Vault::new(&rt);
        assert_eq!(v.write_contexts(Some("nether")), ["server=hub", "world=nether"]);
        assert_eq!(v.write_contexts(None), ["server=hub"]);
        let (rt2, _) = (rt_with(Config::default()), ());
        assert!(Vault::new(&rt2).write_contexts(Some("global")).is_empty());
        // Without a running worker the writes are accepted (and dropped), as the Vault provider answers true once queued.
        assert_eq!(v.call(PERMISSION_SERVICE, "add\t-\tAlice\tfoo.bar").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "group_add\t-\tadmin\tfoo.bar").as_deref(), Some("true"));
        assert_eq!(v.call(PERMISSION_SERVICE, "group_add\t-\tmissing\tfoo.bar").as_deref(), Some("false"));
        assert_eq!(v.call(PERMISSION_SERVICE, "add_group\t-\tAlice\tmissing").as_deref(), Some("false"));
    }
}
