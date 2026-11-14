//! `/lp`: LuckPerms' command system, ported from `CommandManager` and the commands under `common/commands`:
//! the same sub commands, argument checks, permission nodes, usage texts and messages (colours included).
//! Runs on the worker thread (it may load offline users and always saves).

mod args;
mod editor;
mod fmt;
mod holder;
mod misc;
pub mod specs;
mod tables;
mod track;
mod usage;

use crate::actionlog::{Action, Part, TargetType};
use crate::calc::HolderRef;
use crate::config::Config;
use crate::context::ContextSet;
use crate::lang::{self, tr};
use crate::model::{Group, Model, NodeSet, User};
use crate::node::Node;
use crate::runtime::{CommandSender, Runtime};
use crate::storage::Storage;
use crate::text::prefixed;
use std::sync::Arc;

#[allow(unused_imports)]
pub(crate) use args::{ArgErr, Args, Check};
pub use misc::mark_started;
use tables::{find_main, holder_children, parent_subs, Auth, Child, MAINS};

/// The names `/lp` answers to.
pub const ROOTS: &[&str] = &["lp", "luckperms", "perm", "perms", "permission", "permissions"];

/// The label usage texts print (`/lp user <user>`).
pub const LABEL: &str = "lp";

/// The LuckPerms version this port follows, as `/lp` prints it.
pub const LUCKPERMS_VERSION: &str = "5.5.0";

/// What a command did, for the worker to finish (log, notify, ping the other servers).
#[derive(Debug, Default)]
pub struct CmdOutput {
    pub lines: Vec<String>,
    pub actions: Vec<Action>,
    /// Users whose data changed (a `userupdate` ping each).
    pub user_updates: Vec<u128>,
    /// A group or track changed (a buffered `update` ping).
    pub group_changed: bool,
    /// Reload everything from storage now.
    pub sync: bool,
    /// Send an `update` ping now (`/lp networksync`).
    pub network_sync: bool,
}

pub(crate) struct Ctx<'a> {
    pub rt: &'a Runtime,
    pub st: &'a mut dyn Storage,
    pub sender: &'a CommandSender,
    pub cfg: Arc<Config>,
    pub out: CmdOutput,
}

impl Ctx<'_> {
    /// A prefixed line (`Message.prefixed`).
    pub fn msg(&mut self, s: impl AsRef<str>) {
        self.out.lines.push(prefixed(s));
    }

    /// A line without the prefix.
    pub fn raw(&mut self, s: impl Into<String>) {
        self.out.lines.push(s.into());
    }

    /// A green message with its arguments and a full stop.
    pub fn ok(&mut self, template: &str, args: &[String]) {
        self.msg(format!("{}.", tr(template, 'a', args)));
    }

    /// A red message with its arguments and a full stop.
    pub fn bad(&mut self, template: &str, args: &[String]) {
        self.msg(format!("{}.", tr(template, 'c', args)));
    }

    /// `Message.COMMAND_NO_PERMISSION`
    pub fn no_permission(&mut self) {
        self.msg(tr(lang::T_COMMANDSYSTEM_NO_PERMISSION, 'c', &[]));
    }

    /// `Message.COMMAND_NOT_RECOGNISED`
    pub fn not_recognised(&mut self) {
        self.msg(format!("{}.", tr(lang::T_COMMANDSYSTEM_COMMAND_NOT_RECOGNISED, 'c', &[])));
    }

    /// `AbstractSender.hasPermission(CommandPermission)`: `luckperms.<node>`, honouring the read-only mode.
    pub fn has(&self, node: &str) -> bool {
        let ro = if self.sender.console { self.cfg.commands_read_only_mode.console } else { self.cfg.commands_read_only_mode.players };
        if ro && !tables::read_only(node) {
            return false;
        }
        self.rt.sender_may(self.sender, node)
    }

    /// Answers a bad argument.
    pub fn arg_err(&mut self, e: ArgErr, name: &str, spec_key: &str) {
        match e {
            ArgErr::Usage => self.detailed_usage(name, spec_key),
            ArgErr::PastDate => self.msg(tr(lang::T_COMMAND_MISC_DATE_IN_PAST_ERROR, 'c', &[])),
            ArgErr::InvalidDate(s) => self.bad(lang::T_COMMAND_MISC_DATE_PARSE_ERROR, &[fmt::dred(&s)]),
            ArgErr::InvalidPriority(s) => {
                let first = tr(lang::T_COMMAND_MISC_INVALID_PRIORITY, 'c', &[fmt::dred(&s)]);
                self.msg(format!("{first}. {}.", lang::T_COMMAND_MISC_EXPECTED_NUMBER));
            }
        }
    }

    /// `USER_LOAD_ERROR` and its kin: `An unexpected error occurred. <what>.`
    pub fn load_error(&mut self, what: &str) {
        self.msg(format!("{}. {what}.", tr(lang::T_COMMAND_MISC_LOADING_ERROR_UNEXPECTED, 'c', &[])));
    }

    pub fn log(&mut self, target_type: TargetType, target_uuid: Option<u128>, target_name: &str, parts: &[Part<'_>]) {
        let mut source_name = self.sender.name.clone();
        if self.sender.console && self.cfg.server != "global" {
            source_name = format!("{}@{}", source_name, self.cfg.server);
        }
        self.out.actions.push(Action {
            timestamp: crate::duration::now(),
            source_uuid: self.sender.uuid,
            source_name,
            target_type,
            target_uuid,
            target_name: target_name.to_owned(),
            description: crate::actionlog::describe(parts),
        });
    }

    pub fn model(&self) -> std::sync::RwLockReadGuard<'_, Model> {
        self.rt.model.read().unwrap_or_else(|e| e.into_inner())
    }
}

