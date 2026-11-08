//! `/lp` itself: sync, info, search, group management, the log.

use super::args::Args;
use super::fmt::{self, aq, ctx_bracketed, dred, green};
use super::holder::page_of;
use super::tables::LOG_CHILDREN;
use super::{tracks_log_usage, Ctx, Holder, Loaded, LUCKPERMS_VERSION};
use crate::actionlog::{Action, Part, TargetType};
use crate::context::ContextSet;
use crate::duration;
use crate::lang::{self, tr};
use crate::model::{Group, DEFAULT_GROUP};
use crate::node::{key, valid_group_name, valid_track_name, Node};
use crate::storage::{ActionFilter, HolderKey, KeyMatch};
use std::sync::OnceLock;
use std::time::Instant;

static STARTED: OnceLock<Instant> = OnceLock::new();

/// Called when the runtime starts: the uptime `/lp info` shows counts from here.
pub fn mark_started() {
    let _ = STARTED.set(Instant::now());
}

/// `/lp sync` and `/lp networksync`
pub(crate) fn sync(c: &mut Ctx<'_>, network: bool) {
    c.msg(format!("{}.", tr(lang::T_COMMAND_UPDATE_TASK_REQUEST, 'b', &[])));
    c.out.sync = true;
    if !network {
        c.msg(format!("{}.", tr(lang::T_COMMAND_UPDATE_TASK_COMPLETE, 'a', &[])));
        return;
    }
    c.msg(format!("{}. {}.", tr(lang::T_COMMAND_UPDATE_TASK_COMPLETE, 'a', &[]), lang::T_COMMAND_UPDATE_TASK_PUSH_ATTEMPTING));
    if !c.rt.messaging_on() {
        c.bad(lang::T_COMMAND_UPDATE_TASK_PUSH_ERROR_NOT_SETUP, &[]);
        return;
    }
    c.out.network_sync = true;
    c.ok(lang::T_COMMAND_UPDATE_TASK_PUSH_COMPLETE, &[aq("SQL Messaging")]);
}

/// `/lp reloadconfig`
pub(crate) fn reload_config(c: &mut Ctx<'_>) {
    // The game's config watcher reloads config/luckperms.toml within a second of a save.
    c.msg(format!(
        "{}. \u{a7}7({})",
        tr(lang::T_COMMAND_RELOAD_CONFIG_SUCCESS, 'a', &[]),
        lang::T_COMMAND_RELOAD_CONFIG_RESTART_NOTE
    ));
}

/// `DurationFormatter.CONCISE_LOW_ACCURACY`: at most three units.
fn low_accuracy(secs: i64) -> String {
    let full = duration::format_concise(secs);
    full.split(' ').take(3).collect::<Vec<_>>().join(" ")
}

/// `/lp info`
pub(crate) fn info(c: &mut Ctx<'_>) {
    let (groups, tracks, users) = {
        let m = c.model();
        (m.groups.len(), m.tracks.len(), m.users.len())
    };
    let online = c.rt.online_players().len();
    let storage = match c.rt.storage_name() {
        "mariadb" => "MariaDB".to_owned(),
        "mysql" => "MySQL".to_owned(),
        other => other.to_owned(),
    };
    let none_white = format!("\u{a7}f{}", lang::T_COMMAND_MISC_NONE);
    let uptime = STARTED.get().map(|s| s.elapsed().as_secs() as i64).unwrap_or(0);
    c.msg(format!(
        "\u{a7}2{} \u{a7}bLuckPerms \u{a7}bv{LUCKPERMS_VERSION}\u{a7}2 by \u{a7}bLuck\u{a7}2.",
        lang::T_COMMAND_INFO_RUNNING_PLUGIN
    ));
    c.msg(format!("\u{a7}3\u{a7}f-  \u{a7}3{}: \u{a7}fHorizon", lang::T_COMMAND_INFO_PLATFORM_KEY));
    c.msg(format!("\u{a7}3\u{a7}f-  \u{a7}3{}: \u{a7}fHorizon", lang::T_COMMAND_INFO_SERVER_BRAND_KEY));
    c.msg(format!("\u{a7}3\u{a7}f-  \u{a7}3{}:", lang::T_COMMAND_INFO_SERVER_VERSION_KEY));
    c.msg("\u{a7}f     Horizon");
    c.msg(format!("\u{a7}b\u{a7}f-  \u{a7}b{}:", lang::T_COMMAND_INFO_STORAGE_KEY));
    c.msg(format!("\u{a7}3     {}: \u{a7}f{storage}", lang::T_COMMAND_INFO_STORAGE_TYPE_KEY));
    c.msg(format!("\u{a7}b\u{a7}f-  \u{a7}b{}:", lang::T_COMMAND_INFO_EXTENSIONS_KEY));
    c.msg(format!("\u{a7}f     \u{a7}b{}", lang::T_COMMAND_MISC_NONE));
    let messaging = if c.rt.messaging_on() { "\u{a7}fSQL".to_owned() } else { none_white.clone() };
    c.msg(format!("\u{a7}b\u{a7}f-  \u{a7}b{}: {messaging}", lang::T_COMMAND_INFO_MESSAGING_KEY));
    c.msg(format!("\u{a7}b\u{a7}f-  \u{a7}b{}:", lang::T_COMMAND_INFO_INSTANCE_KEY));
    c.msg(format!("\u{a7}3     {}: {}", lang::T_COMMAND_INFO_STATIC_CONTEXTS_KEY, ctx_bracketed(&c.rt.static_contexts(), &none_white)));
    c.msg(format!(
        "\u{a7}3     {}: \u{a7}f{online} \u{a7}7({})",
        lang::T_COMMAND_INFO_ONLINE_PLAYERS_KEY,
        tr(lang::T_COMMAND_INFO_ONLINE_PLAYERS_UNIQUE, '7', &[green(&online.to_string())])
    ));
    c.msg(format!("\u{a7}3     {}: \u{a7}7{}", lang::T_COMMAND_INFO_UPTIME_KEY, low_accuracy(uptime)));
    c.msg(format!(
        "\u{a7}3     {}: {}",
        lang::T_COMMAND_INFO_LOCAL_DATA_KEY,
        tr(lang::T_COMMAND_INFO_LOCAL_DATA, '7', &[green(&users.to_string()), green(&groups.to_string()), green(&tracks.to_string())])
    ));
}

/// `/lp search [comparison] <permission> [page]`
pub(crate) fn search(c: &mut Ctx<'_>, mut a: Args) {
    let comparison = match a.get(0).as_str() {
        s @ ("==" | "!=" | "~~" | "!~") => s.to_owned(),
        _ => {
            a.v.insert(0, "==".into());
            "==".to_owned()
        }
    };
    let perm = a.get(1);
    let page = a.int_or(2, 1);
    let km = match comparison.as_str() {
        "!=" => KeyMatch::NotEqual(perm.clone()),
        "~~" => KeyMatch::Like(perm.clone()),
        "!~" => KeyMatch::NotLike(perm.clone()),
        _ => KeyMatch::Exact(perm.clone()),
    };
    c.msg(format!("{}...", tr(lang::T_COMMAND_SEARCH_SEARCHING_PERMISSION, 'a', &[aq(&format!("permissions {comparison} {perm}"))])));
    let hits = match c.st.search(&km) {
        Ok(h) => h,
        Err(_) => {
            c.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_ALL_GROUPS);
            return;
        }
    };
    let mut users: Vec<(String, Node)> = Vec::new();
    let mut groups: Vec<(String, Node)> = Vec::new();
    for (h, n) in hits {
        match h {
            HolderKey::User(u) => {
                let nm = c.st.name_of(u).ok().flatten().unwrap_or_else(|| crate::uuid::format(u));
                users.push((nm, n));
            }
            HolderKey::Group(g) => groups.push((g, n)),
        }
    }
    let (nu, ng) = (users.len(), groups.len());
    c.ok(lang::T_COMMAND_SEARCH_RESULT, &[aq(&(nu + ng).to_string()), aq(&nu.to_string()), aq(&ng.to_string())]);
    let show_node = comparison != "==";
    if !users.is_empty() {
        search_results(c, users, lang::T_COMMAND_SEARCH_SHOWING_USERS, page, Some(show_node));
    }
    if !groups.is_empty() {
        search_results(c, groups, lang::T_COMMAND_SEARCH_SHOWING_GROUPS, page, Some(show_node));
    }
}