/// A user or group being changed, with its nodes as storage holds them.
pub(crate) enum Holder {
    User(User),
    Group(Group),
}

pub(crate) struct Loaded {
    pub h: Holder,
    pub before: Vec<Node>,
}

impl Holder {
    pub fn data(&self) -> &NodeSet {
        match self {
            Holder::User(u) => &u.data,
            Holder::Group(g) => &g.data,
        }
    }
    pub fn data_mut(&mut self) -> &mut NodeSet {
        match self {
            Holder::User(u) => &mut u.data,
            Holder::Group(g) => &mut g.data,
        }
    }
    /// The plain name (`getIdentifier().getName()`): a group's name, a user's name or uuid.
    pub fn name(&self) -> String {
        match self {
            Holder::User(u) => u.display(),
            Holder::Group(g) => g.name.clone(),
        }
    }
    /// `getFormattedDisplayName` as text inside an aqua argument: a group with a display name reads
    /// `name (display)`, the bracket part white.
    pub fn formatted(&self) -> String {
        match self {
            Holder::User(u) => u.display(),
            Holder::Group(g) => match g.display_name() {
                Some(d) => format!("{} \u{a7}f({d})", g.name),
                None => g.name.clone(),
            },
        }
    }
    /// The formatted name as a message argument.
    pub fn arg(&self) -> String {
        fmt::aq(&self.formatted())
    }
    pub fn is_user(&self) -> bool {
        matches!(self, Holder::User(_))
    }
    /// `user` or `group`: the permission tag.
    pub fn tag(&self) -> &'static str {
        match self {
            Holder::User(_) => "user",
            Holder::Group(_) => "group",
        }
    }
    pub fn target_type(&self) -> TargetType {
        match self {
            Holder::User(_) => TargetType::User,
            Holder::Group(_) => TargetType::Group,
        }
    }
    pub fn uuid(&self) -> Option<u128> {
        match self {
            Holder::User(u) => Some(u.uuid),
            Holder::Group(_) => None,
        }
    }
    pub fn holder_ref(&self) -> HolderRef {
        match self {
            Holder::User(u) => HolderRef::User(u.uuid),
            Holder::Group(g) => HolderRef::Group(g.name.clone()),
        }
    }
}

impl Ctx<'_> {
    /// `UserParentCommand.parseTargetUniqueId` and the lookup: a uuid, else a name known to this server or to the
    /// players table. Tells the sender when it cannot.
    pub fn find_user(&mut self, arg: &str) -> Option<u128> {
        if let Some(u) = crate::uuid::parse(arg) {
            return Some(u);
        }
        let lenient = !arg.is_empty() && arg.len() <= crate::node::MAX_USERNAME_LENGTH;
        let valid = lenient && (self.cfg.allow_invalid_usernames || crate::node::valid_username(arg));
        if !valid {
            self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_INVALID, &[fmt::dred(arg)]);
            return None;
        }
        if let Some(p) = self.rt.online_players().into_iter().find(|p| p.name.eq_ignore_ascii_case(arg)) {
            return Some(p.uuid);
        }
        let known = self.model().users.values().find(|u| u.username.as_deref().is_some_and(|n| n.eq_ignore_ascii_case(arg))).map(|u| u.uuid);
        if known.is_some() {
            return known;
        }
        match self.st.uuid_of(arg) {
            Ok(Some(u)) => Some(u),
            _ => {
                self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_NOT_FOUND, &[fmt::dred(arg)]);
                None
            }
        }
    }

    /// The user, from memory when loaded, else from storage.
    pub fn load_user(&mut self, uuid: u128) -> Option<Loaded> {
        let cached = self.model().users.get(&uuid).cloned();
        if let Some(mut u) = cached {
            u.data.audit_expired(crate::duration::now());
            let before = u.saved.clone();
            return Some(Loaded { h: Holder::User(u), before });
        }
        match self.st.load_user(uuid) {
            Ok((nodes, row)) => {
                let mut u = User::new(uuid);
                u.data.nodes = nodes.clone();
                u.saved = nodes.clone();
                if let Some(r) = row {
                    u.primary_group = r.primary_group.to_lowercase();
                    if r.username != "null" {
                        u.username = Some(r.username);
                    }
                }
                u.give_default_if_needed();
                u.data.audit_expired(crate::duration::now());
                Some(Loaded { h: Holder::User(u), before: nodes })
            }
            Err(_) => {
                self.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_USER);
                None
            }
        }
    }

    /// `StorageAssistant.loadGroup`: by name or display name; tells the sender when there is none.
    pub fn load_group(&mut self, name: &str) -> Option<Loaded> {
        let lower = name.to_lowercase();
        let g = self.model().group_by_name_or_display(name).cloned();
        match g {
            Some(mut g) => {
                g.data.audit_expired(crate::duration::now());
                let before = g.data.nodes.clone();
                Some(Loaded { h: Holder::Group(g), before })
            }
            None => {
                self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_NOT_FOUND, &[fmt::dred(&lower)]);
                None
            }
        }
    }

    pub fn group_exists(&self, name: &str) -> bool {
        self.model().groups.contains_key(&name.to_lowercase())
    }

    /// Saves a changed user or group and puts it into the model.
    pub fn save(&mut self, l: Loaded) -> bool {
        match l.h {
            Holder::User(mut u) => {
                u.saved = l.before;
                if self.st.save_user(&u).is_err() {
                    self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_SAVE_ERROR, &[fmt::dred(&u.display())]);
                    return false;
                }
                u.saved = if u.is_non_default() { u.data.nodes.clone() } else { Vec::new() };
                let uuid = u.uuid;
                let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
                if m.users.contains_key(&uuid) || self.rt.online_player(uuid).is_some() {
                    m.users.insert(uuid, u);
                }
                m.touch();
                drop(m);
                self.out.user_updates.push(uuid);
                true
            }
            Holder::Group(g) => {
                if self.st.save_group(&g, &l.before).is_err() {
                    let shown = Holder::Group(g.clone()).formatted();
                    self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_SAVE_ERROR, &[fmt::dred(&shown)]);
                    return false;
                }
                let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
                m.groups.insert(g.name.clone(), g);
                m.touch();
                drop(m);
                self.out.group_changed = true;
                true
            }
        }
    }

    /// The contexts a check without explicit contexts uses: an online user's current ones, the
    /// server's otherwise.
    pub fn default_check_contexts(&self, h: &Holder) -> ContextSet {
        if let Some(u) = h.uuid() {
            if let Some(p) = self.rt.online_player(u) {
                return self.rt.contexts_of(&p);
            }
        }
        self.rt.static_contexts()
    }

    /// `StorageAssistant.loadTrack`
    pub fn load_track(&mut self, name: &str) -> Option<crate::model::Track> {
        let lower = name.to_lowercase();
        let t = self.model().tracks.get(&lower).cloned();
        if t.is_none() {
            self.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_NOT_FOUND, &[fmt::dred(&lower)]);
        }
        t
    }
}