/// `SearchCommand.sendResult` / `GroupListMembers.sendResult`: pages of 15. `node_mode`: `Some(show_node)` for a
/// permission search, None for a list of inheriting holders.
pub(crate) fn search_results(c: &mut Ctx<'_>, mut results: Vec<(String, Node)>, header: &str, page: i64, node_mode: Option<bool>) {
    results.sort_by(|x, y| x.0.to_lowercase().cmp(&y.0.to_lowercase()).then_with(|| x.1.key.cmp(&y.1.key)));
    let (page, pages, slice) = page_of(&results, 15, page);
    c.msg(format!("\u{a7}b{header}:    {}", fmt::page_info(page, pages, results.len())));
    let now = duration::now();
    for (holder, n) in slice {
        let mut line = format!("\u{a7}3> \u{a7}b{holder}");
        if let Some(show_node) = node_mode {
            if show_node {
                line.push_str(&format!("\u{a7}7 - \u{a7}7({})", n.key));
            }
            line.push_str(&format!("\u{a7}7 - {}{}", if n.value { "\u{a7}a" } else { "\u{a7}c" }, n.value));
        }
        if n.has_expiry() {
            line.push_str(&format!(" \u{a7}8(\u{a7}7{} {}\u{a7}8)", lang::T_COMMAND_GENERIC_INFO_EXPIRES_IN, duration::format_long(n.expiry - now)));
        }
        line.push(' ');
        line.push_str(&ctx_bracketed(&n.contexts, ""));
        c.raw(line);
    }
}

fn group_invalid(c: &mut Ctx<'_>, name: &str) {
    c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID, &[dred(name)]);
}

/// `/lp creategroup <name> [weight] [displayname]`
pub(crate) fn create_group(c: &mut Ctx<'_>, a: Args) {
    let name = a.get(0).to_lowercase();
    if !valid_group_name(&name) {
        return group_invalid(c, &name);
    }
    if c.group_exists(&name) {
        return c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR_ALREADY_EXISTS, &[dred(&name)]);
    }
    let weight = a.priority(1).ok();
    let display_at = if weight.is_some() { 2 } else { 1 };
    let display = if a.len() > display_at { Some(a.get(display_at)) } else { None };
    if c.st.create_group(&name).is_err() {
        return c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR, &[dred(&name)]);
    }
    let mut g = Group::new(&name);
    if let Some(w) = weight {
        g.data.set(Node::permanent(key::weight(w), true, ContextSet::new()));
    }
    if let Some(d) = display {
        g.data.set(Node::permanent(key::display_name(&d), true, ContextSet::new()));
    }
    c.save(Loaded { h: Holder::Group(g), before: Vec::new() });
    c.ok(lang::T_COMMAND_GENERIC_CREATE_SUCCESS, &[aq(&name)]);
    c.log(TargetType::Group, None, &name, &[Part::Text("create")]);
}

/// `/lp deletegroup <name> [--update-parent-lists]`
pub(crate) fn delete_group(c: &mut Ctx<'_>, mut a: Args) {
    let name = a.get(0).to_lowercase();
    if !valid_group_name(&name) {
        return group_invalid(c, &name);
    }
    if name == DEFAULT_GROUP {
        return c.bad(lang::T_COMMAND_GROUP_DELETE_NOT_DEFAULT, &[]);
    }
    let Some(group) = c.model().groups.get(&name).cloned() else {
        return c.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP);
    };
    let shown = Holder::Group(group).arg();
    if c.st.delete_group(&name).is_err() {
        return c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[dred(&shown)]);
    }
    {
        let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.groups.remove(&name);
        m.touch();
    }
    c.out.group_changed = true;
    c.ok(lang::T_COMMAND_GENERIC_DELETE_SUCCESS, &[shown]);
    c.log(TargetType::Group, None, &name, &[Part::Text("delete")]);
    if a.remove_flag("--update-parent-lists") {
        remove_inheritance_of(c, &name);
    }
    c.out.sync = true;
}