/// `CommandManager.applyConvenienceAliases`.
fn apply_aliases(args: &mut [String], rewrite_last: bool) {
    fn rewrite(args: &mut [String], i: usize, f: &dyn Fn(&str) -> Option<&'static str>) {
        let lower = args[i].to_lowercase();
        if let Some(r) = f(&lower) {
            args[i] = r.to_owned();
        }
    }
    if !args.is_empty() && (rewrite_last || args.len() >= 2) {
        rewrite(args, 0, &|a| match a {
            "u" => Some("user"),
            "g" => Some("group"),
            "t" => Some("track"),
            "i" => Some("info"),
            _ => None,
        });
    }
    if args.len() >= 3 && (rewrite_last || args.len() >= 4) {
        let a0 = args[0].to_lowercase();
        if a0 == "user" || a0 == "group" {
            rewrite(args, 2, &|a| match a {
                "p" | "perm" => Some("permission"),
                "g" | "group" => Some("parent"),
                "m" => Some("meta"),
                "i" => Some("info"),
                "e" => Some("editor"),
                _ => None,
            });
            if args.len() >= 4 && (rewrite_last || args.len() >= 5) {
                let a2 = args[2].to_lowercase();
                if a2 == "permission" || a2 == "parent" || a2 == "meta" {
                    rewrite(args, 3, &|a| if a == "i" { Some("info") } else { None });
                }
            }
        }
    }
}

/// Whether the sender may use a main command (`Command.isAuthorized`).
fn main_authorized(has: &dyn Fn(&str) -> bool, auth: Auth) -> bool {
    match auth {
        Auth::Node(n) => has(n),
        Auth::User => holder_children(true).iter().any(|ch| child_has(has, "user", ch)),
        Auth::Group => holder_children(false).iter().any(|ch| child_has(has, "group", ch)),
        Auth::Track => tables::TRACK_CHILDREN.iter().any(|ch| has(&format!("track.{}", ch.node))),
        Auth::Log => tables::LOG_CHILDREN.iter().any(|ch| has(&format!("log.{}", ch.node))),
    }
}

/// Whether the sender may use a child of `user <x>` / `group <x>`.
pub(crate) fn child_has(has: &dyn Fn(&str) -> bool, tag: &str, ch: &Child) -> bool {
    if ch.subs.is_empty() {
        has(&format!("{tag}.{}", ch.node))
    } else {
        parent_subs(ch, tag == "user").iter().any(|s| has(&format!("{tag}.{}", s.node)))
    }
}

fn usage_of(m: &tables::Main) -> String {
    specs::spec(m.spec).usage.unwrap_or("/%s").replace("%s", LABEL)
}

/// Runs `/lp <args>`.
pub fn execute(rt: &Runtime, st: &mut dyn Storage, sender: &CommandSender, args: &[String]) -> CmdOutput {
    let mut c = Ctx { rt, st, sender, cfg: rt.config(), out: CmdOutput::default() };
    let disabled = if sender.console { c.cfg.disable_luckperms_commands.console } else { c.cfg.disable_luckperms_commands.players };
    if disabled {
        c.msg(format!("{}.", tr(lang::T_COMMANDSYSTEM_COMMANDS_DISABLED, '3', &[])));
        return c.out;
    }
    let mut a: Vec<String> = args.to_vec();
    apply_aliases(&mut a, true);
    run(&mut c, a);
    c.out
}

fn run(c: &mut Ctx<'_>, mut a: Vec<String>) {
    let authorized = {
        let probe = |auth: Auth| -> bool { main_authorized(&|n: &str| c.has(n), auth) };
        MAINS.iter().map(|m| m.shown && probe(m.auth)).collect::<Vec<bool>>()
    };
    let main_ok = |name: &str| -> bool { MAINS.iter().zip(authorized.iter()).any(|(m, ok)| m.name == name && *ok) };

    // No arguments: the banner and where to go next.
    if a.is_empty() || (a.len() == 1 && a[0].trim().is_empty()) {
        c.banner();
        if authorized.iter().any(|ok| *ok) {
            c.msg(format!("{}.", tr(lang::T_COMMANDSYSTEM_AVAILABLE_COMMANDS, '3', &[format!("\u{a7}a/{LABEL} help")])));
            return;
        }
        let (groups, empty) = {
            let m = c.model();
            (m.groups.len(), m.groups.values().all(|g| g.data.nodes.is_empty()))
        };
        if groups <= 1 && empty {
            let name = c.sender.name.clone();
            c.first_time_setup(LABEL, &name);
        } else {
            c.no_permission_for_subcommands();
        }
        return;
    }

    let word = a[0].to_lowercase();
    // `isAuthorized` for a hidden command (`applyedits`) still counts: only the listing hides it.
    let found = find_main(&word);
    let usable = found.filter(|m| main_ok(m.name) || (!m.shown && main_authorized(&|n: &str| c.has(n), m.auth)));
    let Some(main) = usable else {
        let shown: Vec<(&str, String)> =
            MAINS.iter().zip(authorized.iter()).filter(|(_, ok)| **ok).map(|(m, _)| (m.name, usage_of(m))).collect();
        if shown.is_empty() {
            c.no_permission_for_subcommands();
            return;
        }
        c.command_usage(&shown);
        return;
    };
    a.remove(0);
    if main.check.test(a.len()) {
        c.detailed_usage(&capitalised(main.name), main.spec);
        return;
    }
    if main.unsupported {
        c.msg(format!("\u{a7}c/{LABEL} {} is not available on Horizon (see the mod's README).", main.name));
        return;
    }
    let args = Args::new(a);
    match main.name {
        "user" => holder::holder_command(c, true, args),
        "group" => holder::holder_command(c, false, args),
        "track" => track::track_command(c, args),
        "log" => misc::log_command(c, args),
        "sync" => misc::sync(c, false),
        "networksync" => misc::sync(c, true),
        "info" => misc::info(c),
        "reloadconfig" => misc::reload_config(c),
        "search" => misc::search(c, args),
        "creategroup" => misc::create_group(c, args),
        "deletegroup" => misc::delete_group(c, args),
        "listgroups" => misc::list_groups(c, args),
        "createtrack" => track::create_track(c, args),
        "deletetrack" => track::delete_track(c, args),
        "listtracks" => track::list_tracks(c),
        "editor" => editor::editor(c, args),
        "applyedits" => editor::apply_edits(c, args),
        "trusteditor" => editor::trust_editor(c, args),
        _ => c.not_recognised(),
    }
}

/// The name a single command's detailed usage shows (`CreateGroup`, `ListGroups`).
fn capitalised(name: &str) -> String {
    match name {
        "creategroup" => "CreateGroup".to_owned(),
        "deletegroup" => "DeleteGroup".to_owned(),
        "listgroups" => "ListGroups".to_owned(),
        "createtrack" => "CreateTrack".to_owned(),
        "deletetrack" => "DeleteTrack".to_owned(),
        "listtracks" => "ListTracks".to_owned(),
        "networksync" => "NetworkSync".to_owned(),
        "reloadconfig" => "ReloadConfig".to_owned(),
        "bulkupdate" => "BulkUpdate".to_owned(),
        "applyedits" => "ApplyEdits".to_owned(),
        "trusteditor" => "TrustEditor".to_owned(),
        other => other[..1].to_uppercase() + &other[1..],
    }
}

/// A parent's usage (`ParentCommand.sendUsage`): the header and the sub commands the sender may use.
pub(crate) fn tracks_log_usage(c: &mut Ctx<'_>, name: &str, children: &[Child], prefix: &str) {
    let usage = specs::spec(&name.to_lowercase()).usage.unwrap_or("").replace("%s", LABEL);
    let subs: Vec<(&str, &str)> = children.iter().filter(|ch| c.has(&format!("{prefix}.{}", ch.node))).map(|ch| (ch.name, ch.spec)).collect();
    c.parent_usage(name, &usage, &subs);
}

// ---- tab completion ------------------------------------------------------------------------------

fn starts(cands: Vec<String>, partial: &str) -> Vec<String> {
    let p = partial.to_lowercase();
    cands.into_iter().filter(|s| s.to_lowercase().starts_with(&p)).collect()
}

fn contains(cands: Vec<String>, partial: &str) -> Vec<String> {
    let p = partial.to_lowercase();
    cands.into_iter().filter(|s| s.to_lowercase().contains(&p)).collect()
}

fn booleans(partial: &str) -> Vec<String> {
    starts(vec!["true".into(), "false".into()], partial)
}