/// `--update-parent-lists`: every `group.<name>` node of every user and group goes.
fn remove_inheritance_of(c: &mut Ctx<'_>, name: &str) {
    let k = key::group(name);
    let Ok(hits) = c.st.search(&KeyMatch::Exact(k.clone())) else { return };
    let mut seen: Vec<HolderKey> = Vec::new();
    for (h, _) in hits {
        if seen.contains(&h) {
            continue;
        }
        seen.push(h.clone());
        let loaded = match &h {
            HolderKey::User(u) => c.load_user(*u),
            HolderKey::Group(g) => c.load_group(g),
        };
        let Some(mut l) = loaded else { continue };
        l.h.data_mut().remove_if(|n| n.key == k);
        if let Holder::User(u) = &mut l.h {
            u.give_default_if_needed();
        }
        c.save(l);
    }
}

/// `/lp listgroups [page]`
pub(crate) fn list_groups(c: &mut Ctx<'_>, a: Args) {
    let page = a.int_or(0, 1);
    let tracks: Vec<crate::model::Track> = c.model().tracks.values().cloned().collect();
    let mut groups: Vec<(i32, Group)> = {
        let m = c.model();
        m.groups.values().map(|g| (crate::calc::group_weight(&m, &c.cfg, &g.name), g.clone())).collect()
    };
    groups.sort_by(|x, y| y.0.cmp(&x.0).then_with(|| x.1.name.to_lowercase().cmp(&y.1.name.to_lowercase())));
    let (page, pages, slice) = page_of(&groups, 8, page);
    c.msg(format!("\u{a7}b{}:    {}", lang::T_COMMAND_SEARCH_SHOWING_GROUPS, fmt::page_info(page, pages, groups.len())));
    c.msg(format!("\u{a7}a{}: \u{a7}7(name, weight, tracks)", lang::T_COMMAND_GROUP_LIST_TITLE));
    for (w, g) in slice {
        let on: Vec<String> = tracks.iter().filter(|t| t.groups.contains(&g.name)).map(|t| t.name.clone()).collect();
        let mut line = format!("\u{a7}f-  \u{a7}3{}\u{a7}7 - \u{a7}b{w}", Holder::Group(g.clone()).formatted());
        if !on.is_empty() {
            line.push_str(&format!("\u{a7}7 - {}", fmt::string_list(&on)));
        }
        c.msg(line);
    }
}

// ---- log ----------------------------------------------------------------------------------------

/// `LogParentCommand`
pub(crate) fn log_command(c: &mut Ctx<'_>, a: Args) {
    if a.is_empty() {
        tracks_log_usage(c, "Log", LOG_CHILDREN, "log");
        return;
    }
    let Some(ch) = LOG_CHILDREN.iter().find(|ch| ch.name.eq_ignore_ascii_case(&a.get(0))) else {
        c.not_recognised();
        return;
    };
    if !c.has(&format!("log.{}", ch.node)) {
        c.no_permission();
        return;
    }
    if ch.check.test(a.len() - 1) {
        c.child_detailed(ch);
        return;
    }
    let rest = a.sub(1);
    match ch.name {
        "recent" => recent(c, rest),
        "search" => log_search(c, rest),
        "notify" => notify(c, rest),
        "userhistory" => history(c, rest, "user"),
        "grouphistory" => history(c, rest, "group"),
        _ => history(c, rest, "track"),
    }
}

type Page = (Vec<(usize, Action)>, usize);