/// `TabCompletions.contexts`
fn context_completions(rt: &Runtime, partial: &str) -> Vec<String> {
    let cfg = rt.config();
    let mut keys: Vec<String> = vec!["server".into(), "world".into(), "gamemode".into(), "dimension-type".into()];
    for k in cfg.static_contexts.keys() {
        if !keys.contains(k) {
            keys.push(k.clone());
        }
    }
    match partial.find('=') {
        None => contains(keys, partial),
        Some(i) => {
            let key = &partial[..i];
            if key.trim().is_empty() {
                return Vec::new();
            }
            let value = partial[i + 1..].trim().to_lowercase();
            let mut values: Vec<String> = match key.to_lowercase().as_str() {
                "server" => vec![cfg.server.clone()],
                "gamemode" => ["survival", "creative", "adventure", "spectator"].iter().map(|s| s.to_string()).collect(),
                "dimension-type" => ["overworld", "the_nether", "the_end"].iter().map(|s| s.to_string()).collect(),
                "world" => {
                    let mut w: Vec<String> = Vec::new();
                    for p in rt.online_players() {
                        w.extend(rt.contexts_of(&p).values("world").into_iter().map(str::to_owned));
                    }
                    w
                }
                k => cfg.static_contexts.get(k).cloned().into_iter().collect(),
            };
            values.sort();
            values.dedup();
            values.into_iter().filter(|v| v.to_lowercase().starts_with(&value)).map(|v| format!("{key}={v}")).collect()
        }
    }
}