/// The page of a log listing: below 1 and past the end are refused, no entries too.
fn log_page(c: &mut Ctx<'_>, filter: &ActionFilter, page: i64) -> Option<Page> {
    if page < 1 {
        invalid_page_range(c, 1);
        return None;
    }
    let all = match c.st.log_entries(filter) {
        Ok(a) => a,
        Err(_) => {
            c.bad(lang::T_COMMAND_LOG_LOAD_ERROR, &[]);
            return None;
        }
    };
    if all.is_empty() {
        c.msg(format!("{}.", tr(lang::T_COMMAND_LOG_EMPTY, 'b', &[])));
        return None;
    }
    let max_page = all.len().div_ceil(10);
    if page as usize > max_page {
        invalid_page_range(c, max_page);
        return None;
    }
    let start = 10 * (page as usize - 1);
    let entries = all.into_iter().skip(start).take(10).enumerate().map(|(i, a)| (start + i + 1, a)).collect();
    Some((entries, max_page))
}

fn invalid_page_range(c: &mut Ctx<'_>, max: usize) {
    c.msg(format!(
        "{}. {}.",
        tr(lang::T_COMMAND_LOG_INVALID_PAGE, 'c', &[]),
        tr(lang::T_COMMAND_LOG_INVALID_PAGE_RANGE, 'c', &[dred("1"), dred(&max.to_string())])
    ));
}

fn log_entries_out(c: &mut Ctx<'_>, entries: &[(usize, Action)]) {
    let now = duration::now();
    for (pos, a) in entries {
        let ago = tr(lang::T_DURATION_SINCE, '7', &[low_accuracy((now - a.timestamp).max(0))]);
        let source = if a.source_name.is_empty() || a.source_name == "null" { crate::uuid::format(a.source_uuid) } else { a.source_name.clone() };
        let target = if a.target_name.is_empty() || a.target_name == "null" {
            a.target_uuid.map(crate::uuid::format).unwrap_or_else(|| a.target_name.clone())
        } else {
            a.target_name.clone()
        };
        c.msg(format!(
            "\u{a7}b#{pos} \u{a7}8({ago}\u{a7}8) \u{a7}8(\u{a7}e{source}\u{a7}8) \u{a7}8[\u{a7}a{}\u{a7}8] \u{a7}8(\u{a7}b{target}\u{a7}8)",
            a.target_type.code()
        ));
        c.msg(format!("\u{a7}7> \u{a7}f{}", a.description));
    }
}

/// `(page 1 of 3)` in gray.
fn pages_text(page: i64, max: usize) -> String {
    format!("\u{a7}7({})", tr(lang::T_COMMAND_MISC_PAGE, '7', &[fmt::white(&page.to_string()), fmt::white(&max.to_string())]))
}

fn recent(c: &mut Ctx<'_>, a: Args) {
    let mut page = 1;
    let mut uuid: Option<u128> = None;
    if !a.is_empty() {
        let n = a.int_or(0, i64::MIN);
        if n != i64::MIN {
            page = n;
        } else {
            let Some(u) = c.find_user(&a.get(0)) else { return };
            uuid = Some(u);
            let n = a.int_or(1, i64::MIN);
            if n != i64::MIN {
                page = n;
            }
        }
    }
    let filter = match uuid {
        Some(u) => ActionFilter::Source(u),
        None => ActionFilter::All,
    };
    let Some((entries, max)) = log_page(c, &filter, page) else { return };
    if uuid.is_some() {
        let mut name = entries.first().map(|(_, a)| a.source_name.clone()).unwrap_or_default();
        if let Some(i) = name.find('@') {
            name.truncate(i);
        }
        c.msg(format!("{}  {}", tr(lang::T_COMMAND_LOG_SHOW_BY, 'a', &[aq(&name)]), pages_text(page, max)));
    } else {
        c.msg(format!("{}  {}", tr(lang::T_COMMAND_LOG_SHOW_RECENT, 'a', &[]), pages_text(page, max)));
    }
    log_entries_out(c, &entries);
}

fn log_search(c: &mut Ctx<'_>, mut a: Args) {
    let mut page = 1;
    if a.len() > 1 {
        if let Ok(p) = a.v[a.len() - 1].parse::<i32>() {
            page = p as i64;
            a.v.pop();
        }
    }
    let query = a.v.join(" ");
    let Some((entries, max)) = log_page(c, &ActionFilter::Search(query.clone()), page) else { return };
    c.msg(format!("{}  {}", tr(lang::T_COMMAND_LOG_SHOW_SEARCH, 'a', &[aq(&query)]), pages_text(page, max)));
    log_entries_out(c, &entries);
}

/// `userhistory`, `grouphistory`, `trackhistory`
fn history(c: &mut Ctx<'_>, a: Args, what: &str) {
    let filter = match what {
        "user" => {
            let Some(u) = c.find_user(&a.get(0)) else { return };
            ActionFilter::User(u)
        }
        "group" => {
            let g = a.get(0).to_lowercase();
            if !valid_group_name(&g) {
                return group_invalid(c, &g);
            }
            ActionFilter::Group(g)
        }
        _ => {
            let t = a.get(0).to_lowercase();
            if !valid_track_name(&t) {
                return c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[dred(&t)]);
            }
            ActionFilter::Track(t)
        }
    };
    let page = a.int_or(1, 1);
    let Some((entries, max)) = log_page(c, &filter, page) else { return };
    let name = entries.first().map(|(_, a)| a.target_name.clone()).unwrap_or_default();
    c.msg(format!("{}  {}", tr(lang::T_COMMAND_LOG_SHOW_HISTORY, 'a', &[what.to_owned(), aq(&name)]), pages_text(page, max)));
    log_entries_out(c, &entries);
}

const IGNORE_NODE: &str = "luckperms.log.notify.ignoring";

/// `/lp log notify [on|off]`
fn notify(c: &mut Ctx<'_>, a: Args) {
    if c.sender.console {
        return c.bad(lang::T_COMMAND_LOG_NOTIFY_ERROR_CONSOLE, &[]);
    }
    let uuid = c.sender.uuid;
    let Some(mut l) = c.load_user(uuid) else { return };
    let ignoring = l.h.data().nodes.iter().any(|n| n.key.eq_ignore_ascii_case(IGNORE_NODE) && n.contexts.is_empty() && n.value);
    let set_ignoring = |c: &mut Ctx<'_>, l: &mut Loaded, state: bool| {
        if state {
            l.h.data_mut().set(Node::permanent(IGNORE_NODE, true, ContextSet::new()));
        } else {
            l.h.data_mut().remove_if(|n| n.key.eq_ignore_ascii_case(IGNORE_NODE) && n.contexts.is_empty());
        }
        let taken = std::mem::replace(l, Loaded { h: Holder::Group(Group::new("_")), before: Vec::new() });
        c.save(taken);
    };
    let on_msg = |c: &mut Ctx<'_>| {
        c.msg(format!("{}.", tr(lang::T_COMMAND_LOG_NOTIFY_CHANGED_STATE, 'b', &[green(lang::T_COMMAND_LOG_NOTIFY_ENABLED_TERM)])));
    };
    let off_msg = |c: &mut Ctx<'_>| {
        c.msg(format!("{}.", tr(lang::T_COMMAND_LOG_NOTIFY_CHANGED_STATE, 'b', &[format!("\u{a7}c{}", lang::T_COMMAND_LOG_NOTIFY_DISABLED_TERM)])));
    };
    if a.is_empty() {
        if ignoring {
            set_ignoring(c, &mut l, false);
            on_msg(c);
        } else {
            set_ignoring(c, &mut l, true);
            off_msg(c);
        }
        return;
    }
    let arg = a.get(0);
    if arg.eq_ignore_ascii_case("on") {
        if !ignoring {
            return c.bad(lang::T_COMMAND_LOG_NOTIFY_ALREADY_ON, &[]);
        }
        set_ignoring(c, &mut l, false);
        on_msg(c);
    } else if arg.eq_ignore_ascii_case("off") {
        if ignoring {
            return c.bad(lang::T_COMMAND_LOG_NOTIFY_ALREADY_OFF, &[]);
        }
        set_ignoring(c, &mut l, true);
        off_msg(c);
    } else {
        c.bad(lang::T_COMMAND_LOG_NOTIFY_INVALID_STATE, &["\"on\"".to_owned(), "\"off\"".to_owned()]);
    }
}