/// Completions for the last (partial) argument of `/lp <args>` (`CommandManager.tabCompleteCommand`).
pub fn complete(rt: &Runtime, sender: &CommandSender, args: &[String]) -> Vec<String> {
    let cfg = rt.config();
    let disabled = if sender.console { cfg.disable_luckperms_commands.console } else { cfg.disable_luckperms_commands.players };
    if disabled || args.is_empty() {
        return Vec::new();
    }
    let mut a: Vec<String> = args.to_vec();
    apply_aliases(&mut a, false);
    let (groups, tracks): (Vec<String>, Vec<String>) = {
        let m = rt.model.read().unwrap_or_else(|e| e.into_inner());
        (m.groups.keys().cloned().collect(), m.tracks.keys().cloned().collect())
    };
    let players: Vec<String> = rt.online_players().into_iter().map(|p| p.name).collect();
    let has = |node: &str| -> bool {
        let ro = if sender.console { cfg.commands_read_only_mode.console } else { cfg.commands_read_only_mode.players };
        if ro && !tables::read_only(node) {
            return false;
        }
        rt.sender_may(sender, node)
    };
    let last = a.len() - 1;
    let partial = a[last].clone();
    if last == 0 {
        let names = MAINS.iter().filter(|m| m.shown && main_authorized(&has, m.auth)).map(|m| m.name.to_string()).collect();
        return starts(names, &partial);
    }
    let Some(main) = MAINS.iter().find(|m| m.shown && m.name.eq_ignore_ascii_case(&a[0]) && main_authorized(&has, m.auth)) else {
        return Vec::new();
    };
    let rel: Vec<String> = a[1..].to_vec();
    let rl = rel.len() - 1;
    let ctxs = || context_completions(rt, &partial);
    match main.name {
        "user" | "group" => {
            let user = main.name == "user";
            if rl == 0 {
                return starts(if user { players } else { groups }, &partial);
            }
            let children = holder_children(user);
            if rl == 1 {
                let names = children.iter().filter(|ch| child_has(&has, main.name, ch)).map(|ch| ch.name.to_string()).collect();
                return starts(names, &partial);
            }
            let Some(ch) = children.iter().find(|ch| ch.name.eq_ignore_ascii_case(&rel[1]) && child_has(&has, main.name, ch)) else {
                return Vec::new();
            };
            let r2: Vec<String> = rel[2..].to_vec();
            let i = r2.len() - 1;
            if !ch.subs.is_empty() {
                let subs = parent_subs(ch, user);
                if i == 0 {
                    let names = subs.iter().filter(|s| has(&format!("{}.{}", main.name, s.node))).map(|s| s.name.to_string()).collect();
                    return starts(names, &partial);
                }
                let Some(sub) = subs.iter().find(|s| s.name.eq_ignore_ascii_case(&r2[0]) && has(&format!("{}.{}", main.name, s.node))) else {
                    return Vec::new();
                };
                return sub_completions(ch.name, sub.name, i - 1, &partial, &groups, &tracks, &ctxs);
            }
            match (ch.name, i) {
                ("promote" | "demote", 0) => starts(tracks, &partial),
                ("promote" | "demote" | "listmembers" | "setdisplayname", _) => {
                    if ch.name == "listmembers" && i == 0 || ch.name == "setdisplayname" && i == 0 {
                        Vec::new()
                    } else {
                        ctxs()
                    }
                }
                ("rename", 1) => starts(vec!["--update-parent-lists".into()], &partial),
                ("clear", _) => ctxs(),
                _ => Vec::new(),
            }
        }
        "track" => {
            if rl == 0 {
                return starts(tracks, &partial);
            }
            if rl == 1 {
                let names = tables::TRACK_CHILDREN.iter().filter(|ch| has(&format!("track.{}", ch.node))).map(|ch| ch.name.to_string()).collect();
                return starts(names, &partial);
            }
            let sub = rel[1].to_lowercase();
            if matches!(sub.as_str(), "append" | "insert" | "remove") && rl == 2 {
                starts(groups, &partial)
            } else {
                Vec::new()
            }
        }
        "log" => {
            if rl == 0 {
                let names = tables::LOG_CHILDREN.iter().filter(|ch| has(&format!("log.{}", ch.node))).map(|ch| ch.name.to_string()).collect();
                return starts(names, &partial);
            }
            let sub = rel[0].to_lowercase();
            match (sub.as_str(), rl) {
                ("grouphistory", 1) => starts(groups, &partial),
                ("trackhistory", 1) => starts(tracks, &partial),
                _ => Vec::new(),
            }
        }
        "deletegroup" => match rl {
            0 => starts(groups, &partial),
            1 => starts(vec!["--update-parent-lists".into()], &partial),
            _ => Vec::new(),
        },
        "deletetrack" => {
            if rl == 0 {
                starts(tracks, &partial)
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// The completion of a shared parent's sub command at argument `i` (counted after the sub command's name).
fn sub_completions(
    parent: &str,
    sub: &str,
    i: usize,
    partial: &str,
    groups: &[String],
    tracks: &[String],
    ctxs: &dyn Fn() -> Vec<String>,
) -> Vec<String> {
    let g = || starts(groups.to_vec(), partial);
    let t = || starts(tracks.to_vec(), partial);
    match (parent, sub) {
        ("permission", "set") => match i {
            0 => Vec::new(),
            1 => booleans(partial),
            _ => ctxs(),
        },
        ("permission", "unset" | "unsettemp") => {
            if i == 0 {
                Vec::new()
            } else {
                ctxs()
            }
        }
        ("permission", "settemp") => match i {
            1 => booleans(partial),
            0 | 2 => Vec::new(),
            _ => ctxs(),
        },
        ("permission", "clear") => ctxs(),
        ("parent", "set" | "add" | "remove" | "removetemp") => {
            if i == 0 {
                g()
            } else {
                ctxs()
            }
        }
        ("parent", "addtemp") => match i {
            0 => g(),
            1 => Vec::new(),
            _ => ctxs(),
        },
        ("parent", "settrack") => match i {
            0 => t(),
            1 => g(),
            _ => ctxs(),
        },
        ("parent", "clear") => ctxs(),
        ("parent", "cleartrack") => {
            if i == 0 {
                t()
            } else {
                ctxs()
            }
        }
        ("parent", "switchprimarygroup") => {
            if i == 0 {
                g()
            } else {
                Vec::new()
            }
        }
        ("meta", "set" | "addprefix" | "addsuffix" | "removeprefix" | "removesuffix" | "settempprefix" | "settempsuffix" | "removetempprefix" | "removetempsuffix") => {
            if i >= 2 {
                ctxs()
            } else {
                Vec::new()
            }
        }
        ("meta", "unset" | "unsettemp" | "setprefix" | "setsuffix") => {
            if i >= 1 {
                ctxs()
            } else {
                Vec::new()
            }
        }
        ("meta", "settemp" | "addtempprefix" | "addtempsuffix") => {
            if i >= 3 {
                ctxs()
            } else {
                Vec::new()
            }
        }
        ("meta", "clear") => ctxs(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests;
