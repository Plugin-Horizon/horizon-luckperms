//! `/lp user <user> ...` and `/lp group <group> ...`: the parent commands and everything under them
//! (`UserParentCommand`, `GroupParentCommand`, the generic `permission`, `parent` and `meta` commands).

use super::args::{ArgErr, Args};
use super::fmt::{self, aq, ctx, ctx_bracketed, dur_aq, white};
use super::tables::{holder_children, parent_subs, Child, Sub};
use super::{child_has, Ctx, Holder, Loaded, LABEL};
use crate::actionlog::Part;
use crate::calc::{self, Tristate};
use crate::context::ContextSet;
use crate::duration::{self, format_concise, format_long};
use crate::lang::{self, tr};
use crate::model::{Group, Mutate, TempMerge};
use crate::node::{key, valid_group_name, valid_track_name, Equality, Kind, Node};
use std::collections::HashSet;

type R = Result<(), ArgErr>;

/// `ParentCommand.execute` for `user` and `group`.
pub(crate) fn holder_command(c: &mut Ctx<'_>, user: bool, a: Args) {
    let tag = if user { "user" } else { "group" };
    let children = holder_children(user);
    if a.len() < 2 {
        let usage = format!("/{LABEL} {tag} <{tag}>");
        let subs: Vec<(&str, &str)> = {
            let has = |n: &str| c.has(n);
            children.iter().filter(|ch| child_has(&has, tag, ch)).map(|ch| (ch.name, ch.spec)).collect()
        };
        c.parent_usage(if user { "User" } else { "Group" }, &usage, &subs);
        return;
    }
    let Some(ch) = children.iter().find(|ch| ch.name.eq_ignore_ascii_case(&a.get(1))) else {
        c.not_recognised();
        return;
    };
    let ok = {
        let has = |n: &str| c.has(n);
        child_has(&has, tag, ch)
    };
    if !ok {
        c.no_permission();
        return;
    }
    if ch.check.test(a.len() - 2) {
        c.child_detailed(ch);
        return;
    }
    // The target.
    let target = a.get(0);
    let loaded = if user {
        let Some(uuid) = c.find_user(&target) else { return };
        c.load_user(uuid)
    } else {
        c.load_group(&target)
    };
    let Some(mut l) = loaded else { return };
    let rest = a.sub(2);
    run_child(c, &mut l, ch, rest);
}

fn run_child(c: &mut Ctx<'_>, l: &mut Loaded, ch: &Child, a: Args) {
    let user = l.h.is_user();
    if !ch.subs.is_empty() {
        return generic_parent(c, l, ch, a);
    }
    let r: R = match ch.name {
        "info" => {
            info(c, l);
            Ok(())
        }
        "editor" => {
            super::editor::holder_editor(c, l);
            Ok(())
        }
        "promote" if user => promote(c, l, a, true),
        "demote" if user => promote(c, l, a, false),
        "showtracks" => {
            show_tracks(c, l);
            Ok(())
        }
        "clear" => clear(c, l, a),
        "clone" => clone(c, l, a),
        "listmembers" => list_members(c, l, a),
        "setweight" => set_weight(c, l, a),
        "setdisplayname" => set_display_name(c, l, a),
        "rename" => rename(c, l, a),
        _ => {
            c.not_recognised();
            Ok(())
        }
    };
    if let Err(e) = r {
        c.arg_err(e, ch.name, ch.spec);
    }
}

/// `GenericParentCommand.execute`: `permission`, `parent` and `meta`.
fn generic_parent(c: &mut Ctx<'_>, l: &mut Loaded, ch: &Child, a: Args) {
    let tag = l.h.tag();
    let subs = parent_subs(ch, l.h.is_user());
    if a.is_empty() {
        let usage = format!("/{LABEL} {tag} <{tag}> {}", ch.name);
        let listed: Vec<(&str, &str)> = subs.iter().filter(|s| c.has(&format!("{tag}.{}", s.node))).map(|s| (s.name, s.spec)).collect();
        c.parent_usage(ch.shown, &usage, &listed);
        return;
    }
    let Some(sub) = subs.iter().find(|s| s.name.eq_ignore_ascii_case(&a.get(0))) else {
        c.not_recognised();
        return;
    };
    if !c.has(&format!("{tag}.{}", sub.node)) {
        c.no_permission();
        return;
    }
    if sub.check.test(a.len() - 1) {
        c.detailed_usage(sub.name, sub.spec);
        return;
    }
    let rest = a.sub(1);
    let r = match ch.name {
        "permission" => permission(c, l, sub, rest),
        "parent" => parent(c, l, sub, rest),
        _ => meta(c, l, sub, rest),
    };
    if let Err(e) = r {
        c.arg_err(e, sub.name, sub.spec);
    }
}

// ---- helpers ------------------------------------------------------------------------------------

fn log_parts(c: &mut Ctx<'_>, l: &Loaded, parts: &[Part<'_>]) {
    let name = l.h.name();
    c.log(l.h.target_type(), l.h.uuid(), &name, parts);
}

fn save(c: &mut Ctx<'_>, l: &mut Loaded) {
    let taken = std::mem::replace(l, Loaded { h: Holder::Group(Group::new("_")), before: Vec::new() });
    c.save(taken);
}

/// `Iterators.divideIterable` pages of `per` entries: (page used, pages, the entries of it).
pub(crate) fn page_of<T>(items: &[T], per: usize, page: i64) -> (usize, usize, &[T]) {
    let pages = items.len().div_ceil(per).max(1);
    let idx = page - 1;
    let used = if idx < 0 || idx as usize >= pages { 1 } else { page as usize };
    let start = ((used - 1) * per).min(items.len());
    let end = (start + per).min(items.len());
    (used, pages, &items[start..end])
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortType {
    Priority,
    Alphabetically,
}

/// `SortMode.determine`: (type, ascending).
fn sort_mode(a: &Args) -> (SortType, bool) {
    let mut ty = SortType::Priority;
    let mut asc = true;
    for arg in &a.v {
        let l = arg.to_lowercase();
        if arg == "!" || l == "reverse" || l == "reversed" {
            asc = false;
        } else if l == "priority" {
            ty = SortType::Priority;
            asc = true;
        } else if l == "!priority" {
            ty = SortType::Priority;
            asc = false;
        } else if l == "alphabetically" || l == "abc" {
            ty = SortType::Alphabetically;
            asc = true;
        } else if l == "!alphabetically" || l == "!abc" {
            ty = SortType::Alphabetically;
            asc = false;
        } else {
            continue;
        }
        break;
    }
    (ty, asc)
}

fn expires_line(n: &Node, now: i64) -> String {
    format!("\u{a7}2-    {} {}", lang::T_COMMAND_GENERIC_INFO_EXPIRES_IN, format_long(n.expiry - now))
}

/// `resolveInheritedNodes(DEFAULT_NON_CONTEXTUAL)`: the holder's own nodes, then those of every group it inherits (any
/// context), each with the name of the group it came from (None for its own).
fn inherited_nodes(c: &Ctx<'_>, l: &Loaded) -> Vec<(Node, Option<String>)> {
    let now = duration::now();
    let m = c.model();
    let mut out: Vec<(Node, Option<String>)> = Vec::new();
    for n in l.h.data().sorted() {
        if !n.has_expired(now) {
            out.push((n.clone(), None));
        }
    }
    let parents = |nodes: Vec<&Node>| -> Vec<String> {
        let mut gs: Vec<String> = Vec::new();
        for n in nodes {
            if let Kind::Inheritance(g) = &n.kind {
                if n.value && !n.has_expired(now) && !gs.contains(g) {
                    gs.push(g.clone());
                }
            }
        }
        gs.sort_by_key(|g| std::cmp::Reverse(calc::group_weight(&m, &c.cfg, g)));
        gs
    };
    let mut visited: HashSet<String> = HashSet::new();
    if let Holder::Group(g) = &l.h {
        visited.insert(g.name.clone());
    }
    let mut stack: Vec<String> = parents(l.h.data().sorted()).into_iter().rev().collect();
    while let Some(g) = stack.pop() {
        if !visited.insert(g.clone()) {
            continue;
        }
        let Some(group) = m.groups.get(&g) else { continue };
        for n in group.data.sorted() {
            if !n.has_expired(now) {
                out.push((n.clone(), Some(g.clone())));
            }
        }
        for p in parents(group.data.sorted()).into_iter().rev() {
            if !visited.contains(&p) {
                stack.push(p);
            }
        }
    }
    out
}

/// Resolves the holder being worked on (it may not be in the model: an offline user).
fn resolve_for(c: &Ctx<'_>, l: &Loaded, contexts: &ContextSet) -> calc::Resolved {
    let mut m = c.model().clone();
    match &l.h {
        Holder::User(u) => {
            m.users.insert(u.uuid, u.clone());
        }
        Holder::Group(g) => {
            m.groups.insert(g.name.clone(), g.clone());
        }
    }
    let op = l.h.uuid().and_then(|u| c.rt.online_player(u)).is_some_and(|p| c.rt.is_op(p.op_level));
    calc::resolve(&m, &c.cfg, &l.h.holder_ref(), contexts, op, duration::now())
}

/// `&f'value&f'` with the value's colour codes applied.
fn value_arg(v: &str) -> String {
    format!("\u{a7}f'{}\u{a7}f'", fmt::colored_value(v))
}

/// `&f'key'`
fn plain_quoted(v: &str) -> String {
    format!("\u{a7}f'{v}'")
}

// ---- info ---------------------------------------------------------------------------------------

fn parents_block(c: &mut Ctx<'_>, l: &Loaded) {
    let now = duration::now();
    let parents: Vec<&Node> = l.h.data().sorted().into_iter().filter(|n| matches!(n.kind, Kind::Inheritance(_)) && n.value).collect();
    let (temp, perm): (Vec<&Node>, Vec<&Node>) = parents.into_iter().partition(|n| n.has_expiry());
    let mut lines: Vec<String> = Vec::new();
    if !perm.is_empty() {
        lines.push(format!("\u{a7}a\u{a7}f- \u{a7}a{}:", lang::T_COMMAND_GENERIC_INFO_PARENT_TITLE));
        for n in perm {
            if let Kind::Inheritance(g) = &n.kind {
                lines.push(format!("\u{a7}3    > \u{a7}f{g} {}", ctx_bracketed(&n.contexts, "")));
            }
        }
    }
    if !temp.is_empty() {
        lines.push(format!("\u{a7}a\u{a7}f- \u{a7}a{}:", lang::T_COMMAND_GENERIC_INFO_PARENT_TEMPORARY_TITLE));
        for n in temp {
            if let Kind::Inheritance(g) = &n.kind {
                lines.push(format!("\u{a7}3    > \u{a7}f{g} {}", ctx_bracketed(&n.contexts, "")));
                lines.push(format!("\u{a7}2    -    {} {}", lang::T_COMMAND_GENERIC_INFO_EXPIRES_IN, format_long(n.expiry - now)));
            }
        }
    }
    for line in lines {
        c.msg(line);
    }
}

/// The prefix, suffix, (primary group) and meta lines of the contextual data.
fn chat_meta_lines(c: &mut Ctx<'_>, r: &calc::Resolved, with_primary: bool) {
    let none = format!("\u{a7}b{}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_NULL_RESULT);
    let quoted = |v: &Option<String>| match v {
        Some(p) => format!("\u{a7}f\"{}\u{a7}f\"", fmt::colored_value(p)),
        None => none.clone(),
    };
    c.msg(format!("\u{a7}3    {}: {}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_PREFIX_KEY, quoted(&r.meta.prefix)));
    c.msg(format!("\u{a7}3    {}: {}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_SUFFIX_KEY, quoted(&r.meta.suffix)));
    if with_primary {
        let pg = r.meta.primary_group.clone().unwrap_or_else(|| "default".into());
        c.msg(format!("\u{a7}3    {}: \u{a7}f{pg}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_PRIMARY_GROUP_KEY));
    }
    let mut entries: Vec<String> = Vec::new();
    for (k, vs) in &r.meta.meta {
        if k == "primarygroup" || k == "weight" {
            continue;
        }
        for v in vs {
            entries.push(format!("\u{a7}8(\u{a7}7{k}\u{a7}7=\u{a7}f{}\u{a7}8)", fmt::colored_value(v)));
        }
    }
    let meta = if entries.is_empty() { none.clone() } else { entries.join(" ") };
    c.msg(format!("\u{a7}3    {}: {meta}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_META_KEY));
}

fn info(c: &mut Ctx<'_>, l: &Loaded) {
    match &l.h {
        Holder::User(u) => {
            let online = c.rt.online_player(u.uuid).is_some();
            let (ty_color, ty_name) = match (u.uuid >> 76) & 0xF {
                4 => ("2", lang::T_COMMAND_USER_INFO_UUID_TYPE_MOJANG),
                3 => ("8", lang::T_COMMAND_USER_INFO_UUID_TYPE_NOT_MOJANG),
                2 => ("6", lang::T_COMMAND_USER_INFO_UUID_TYPE_NPC),
                _ => ("c", lang::T_COMMAND_USER_INFO_UUID_TYPE_UNKNOWN),
            };
            c.msg(format!(
                "\u{a7}b\u{a7}l>\u{a7}b {}: \u{a7}f{}",
                lang::T_COMMAND_USER_INFO_TITLE,
                u.username.clone().unwrap_or_else(|| "Unknown".into())
            ));
            c.msg(format!("\u{a7}3\u{a7}f- \u{a7}3{}: \u{a7}f{}", lang::T_COMMAND_USER_INFO_UUID_KEY, crate::uuid::format(u.uuid)));
            c.msg(format!("\u{a7}7    ({}: \u{a7}{ty_color}{ty_name}\u{a7}7)", lang::T_COMMAND_USER_INFO_UUID_TYPE_KEY));
            let status = if online { format!("\u{a7}a{}", lang::T_COMMAND_USER_INFO_STATUS_ONLINE) } else { format!("\u{a7}c{}", lang::T_COMMAND_USER_INFO_STATUS_OFFLINE) };
            c.msg(format!("\u{a7}3\u{a7}f- \u{a7}3{}: {status}", lang::T_COMMAND_USER_INFO_STATUS_KEY));
            parents_block(c, l);
            let active = c.rt.online_player(u.uuid).map(|p| c.rt.contexts_of(&p));
            let ctxs = active.clone().unwrap_or_else(|| c.rt.static_contexts());
            let r = resolve_for(c, l, &ctxs);
            let mode = if active.is_some() {
                format!("\u{a7}2{}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_ACTIVE_PLAYER)
            } else {
                format!("\u{a7}8{}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_SERVER)
            };
            c.msg(format!(
                "\u{a7}f- \u{a7}a{}\u{a7}a: \u{a7}7({}: {mode}\u{a7}7)",
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_TITLE,
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_KEY
            ));
            c.msg(format!(
                "\u{a7}3    {}: {}",
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_CONTEXTS_KEY,
                ctx_bracketed(&ctxs, &format!("\u{a7}b{}", lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_NULL_RESULT))
            ));
            chat_meta_lines(c, &r, true);
        }
        Holder::Group(g) => {
            let weight = g.weight().or_else(|| c.cfg.group_weight.get(&g.name).copied());
            c.msg(format!("\u{a7}b\u{a7}l>\u{a7}b {}: \u{a7}f{}", lang::T_COMMAND_GROUP_INFO_TITLE, g.name));
            c.msg(format!(
                "\u{a7}3\u{a7}f- \u{a7}3{}: \u{a7}f{}",
                lang::T_COMMAND_GROUP_INFO_DISPLAY_NAME_KEY,
                g.display_name().unwrap_or_else(|| g.name.clone())
            ));
            let w = weight.map(|w| w.to_string()).unwrap_or_else(|| lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_NULL_RESULT.to_owned());
            c.msg(format!("\u{a7}3\u{a7}f- \u{a7}3{}: \u{a7}f{w}", lang::T_COMMAND_GROUP_INFO_WEIGHT_KEY));
            parents_block(c, l);
            let ctxs = c.rt.static_contexts();
            let r = resolve_for(c, l, &ctxs);
            c.msg(format!(
                "\u{a7}f- \u{a7}a{}\u{a7}a: \u{a7}7({}: \u{a7}8{}\u{a7}7)",
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_TITLE,
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_KEY,
                lang::T_COMMAND_GENERIC_CONTEXTUAL_DATA_MODE_SERVER
            ));
            chat_meta_lines(c, &r, false);
        }
    }
}

// ---- permission ---------------------------------------------------------------------------------

/// The outcome of removing a temporary node, possibly only shortening it.
enum Unset {
    Removed,
    Shortened(Node),
    Lacks,
}

/// `PermissionHolder.unsetNode(node, duration)` for a temporary `probe`.
fn unset_temp_node(l: &mut Loaded, probe: &Node, dur: Option<i64>, now: i64) -> Unset {
    if let Some(d) = dur {
        let data = l.h.data_mut();
        if let Some(i) = data.nodes.iter().position(|n| n.has_expiry() && n.key == probe.key && n.contexts == probe.contexts) {
            let new_expiry = data.nodes[i].expiry - d;
            if new_expiry > now {
                data.nodes[i].expiry = new_expiry;
                return Unset::Shortened(data.nodes[i].clone());
            }
        }
    }
    match l.h.data_mut().unset(probe) {
        Mutate::Success => Unset::Removed,
        _ => Unset::Lacks,
    }
}

fn empty_permission(c: &mut Ctx<'_>) {
    c.msg(format!("{}.", tr(lang::T_COMMAND_MISC_INVALID_INPUT_EMPTY_PERMISSION, 'c', &[])));
}

fn permission(c: &mut Ctx<'_>, l: &mut Loaded, sub: &Sub, mut a: Args) -> R {
    let now = duration::now();
    let h = l.h.arg();
    match sub.name {
        "info" => {
            let page = a.int_or(0, 1);
            let (ty, asc) = sort_mode(&a);
            let mut nodes: Vec<Node> = l
                .h
                .data()
                .sorted()
                .into_iter()
                .filter(|n| !(matches!(&n.kind, Kind::Inheritance(g) if n.value && c.group_exists(g)) || n.kind.is_meta_or_chat_meta()))
                .cloned()
                .collect();
            if nodes.is_empty() {
                c.ok(lang::T_COMMAND_GENERIC_PERMISSION_INFO_EMPTY, &[h]);
                return Ok(());
            }
            if ty == SortType::Alphabetically {
                nodes.sort_by(|x, y| x.key.cmp(&y.key));
            }
            if !asc {
                nodes.reverse();
            }
            let (page, pages, slice) = page_of(&nodes, 19, page);
            c.msg(format!("{}:  {}", tr(lang::T_COMMAND_GENERIC_PERMISSION_INFO_TITLE, 'b', &[h]), fmt::page_info(page, pages, nodes.len())));
            for n in slice {
                c.raw(format!("\u{a7}3> {}{} {}", if n.value { "\u{a7}a" } else { "\u{a7}c" }, n.key, ctx_bracketed(&n.contexts, "")));
                if n.has_expiry() {
                    c.raw(expires_line(n, now));
                }
            }
        }
        "set" => {
            let node = a.get(0);
            let value = a.bool_or_insert(1, true);
            let cx = a.contexts(2);
            if node.is_empty() {
                empty_permission(c);
                return Ok(());
            }
            let built = Node::permanent(node.as_str(), value, cx.clone());
            match l.h.data_mut().set(built) {
                Mutate::Success => {
                    c.ok(lang::T_COMMAND_GENERIC_PERMISSION_SET, &[aq(&node), fmt::bool_aq(value), h, ctx(&cx)]);
                    let v = value.to_string();
                    log_parts(c, l, &[Part::Text("permission"), Part::Text("set"), Part::Text(&node), Part::Text(&v), Part::Contexts(&cx)]);
                    save(c, l);
                }
                _ => c.ok(lang::T_COMMAND_GENERIC_PERMISSION_ALREADY_HAS, &[h, aq(&node), ctx(&cx)]),
            }
        }
        "unset" => {
            let node = a.get(0);
            let cx = a.contexts(1);
            if node.is_empty() {
                empty_permission(c);
                return Ok(());
            }
            let built = Node::permanent(node.as_str(), true, cx.clone());
            match l.h.data_mut().unset(&built) {
                Mutate::Success => {
                    c.ok(lang::T_COMMAND_GENERIC_PERMISSION_UNSET, &[aq(&node), h, ctx(&cx)]);
                    log_parts(c, l, &[Part::Text("permission"), Part::Text("unset"), Part::Text(&node), Part::Contexts(&cx)]);
                    save(c, l);
                }
                _ => c.ok(lang::T_COMMAND_GENERIC_PERMISSION_DOESNT_HAVE, &[h, aq(&node), ctx(&cx)]),
            }
        }
        "settemp" => {
            let node = a.get(0);
            let value = a.bool_or_insert(1, true);
            let dur = a.duration(2, now)?;
            let merge = a.temp_modifier_and_remove(3).unwrap_or(c.cfg.temporary_add_behaviour);
            let cx = a.contexts(3);
            if node.is_empty() {
                empty_permission(c);
                return Ok(());
            }
            let built = Node::new(node.as_str(), value, now + dur, cx.clone());
            let (r, ended) = l.h.data_mut().set_temp(built, merge);
            if r == Mutate::Success {
                let left = (ended.expiry - now).max(0);
                c.ok(lang::T_COMMAND_GENERIC_PERMISSION_SET_TEMP, &[aq(&node), fmt::bool_aq(value), h, dur_aq(left), ctx(&cx)]);
                let v = value.to_string();
                let d = format_concise(left);
                log_parts(c, l, &[Part::Text("permission"), Part::Text("settemp"), Part::Text(&node), Part::Text(&v), Part::Text(&d), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_PERMISSION_ALREADY_HAS_TEMP, &[h, aq(&node), ctx(&cx)]);
            }
        }
        "unsettemp" => {
            let node = a.get(0);
            let dur = a.duration_or_none(1, now)?;
            let from = if dur.is_none() { 1 } else { 2 };
            let cx = a.contexts(from);
            if node.is_empty() {
                empty_permission(c);
                return Ok(());
            }
            let probe = Node::new(node.as_str(), true, now + 10, cx.clone());
            match unset_temp_node(l, &probe, dur, now) {
                Unset::Shortened(m) => {
                    let less = dur.unwrap_or(0);
                    c.ok(
                        lang::T_COMMAND_GENERIC_PERMISSION_SUBTRACT,
                        &[aq(&m.key), fmt::bool_aq(m.value), h, dur_aq((m.expiry - now).max(0)), ctx(&cx), dur_aq(less)],
                    );
                    let d = format_concise(less);
                    log_parts(c, l, &[Part::Text("permission"), Part::Text("unsettemp"), Part::Text(&node), Part::Text(&d), Part::Contexts(&cx)]);
                    save(c, l);
                }
                Unset::Removed => {
                    c.ok(lang::T_COMMAND_GENERIC_PERMISSION_UNSET_TEMP, &[aq(&node), h, ctx(&cx)]);
                    log_parts(c, l, &[Part::Text("permission"), Part::Text("unsettemp"), Part::Text(&node), Part::Contexts(&cx)]);
                    save(c, l);
                }
                Unset::Lacks => c.ok(lang::T_COMMAND_GENERIC_PERMISSION_DOESNT_HAVE_TEMP, &[h, aq(&node), ctx(&cx)]),
            }
        }
        "check" => check(c, l, &a.get(0)),
        "clear" => {
            let before = l.h.data().nodes.len();
            let cx = a.contexts(0);
            let all = cx.is_empty();
            l.h.data_mut().remove_if(|n| matches!(n.kind, Kind::Permission | Kind::Regex(_)) && (all || n.contexts == cx));
            let removed = before - l.h.data().nodes.len();
            clear_success(c, lang::T_COMMAND_GENERIC_PERMISSION_CLEAR, &[h], &cx, removed);
            log_parts(c, l, &[Part::Text("permission"), Part::Text("clear"), Part::Contexts(&cx)]);
            save(c, l);
        }
        _ => c.not_recognised(),
    }
    Ok(())
}

/// `CLEAR_SUCCESS` and its kin: `<template>. (N nodes were removed.)`
fn clear_success(c: &mut Ctx<'_>, template: &str, first: &[String], cx: &ContextSet, removed: usize) {
    let mut args: Vec<String> = first.to_vec();
    args.push(ctx(cx));
    let key = if removed == 1 { lang::T_COMMAND_GENERIC_CLEAR_NODE_REMOVED_SINGULAR } else { lang::T_COMMAND_GENERIC_CLEAR_NODE_REMOVED };
    let count = tr(key, 'b', &[removed.to_string()]);
    c.msg(format!("{}. \u{a7}a({count}.\u{a7}a)", tr(template, 'a', &args)));
}

/// `PermissionCheck`
fn check(c: &mut Ctx<'_>, l: &mut Loaded, node: &str) {
    if node.is_empty() {
        empty_permission(c);
        return;
    }
    let h = l.h.arg();
    let nodes = inherited_nodes(c, l);
    let mut own: Vec<&Node> = Vec::new();
    let mut inherited: Vec<(&Node, &str)> = Vec::new();
    let mut wildcards: Vec<(&Node, Option<&str>)> = Vec::new();
    for (n, origin) in &nodes {
        if check_matches(c, node, n) {
            match origin {
                None => own.push(n),
                Some(o) => inherited.push((n, o.as_str())),
            }
        }
        if check_matches_wildcard(c, node, n) {
            wildcards.push((n, origin.as_deref()));
        }
    }
    c.msg(format!("{}:", tr(lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_TITLE, 'a', &[aq(node)])));
    let directly = |c: &mut Ctx<'_>, n: &Node| {
        let t = tr(
            lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_DIRECTLY,
            '7',
            &[h.clone(), aq(&n.key), fmt::tristate(Tristate::of(n.value)), ctx(&n.contexts)],
        );
        c.msg(format!("\u{a7}f- {t}."));
    };
    let inherits = |c: &mut Ctx<'_>, n: &Node, o: &str| {
        let t = tr(
            lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_INHERITED,
            '7',
            &[h.clone(), aq(&n.key), fmt::tristate(Tristate::of(n.value)), format!("\u{a7}a{o}"), ctx(&n.contexts)],
        );
        c.msg(format!("\u{a7}f- {t}."));
    };
    if own.is_empty() {
        let t = tr(lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_NOT_DIRECTLY, '7', &[h.clone(), aq(node)]);
        c.msg(format!("\u{a7}f- {t}."));
    } else {
        for n in own {
            directly(c, n);
        }
    }
    if inherited.is_empty() {
        let t = tr(lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_NOT_INHERITED, '7', &[h.clone(), aq(node)]);
        c.msg(format!("\u{a7}f- {t}."));
    } else {
        for (n, o) in inherited {
            inherits(c, n, o);
        }
    }
    for (n, o) in wildcards {
        match o {
            Some(o) => inherits(c, n, o),
            None => directly(c, n),
        }
    }
    c.msg("");
    // The decision in the holder's own contexts.
    let cx = c.default_check_contexts(&l.h);
    let r = resolve_for(c, l, &cx);
    let res = r.check(node);
    let processor = match res.processor {
        "direct" => Some("common.DirectProcessor"),
        "regex" => Some("common.RegexProcessor"),
        "wildcard" => Some("common.WildcardProcessor"),
        "op" => Some("bukkit.OpProcessor"),
        _ => None,
    };
    c.msg(format!("{}:", tr(lang::T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_TITLE, 'a', &[aq(node)])));
    c.msg(format!("\u{a7}3    {}: {}", lang::T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_RESULT_KEY, fmt::tristate(res.result)));
    let none = format!("\u{a7}b{}", lang::T_COMMAND_MISC_NONE);
    c.msg(format!(
        "\u{a7}3    {}: {}",
        lang::T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_PROCESSOR_KEY,
        processor.map(white).unwrap_or_else(|| none.clone())
    ));
    let cause = res.node.as_ref().and_then(|k| r.permissions.get(&k.to_lowercase()).map(|o| (k.clone(), o.clone())));
    let cause_text = match cause {
        Some((k, o)) => {
            tr(lang::T_COMMAND_GENERIC_PERMISSION_CHECK_INFO_DIRECTLY, '7', &[aq(&o.holder), aq(&k), fmt::bool_gr(o.value), ctx(&o.contexts)])
        }
        None => none.clone(),
    };
    c.msg(format!("\u{a7}3    {}: {cause_text}", lang::T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_CAUSE_KEY));
    c.msg(format!("\u{a7}3    {}: {}", lang::T_COMMAND_GENERIC_PERMISSION_CHECK_RESULT_CONTEXT_KEY, ctx_bracketed(&cx, &none)));
}

fn check_matches(c: &Ctx<'_>, permission: &str, node: &Node) -> bool {
    if node.key == permission {
        return true;
    }
    if c.cfg.apply_shorthand && crate::node::expand_shorthand(&node.key).iter().any(|s| s == permission) {
        return true;
    }
    if c.cfg.apply_regex {
        if let Kind::Regex(p) = &node.kind {
            if let Ok(re) = regex::Regex::new(&format!("^(?:{p})$")) {
                return re.is_match(permission);
            }
        }
    }
    false
}

fn check_matches_wildcard(c: &Ctx<'_>, permission: &str, node: &Node) -> bool {
    if c.cfg.apply_wildcards && node.is_wildcard() {
        if node.key == "*" || node.key == "'*'" {
            return true;
        }
        let body = &node.key[..node.key.len() - 1];
        if permission.starts_with(body) {
            return true;
        }
    }
    false
}

// ---- parent -------------------------------------------------------------------------------------

/// `args.getLowercase(i, GROUP_NAME_TEST[_ALLOW_SPACE])`
fn group_name_arg(a: &Args, i: usize, allow_space: bool) -> Result<String, ArgErr> {
    let s = a.get(i).to_lowercase();
    let ok = if allow_space { !s.is_empty() && s.chars().count() <= crate::node::MAX_GROUP_NAME_LENGTH } else { valid_group_name(&s) };
    if ok {
        Ok(s)
    } else {
        Err(ArgErr::Usage)
    }
}

fn group_arg_formatted(c: &Ctx<'_>, name: &str) -> String {
    let m = c.model();
    match m.groups.get(name) {
        Some(g) => aq(&Holder::Group(g.clone()).formatted()),
        None => aq(name),
    }
}

fn existing_group(c: &mut Ctx<'_>, name: &str) -> Option<String> {
    let found = c.model().group_by_name_or_display(name).map(|g| g.name.clone());
    if found.is_none() {
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_NOT_FOUND, &[fmt::dred(name)]);
    }
    found
}

/// A user left without a group gets `default` back (`UserManager.giveDefaultIfNeeded`).
fn give_default(l: &mut Loaded) {
    if let Holder::User(u) = &mut l.h {
        let inherits_primary = u.data.inheritance().any(|(n, g)| g == u.primary_group && n.contexts.is_empty() && n.value);
        if !inherits_primary {
            u.primary_group = crate::model::DEFAULT_GROUP.into();
        }
        u.give_default_if_needed();
    }
}

fn parent(c: &mut Ctx<'_>, l: &mut Loaded, sub: &Sub, mut a: Args) -> R {
    let now = duration::now();
    let h = l.h.arg();
    match sub.name {
        "info" => {
            let page = a.int_or(0, 1);
            let (ty, asc) = sort_mode(&a);
            let mut nodes: Vec<(String, Node)> = l
                .h
                .data()
                .sorted()
                .into_iter()
                .filter_map(|n| match &n.kind {
                    Kind::Inheritance(g) if n.value => Some((g.clone(), n.clone())),
                    _ => None,
                })
                .collect();
            if nodes.is_empty() {
                c.ok(lang::T_COMMAND_GENERIC_PARENT_INFO_EMPTY, &[h]);
                return Ok(());
            }
            if ty == SortType::Alphabetically {
                nodes.sort_by(|x, y| x.0.cmp(&y.0));
            }
            if !asc {
                nodes.reverse();
            }
            let (page, pages, slice) = page_of(&nodes, 19, page);
            c.msg(format!("{}:  {}", tr(lang::T_COMMAND_GENERIC_PARENT_INFO_TITLE, 'b', &[h]), fmt::page_info(page, pages, nodes.len())));
            for (g, n) in slice {
                c.raw(format!("\u{a7}3> \u{a7}a{g} {}", ctx_bracketed(&n.contexts, "")));
                if n.has_expiry() {
                    c.raw(expires_line(n, now));
                }
            }
        }
        "set" => {
            let name = group_name_arg(&a, 0, false)?;
            let cx = a.contexts(1);
            let Some(g) = existing_group(c, &name) else { return Ok(()) };
            l.h.data_mut().remove_if(|n| matches!(n.kind, Kind::Inheritance(_)) && n.contexts == cx);
            l.h.data_mut().set(Node::permanent(key::group(&g), true, cx.clone()));
            if let Holder::User(u) = &mut l.h {
                u.primary_group = g.clone();
            }
            let gf = group_arg_formatted(c, &g);
            c.ok(lang::T_COMMAND_GENERIC_PARENT_SET, &[h, gf, ctx(&cx)]);
            log_parts(c, l, &[Part::Text("parent"), Part::Text("set"), Part::Text(&g), Part::Contexts(&cx)]);
            save(c, l);
        }
        "add" => {
            let name = group_name_arg(&a, 0, false)?;
            let cx = a.contexts(1);
            let Some(g) = existing_group(c, &name) else { return Ok(()) };
            let gf = group_arg_formatted(c, &g);
            // A group always "has" itself as a parent.
            let itself = matches!(&l.h, Holder::Group(me) if me.name == g);
            let r = if itself { Mutate::AlreadyHas } else { l.h.data_mut().set(Node::permanent(key::group(&g), true, cx.clone())) };
            if r == Mutate::Success {
                c.ok(lang::T_COMMAND_GENERIC_PARENT_ADD, &[h, gf, ctx(&cx)]);
                log_parts(c, l, &[Part::Text("parent"), Part::Text("add"), Part::Text(&g), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_PARENT_ALREADY_INHERITS, &[h, gf, ctx(&cx)]);
            }
        }
        "remove" => {
            let name = group_name_arg(&a, 0, true)?;
            let cx = a.contexts(1);
            if let Holder::User(u) = &l.h {
                let stored = c.cfg.primary_group_calculation.eq_ignore_ascii_case("stored");
                if c.cfg.prevent_primary_group_removal && cx.is_empty() && stored && u.primary_group.eq_ignore_ascii_case(&name) {
                    c.ok(lang::T_COMMAND_USER_REMOVEGROUP_ERROR_PRIMARY, &[]);
                    return Ok(());
                }
            }
            match l.h.data_mut().unset(&Node::permanent(key::group(&name), true, cx.clone())) {
                Mutate::Success => {
                    c.ok(lang::T_COMMAND_GENERIC_PARENT_REMOVE, &[h, aq(&name), ctx(&cx)]);
                    log_parts(c, l, &[Part::Text("parent"), Part::Text("remove"), Part::Text(&name), Part::Contexts(&cx)]);
                    give_default(l);
                    save(c, l);
                }
                _ => c.ok(lang::T_COMMAND_GENERIC_PARENT_DOESNT_INHERIT, &[h, aq(&name), ctx(&cx)]),
            }
        }
        "addtemp" => {
            let name = group_name_arg(&a, 0, false)?;
            let dur = a.duration(1, now)?;
            let merge = a.temp_modifier_and_remove(2).unwrap_or(c.cfg.temporary_add_behaviour);
            let cx = a.contexts(2);
            let Some(g) = existing_group(c, &name) else { return Ok(()) };
            let gf = group_arg_formatted(c, &g);
            if matches!(&l.h, Holder::Group(me) if me.name == g) {
                c.ok(lang::T_COMMAND_GENERIC_PARENT_ALREADY_TEMP_INHERITS, &[h, gf, ctx(&cx)]);
                return Ok(());
            }
            let (r, ended) = l.h.data_mut().set_temp(Node::new(key::group(&g), true, now + dur, cx.clone()), merge);
            if r == Mutate::Success {
                let left = (ended.expiry - now).max(0);
                c.ok(lang::T_COMMAND_GENERIC_PARENT_ADD_TEMP, &[h, gf, dur_aq(left), ctx(&cx)]);
                let d = format_concise(left);
                log_parts(c, l, &[Part::Text("parent"), Part::Text("addtemp"), Part::Text(&g), Part::Text(&d), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_PARENT_ALREADY_TEMP_INHERITS, &[h, gf, ctx(&cx)]);
            }
        }
        "removetemp" => {
            let name = group_name_arg(&a, 0, true)?;
            let dur = a.duration_or_none(1, now)?;
            let from = if dur.is_none() { 1 } else { 2 };
            let cx = a.contexts(from);
            let probe = Node::new(key::group(&name), true, now + 10, cx.clone());
            match unset_temp_node(l, &probe, dur, now) {
                Unset::Shortened(m) => {
                    let less = dur.unwrap_or(0);
                    c.ok(lang::T_COMMAND_GENERIC_PARENT_SUBTRACT, &[h, aq(&name), dur_aq((m.expiry - now).max(0)), ctx(&cx), dur_aq(less)]);
                    let d = format_concise(less);
                    log_parts(c, l, &[Part::Text("parent"), Part::Text("removetemp"), Part::Text(&name), Part::Text(&d), Part::Contexts(&cx)]);
                    save(c, l);
                }
                Unset::Removed => {
                    c.ok(lang::T_COMMAND_GENERIC_PARENT_REMOVE_TEMP, &[h, aq(&name), ctx(&cx)]);
                    log_parts(c, l, &[Part::Text("parent"), Part::Text("removetemp"), Part::Text(&name), Part::Contexts(&cx)]);
                    save(c, l);
                }
                Unset::Lacks => c.ok(lang::T_COMMAND_GENERIC_PARENT_DOESNT_TEMP_INHERIT, &[h, aq(&name), ctx(&cx)]),
            }
        }
        "settrack" => {
            let track_name = a.get(0).to_lowercase();
            if !valid_track_name(&track_name) {
                c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[fmt::dred(&track_name)]);
                return Ok(());
            }
            let Some(track) = c.load_track(&track_name) else { return Ok(()) };
            if track.groups.len() <= 1 {
                c.bad(lang::T_COMMAND_TRACK_ERROR_EMPTY, &[fmt::dred(&track.name)]);
                return Ok(());
            }
            let index = a.int_or(1, -1);
            let group_name = if index > 0 {
                let i = index as usize;
                if i - 1 >= track.groups.len() {
                    c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR_DOESNT_EXIST, &[fmt::dred(&index.to_string())]);
                    return Ok(());
                }
                track.groups[i - 1].clone()
            } else {
                let g = group_name_arg(&a, 1, false)?;
                if !track.groups.contains(&g) {
                    c.ok(lang::T_COMMAND_TRACK_DOESNT_CONTAIN, &[aq(&track.name), aq(&g)]);
                    return Ok(());
                }
                g
            };
            let cx = a.contexts(2);
            let Some(g) = existing_group(c, &group_name) else { return Ok(()) };
            l.h.data_mut().remove_if(|n| matches!(&n.kind, Kind::Inheritance(x) if track.groups.contains(x)) && n.contexts == cx);
            l.h.data_mut().set(Node::permanent(key::group(&g), true, cx.clone()));
            let gf = group_arg_formatted(c, &g);
            c.ok(lang::T_COMMAND_GENERIC_PARENT_SET_TRACK, &[h, aq(&track.name), gf, ctx(&cx)]);
            log_parts(c, l, &[Part::Text("parent"), Part::Text("settrack"), Part::Text(&track.name), Part::Text(&group_name), Part::Contexts(&cx)]);
            save(c, l);
        }
        "clear" => {
            let before = l.h.data().nodes.len();
            let cx = a.contexts(0);
            let all = cx.is_empty();
            l.h.data_mut().remove_if(|n| matches!(n.kind, Kind::Inheritance(_)) && (all || n.contexts == cx));
            give_default(l);
            let removed = before.saturating_sub(l.h.data().nodes.len());
            clear_success(c, lang::T_COMMAND_GENERIC_PARENT_CLEAR, &[h], &cx, removed);
            log_parts(c, l, &[Part::Text("parent"), Part::Text("clear"), Part::Contexts(&cx)]);
            save(c, l);
        }
        "cleartrack" => {
            let track_name = a.get(0).to_lowercase();
            if !valid_track_name(&track_name) {
                c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[fmt::dred(&track_name)]);
                return Ok(());
            }
            let Some(track) = c.load_track(&track_name) else { return Ok(()) };
            if track.groups.len() <= 1 {
                c.bad(lang::T_COMMAND_TRACK_ERROR_EMPTY, &[fmt::dred(&track.name)]);
                return Ok(());
            }
            let before = l.h.data().nodes.len();
            let cx = a.contexts(1);
            let all = cx.is_empty();
            l.h.data_mut().remove_if(|n| matches!(&n.kind, Kind::Inheritance(x) if track.groups.contains(x)) && (all || n.contexts == cx));
            give_default(l);
            let removed = before.saturating_sub(l.h.data().nodes.len());
            clear_success(c, lang::T_COMMAND_GENERIC_PARENT_CLEAR_TRACK, &[h, aq(&track.name)], &cx, removed);
            log_parts(c, l, &[Part::Text("parent"), Part::Text("cleartrack"), Part::Text(&track.name), Part::Contexts(&cx)]);
            save(c, l);
        }
        "switchprimarygroup" => {
            if !l.h.is_user() {
                c.not_recognised();
                return Ok(());
            }
            let opt = c.cfg.primary_group_calculation.clone();
            if opt != "stored" {
                c.ok(lang::T_COMMAND_USER_PRIMARYGROUP_WARN_OPTION, &[opt]);
            }
            let name = a.get(0).to_lowercase();
            let Some(g) = c.model().groups.get(&name).map(|g| g.name.clone()) else {
                c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR_DOESNT_EXIST, &[fmt::dred(&name)]);
                return Ok(());
            };
            let gf = group_arg_formatted(c, &g);
            let Holder::User(u) = &mut l.h else { return Ok(()) };
            let uname = aq(&u.display());
            if u.primary_group.eq_ignore_ascii_case(&g) {
                c.ok(lang::T_COMMAND_USER_PRIMARYGROUP_ALREADY_HAS, &[uname, gf]);
                return Ok(());
            }
            let node = Node::permanent(key::group(&g), true, ContextSet::new());
            if u.data.has(&node, Equality::IgnoreExpiryTimeAndValue).is_none() {
                c.ok(lang::T_COMMAND_USER_PRIMARYGROUP_NOT_MEMBER, &[uname.clone(), gf.clone()]);
                u.data.set(node);
            }
            u.primary_group = g.clone();
            c.ok(lang::T_COMMAND_USER_PRIMARYGROUP_SET, &[uname, gf]);
            log_parts(c, l, &[Part::Text("parent"), Part::Text("switchprimarygroup"), Part::Text(&g)]);
            save(c, l);
        }
        _ => c.not_recognised(),
    }
    Ok(())
}

// ---- meta ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Chat {
    Prefix,
    Suffix,
}

impl Chat {
    fn name(self) -> &'static str {
        match self {
            Chat::Prefix => "prefix",
            Chat::Suffix => "suffix",
        }
    }
    fn key(self, p: i32, v: &str) -> String {
        match self {
            Chat::Prefix => key::prefix(p, v),
            Chat::Suffix => key::suffix(p, v),
        }
    }
    fn of(self, n: &Node) -> Option<(i32, &str)> {
        match (&n.kind, self) {
            (Kind::Prefix(p, v), Chat::Prefix) | (Kind::Suffix(p, v), Chat::Suffix) => Some((*p, v.as_str())),
            _ => None,
        }
    }
}

/// holder, type, `'value'`, priority, context.
fn chat_args(h: &str, ty: Chat, value: &str, priority: i32, cx: &ContextSet) -> Vec<String> {
    vec![h.to_owned(), ty.name().to_owned(), value_arg(value), aq(&priority.to_string()), ctx(cx)]
}

fn empty_meta_key(c: &mut Ctx<'_>) {
    c.msg(format!("{}.", tr(lang::T_COMMAND_MISC_INVALID_INPUT_EMPTY_META_KEY, 'c', &[])));
}

fn meta(c: &mut Ctx<'_>, l: &mut Loaded, sub: &Sub, mut a: Args) -> R {
    let now = duration::now();
    let h = l.h.arg();
    let name = sub.name;
    match name {
        "info" => meta_info(c, l),
        "set" => {
            let k = a.get(0);
            let v = a.get(1);
            let cx = a.contexts(2);
            if k.is_empty() {
                empty_meta_key(c);
                return Ok(());
            }
            let node = Node::permanent(key::meta(&k, &v), true, cx.clone());
            let exists = l.h.data().nodes.iter().any(|n| n.key == node.key && n.contexts == cx && !n.has_expiry());
            if exists {
                c.ok(lang::T_COMMAND_GENERIC_META_ALREADY_HAS, &[h, plain_quoted(&k), value_arg(&v), ctx(&cx)]);
                return Ok(());
            }
            let lk = k.to_lowercase();
            l.h.data_mut().remove_if(|n| matches!(&n.kind, Kind::Meta(mk, _) if mk.to_lowercase() == lk) && !n.has_expiry() && n.contexts == cx);
            l.h.data_mut().set(node);
            c.ok(lang::T_COMMAND_GENERIC_META_SET, &[plain_quoted(&k), value_arg(&v), h, ctx(&cx)]);
            log_parts(c, l, &[Part::Text("meta"), Part::Text("set"), Part::Text(&k), Part::Text(&v), Part::Contexts(&cx)]);
            save(c, l);
        }
        "unset" | "unsettemp" => {
            let temp = name == "unsettemp";
            let k = a.get(0);
            let cx = a.contexts(1);
            if k.is_empty() {
                empty_meta_key(c);
                return Ok(());
            }
            let lk = k.to_lowercase();
            let removed = l.h.data_mut().remove_if(|n| matches!(&n.kind, Kind::Meta(mk, _) if mk.to_lowercase() == lk) && n.has_expiry() == temp && n.contexts == cx);
            if removed {
                let t = if temp { lang::T_COMMAND_GENERIC_META_UNSET_TEMP } else { lang::T_COMMAND_GENERIC_META_UNSET };
                c.ok(t, &[plain_quoted(&k), h, ctx(&cx)]);
                log_parts(c, l, &[Part::Text("meta"), Part::Text(name), Part::Text(&k), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                let t = if temp { lang::T_COMMAND_GENERIC_META_DOESNT_HAVE_TEMP } else { lang::T_COMMAND_GENERIC_META_DOESNT_HAVE };
                c.ok(t, &[h, plain_quoted(&k), ctx(&cx)]);
            }
        }
        "settemp" => {
            let k = a.get(0);
            let v = a.get(1);
            let dur = a.duration(2, now)?;
            let merge = a.temp_modifier_and_remove(3).unwrap_or(c.cfg.temporary_add_behaviour);
            let cx = a.contexts(3);
            if k.is_empty() {
                empty_meta_key(c);
                return Ok(());
            }
            let node = Node::new(key::meta(&k, &v), true, now + dur, cx.clone());
            let lk = k.to_lowercase();
            l.h.data_mut().remove_if(|n| matches!(&n.kind, Kind::Meta(mk, mv) if mk.to_lowercase() == lk && *mv != v) && n.has_expiry() && n.contexts == cx);
            let (r, ended) = l.h.data_mut().set_temp(node, merge);
            if r == Mutate::Success {
                let left = (ended.expiry - now).max(0);
                c.ok(lang::T_COMMAND_GENERIC_META_SET_TEMP, &[plain_quoted(&k), value_arg(&v), h, dur_aq(left), ctx(&cx)]);
                let d = format_concise(left);
                log_parts(c, l, &[Part::Text("meta"), Part::Text("settemp"), Part::Text(&k), Part::Text(&v), Part::Text(&d), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_META_ALREADY_HAS_TEMP, &[h, plain_quoted(&k), value_arg(&v), ctx(&cx)]);
            }
        }
        "addprefix" | "addsuffix" => {
            let ty = if name == "addprefix" { Chat::Prefix } else { Chat::Suffix };
            let priority = a.priority(0)?;
            let v = a.get(1);
            let cx = a.contexts(2);
            if l.h.data_mut().set(Node::permanent(ty.key(priority, &v), true, cx.clone())) == Mutate::Success {
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ADD, &chat_args(&h, ty, &v, priority, &cx));
                let ps = priority.to_string();
                log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("add{}", ty.name())), Part::Text(&ps), Part::Text(&v), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS, &chat_args(&h, ty, &v, priority, &cx));
            }
        }
        "addtempprefix" | "addtempsuffix" => {
            let ty = if name == "addtempprefix" { Chat::Prefix } else { Chat::Suffix };
            let priority = a.priority(0)?;
            let v = a.get(1);
            let dur = a.duration(2, now)?;
            let merge = a.temp_modifier_and_remove(3).unwrap_or(c.cfg.temporary_add_behaviour);
            let cx = a.contexts(3);
            let (r, ended) = l.h.data_mut().set_temp(Node::new(ty.key(priority, &v), true, now + dur, cx.clone()), merge);
            if r == Mutate::Success {
                let left = (ended.expiry - now).max(0);
                let mut args = chat_args(&h, ty, &v, priority, &cx);
                args.insert(4, dur_aq(left));
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ADD_TEMP, &args);
                let ps = priority.to_string();
                let d = format_concise(left);
                log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("addtemp{}", ty.name())), Part::Text(&ps), Part::Text(&v), Part::Text(&d), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS_TEMP, &chat_args(&h, ty, &v, priority, &cx));
            }
        }
        "setprefix" | "setsuffix" | "settempprefix" | "settempsuffix" => {
            let ty = if name.ends_with("prefix") { Chat::Prefix } else { Chat::Suffix };
            let temp = name.contains("temp");
            let first = a.int_or(0, i64::MIN);
            let (explicit, v, dur, merge, cx) = if first == i64::MIN {
                let v = a.get(0);
                if temp {
                    let dur = a.duration(1, now)?;
                    let merge = a.temp_modifier_and_remove(2).unwrap_or(c.cfg.temporary_add_behaviour);
                    (None, v, Some(dur), merge, a.contexts(2))
                } else {
                    (None, v, None, TempMerge::Deny, a.contexts(1))
                }
            } else {
                if a.len() <= if temp { 2 } else { 1 } {
                    return Err(ArgErr::Usage);
                }
                let v = a.get(1);
                if temp {
                    let dur = a.duration(2, now)?;
                    let merge = a.temp_modifier_and_remove(3).unwrap_or(c.cfg.temporary_add_behaviour);
                    (Some(first as i32), v, Some(dur), merge, a.contexts(3))
                } else {
                    (Some(first as i32), v, None, TempMerge::Deny, a.contexts(2))
                }
            };
            l.h.data_mut().remove_if(|n| ty.of(n).is_some() && n.contexts == cx);
            let priority = match explicit {
                Some(p) => p,
                None => {
                    let r = resolve_for(c, l, &cx);
                    let list = if ty == Chat::Prefix { &r.meta.prefixes } else { &r.meta.suffixes };
                    let mut p = list.iter().map(|(p, _)| *p).max().unwrap_or(0) + 1;
                    if let Holder::Group(g) = &l.h {
                        if let Some(w) = g.weight() {
                            if w > p {
                                p = w;
                            }
                        }
                    }
                    p
                }
            };
            let ps = priority.to_string();
            if let Some(dur) = dur {
                let (r, ended) = l.h.data_mut().set_temp(Node::new(ty.key(priority, &v), true, now + dur, cx.clone()), merge);
                if r == Mutate::Success {
                    let left = (ended.expiry - now).max(0);
                    let mut args = chat_args(&h, ty, &v, priority, &cx);
                    args.insert(4, dur_aq(left));
                    c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ADD_TEMP, &args);
                    let d = format_concise(left);
                    log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("settemp{}", ty.name())), Part::Text(&ps), Part::Text(&v), Part::Text(&d), Part::Contexts(&cx)]);
                    save(c, l);
                } else {
                    c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS_TEMP, &chat_args(&h, ty, &v, priority, &cx));
                }
            } else if l.h.data_mut().set(Node::permanent(ty.key(priority, &v), true, cx.clone())) == Mutate::Success {
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ADD, &chat_args(&h, ty, &v, priority, &cx));
                log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("set{}", ty.name())), Part::Text(&ps), Part::Text(&v), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                c.ok(lang::T_COMMAND_GENERIC_CHAT_META_ALREADY_HAS, &chat_args(&h, ty, &v, priority, &cx));
            }
        }
        "removeprefix" | "removesuffix" | "removetempprefix" | "removetempsuffix" => {
            let ty = if name.ends_with("prefix") { Chat::Prefix } else { Chat::Suffix };
            let temp = name.contains("temp");
            let priority = a.priority(0)?;
            let v = a.get_or(1, "null");
            let cx = a.contexts(2);
            let word = if temp { "removetemp" } else { "remove" };
            let ps = priority.to_string();
            if v.eq_ignore_ascii_case("null") || v == "*" {
                l.h.data_mut().remove_if(|n| ty.of(n).is_some_and(|(p, _)| p == priority) && n.has_expiry() == temp && n.contexts == cx);
                let t = if temp { lang::T_COMMAND_GENERIC_CHAT_META_REMOVE_TEMP_BULK } else { lang::T_COMMAND_GENERIC_CHAT_META_REMOVE_BULK };
                c.ok(t, &[h, format!("{}es", ty.name()), aq(&ps), ctx(&cx)]);
                log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("{word}{}", ty.name())), Part::Text(&ps), Part::Text("*"), Part::Contexts(&cx)]);
                save(c, l);
                return Ok(());
            }
            let probe = if temp { Node::new(ty.key(priority, &v), true, now + 10, cx.clone()) } else { Node::permanent(ty.key(priority, &v), true, cx.clone()) };
            if l.h.data_mut().unset(&probe) == Mutate::Success {
                let t = if temp { lang::T_COMMAND_GENERIC_CHAT_META_REMOVE_TEMP } else { lang::T_COMMAND_GENERIC_CHAT_META_REMOVE };
                let value = if temp { plain_quoted(&v) } else { value_arg(&v) };
                c.ok(t, &[h, ty.name().to_owned(), value, aq(&ps), ctx(&cx)]);
                log_parts(c, l, &[Part::Text("meta"), Part::Owned(format!("{word}{}", ty.name())), Part::Text(&ps), Part::Text(&v), Part::Contexts(&cx)]);
                save(c, l);
            } else {
                let t = if temp { lang::T_COMMAND_GENERIC_CHAT_META_DOESNT_HAVE_TEMP } else { lang::T_COMMAND_GENERIC_CHAT_META_DOESNT_HAVE };
                c.ok(t, &chat_args(&h, ty, &v, priority, &cx));
            }
        }
        "clear" => {
            // `meta clear [type] [context...]`
            let mut ty: Option<&str> = None;
            if !a.is_empty() {
                let id = a.get(0).to_lowercase();
                let t = match id.as_str() {
                    "any" | "all" | "*" => Some("meta_or_chat_meta"),
                    "chat" | "chatmeta" => Some("chat_meta"),
                    "meta" => Some("meta"),
                    "prefix" | "prefixes" => Some("prefix"),
                    "suffix" | "suffixes" => Some("suffix"),
                    _ => None,
                };
                if t.is_some() {
                    a.v.remove(0);
                    ty = t;
                }
            }
            let ty = ty.unwrap_or("meta_or_chat_meta");
            let before = l.h.data().nodes.len();
            let cx = a.contexts(0);
            let all = cx.is_empty();
            l.h.data_mut().remove_if(|n| {
                let matches_type = match &n.kind {
                    Kind::Prefix(..) => matches!(ty, "prefix" | "chat_meta" | "meta_or_chat_meta"),
                    Kind::Suffix(..) => matches!(ty, "suffix" | "chat_meta" | "meta_or_chat_meta"),
                    Kind::Meta(..) => matches!(ty, "meta" | "meta_or_chat_meta"),
                    _ => false,
                };
                matches_type && (all || n.contexts == cx)
            });
            let removed = before - l.h.data().nodes.len();
            clear_success(c, lang::T_COMMAND_GENERIC_META_CLEAR, &[h, aq(ty)], &cx, removed);
            log_parts(c, l, &[Part::Text("meta"), Part::Text("clear"), Part::Contexts(&cx)]);
            save(c, l);
        }
        _ => c.not_recognised(),
    }
    Ok(())
}

type ChatEntry<'a> = (i32, &'a Node, &'a Option<String>);

/// `MetaInfo`
fn meta_info(c: &mut Ctx<'_>, l: &Loaded) {
    let h = l.h.arg();
    let now = duration::now();
    let nodes = inherited_nodes(c, l);
    let mut prefixes: Vec<ChatEntry<'_>> = Vec::new();
    let mut suffixes: Vec<ChatEntry<'_>> = Vec::new();
    let mut metas: Vec<(&Node, &Option<String>)> = Vec::new();
    for (n, o) in &nodes {
        match &n.kind {
            Kind::Prefix(p, _) => prefixes.push((*p, n, o)),
            Kind::Suffix(p, _) => suffixes.push((*p, n, o)),
            Kind::Meta(..) => metas.push((n, o)),
            _ => {}
        }
    }
    // Highest priority first.
    prefixes.sort_by(|x, y| y.0.cmp(&x.0));
    suffixes.sort_by(|x, y| y.0.cmp(&x.0));
    let tail = |n: &Node, o: &Option<String>| -> String {
        let mut s = String::new();
        if n.has_expiry() {
            s.push_str(&format!(
                " \u{a7}8(\u{a7}7{} \u{a7}b{}\u{a7}8)",
                lang::T_COMMAND_GENERIC_INFO_EXPIRES_IN,
                format_concise(n.expiry - now)
            ));
        }
        let origin = o.clone().unwrap_or_else(|| lang::T_COMMAND_GENERIC_INFO_INHERITED_FROM_SELF.to_owned());
        s.push_str(&format!(
            " \u{a7}8(\u{a7}7{} \u{a7}a{origin}\u{a7}8) {}",
            lang::T_COMMAND_GENERIC_INFO_INHERITED_FROM,
            ctx_bracketed(&n.contexts, "")
        ));
        s
    };
    let sections: [(&Vec<ChatEntry<'_>>, &str, &str); 2] = [
        (&prefixes, lang::T_COMMAND_GENERIC_CHAT_META_INFO_NONE_PREFIX, lang::T_COMMAND_GENERIC_CHAT_META_INFO_TITLE_PREFIX),
        (&suffixes, lang::T_COMMAND_GENERIC_CHAT_META_INFO_NONE_SUFFIX, lang::T_COMMAND_GENERIC_CHAT_META_INFO_TITLE_SUFFIX),
    ];
    for (list, none_t, title_t) in sections {
        if list.is_empty() {
            c.msg(format!("{}.", tr(none_t, 'b', std::slice::from_ref(&h))));
        } else {
            c.msg(tr(title_t, 'b', std::slice::from_ref(&h)));
            for (p, n, o) in list {
                let value = match &n.kind {
                    Kind::Prefix(_, v) | Kind::Suffix(_, v) => v.clone(),
                    _ => String::new(),
                };
                c.msg(format!("\u{a7}b-> \u{a7}b{p}\u{a7}f - {}{}", value_arg(&value), tail(n, o)));
            }
        }
    }
    if metas.is_empty() {
        c.msg(format!("{}.", tr(lang::T_COMMAND_GENERIC_META_INFO_NONE, 'b', std::slice::from_ref(&h))));
    } else {
        c.msg(tr(lang::T_COMMAND_GENERIC_META_INFO_TITLE, 'b', std::slice::from_ref(&h)));
        for (n, o) in metas {
            if let Kind::Meta(k, v) = &n.kind {
                c.msg(format!("\u{a7}b-> \u{a7}a{k}\u{a7}f = {}{}", value_arg(v), tail(n, o)));
            }
        }
    }
}

// ---- the rest -----------------------------------------------------------------------------------

fn show_tracks(c: &mut Ctx<'_>, l: &Loaded) {
    let h = l.h.arg();
    let tracks: Vec<crate::model::Track> = c.model().tracks.values().cloned().collect();
    let mut lines: Vec<(String, ContextSet, String)> = Vec::new();
    match &l.h {
        Holder::User(u) => {
            let mut seen: Vec<(String, ContextSet)> = Vec::new();
            for (n, g) in u.data.inheritance() {
                if !n.value || n.has_expiry() {
                    continue;
                }
                if seen.iter().any(|(sg, sc)| sg == g && *sc == n.contexts) {
                    continue;
                }
                seen.push((g.to_owned(), n.contexts.clone()));
                for t in tracks.iter().filter(|t| t.groups.iter().any(|x| x == g)) {
                    lines.push((t.name.clone(), n.contexts.clone(), fmt::track_path_hl(&t.groups, g)));
                }
            }
        }
        Holder::Group(g) => {
            for t in tracks.iter().filter(|t| t.groups.contains(&g.name)) {
                lines.push((t.name.clone(), ContextSet::new(), fmt::track_path_hl(&t.groups, &g.name)));
            }
        }
    }
    if lines.is_empty() {
        c.ok(lang::T_COMMAND_GENERIC_SHOW_TRACKS_EMPTY, &[h]);
        return;
    }
    c.msg(format!("{}:", tr(lang::T_COMMAND_GENERIC_SHOW_TRACKS_TITLE, 'b', &[h])));
    for (name, cx, path) in lines {
        c.raw(format!("\u{a7}3> \u{a7}a{name}: {}", ctx_bracketed(&cx, "")));
        c.raw(format!("\u{a7}7  ({path}\u{a7}7)"));
    }
}

fn clear(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let h = l.h.arg();
    let before = l.h.data().nodes.len();
    let cx = a.contexts(0);
    let all = cx.is_empty();
    l.h.data_mut().remove_if(|n| all || n.contexts == cx);
    give_default(l);
    let removed = before.saturating_sub(l.h.data().nodes.len());
    clear_success(c, lang::T_COMMAND_GENERIC_CLEAR, &[h], &cx, removed);
    log_parts(c, l, &[Part::Text("clear"), Part::Contexts(&cx)]);
    save(c, l);
    Ok(())
}

fn clone(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let from = l.h.arg();
    let nodes = l.h.data().nodes.clone();
    let src = l.h.name();
    if l.h.is_user() {
        let Some(uuid) = c.find_user(&a.get(0)) else { return Ok(()) };
        let Some(mut other) = c.load_user(uuid) else { return Ok(()) };
        other.h.data_mut().nodes = nodes;
        c.ok(lang::T_COMMAND_GENERIC_CLONE_SUCCESS, &[from, other.h.arg()]);
        log_parts(c, &other, &[Part::Text("clone"), Part::Text(&src)]);
        save(c, &mut other);
    } else {
        let new_name = a.get(0).to_lowercase();
        if !valid_group_name(&new_name) {
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID, &[fmt::dred(&new_name)]);
            return Ok(());
        }
        if !c.group_exists(&new_name) && c.st.create_group(&new_name).is_err() {
            c.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP);
            return Ok(());
        }
        let mut g = c.model().groups.get(&new_name).cloned().unwrap_or_else(|| Group::new(&new_name));
        let before = g.data.nodes.clone();
        g.data.nodes = nodes;
        let mut other = Loaded { h: Holder::Group(g), before };
        c.ok(lang::T_COMMAND_GENERIC_CLONE_SUCCESS, &[from, other.h.arg()]);
        log_parts(c, &other, &[Part::Text("clone"), Part::Text(&src)]);
        save(c, &mut other);
    }
    Ok(())
}

fn list_members(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let Holder::Group(g) = &l.h else { return Ok(()) };
    let name = g.name.clone();
    let page = a.int_or(0, 1);
    let cx = a.contexts(1);
    c.msg(format!("{}...", tr(lang::T_COMMAND_SEARCH_SEARCHING_INHERIT, 'a', &[aq(&name)])));
    let hits = match c.st.search(&crate::storage::KeyMatch::Exact(key::group(&name))) {
        Ok(h) => h,
        Err(_) => {
            c.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_ALL_GROUPS);
            return Ok(());
        }
    };
    let wanted = |n: &Node| n.value && (cx.is_empty() || n.contexts.is_satisfied_by(&cx, crate::context::SatisfyMode::AtLeastOneValuePerKey));
    let mut users: Vec<(String, Node)> = Vec::new();
    let mut groups: Vec<(String, Node)> = Vec::new();
    for (h, n) in hits {
        if !wanted(&n) {
            continue;
        }
        match h {
            crate::storage::HolderKey::User(u) => {
                let nm = c.st.name_of(u).ok().flatten().unwrap_or_else(|| crate::uuid::format(u));
                users.push((nm, n));
            }
            crate::storage::HolderKey::Group(gn) => groups.push((gn, n)),
        }
    }
    if name == crate::model::DEFAULT_GROUP {
        let defaults: Vec<String> = c.model().users.values().filter(|u| !u.is_non_default()).map(|u| u.display()).collect();
        for d in defaults {
            users.push((d, Node::permanent(key::group(&name), true, ContextSet::new())));
        }
        c.msg(tr(lang::T_COMMAND_SEARCH_RESULT_DEFAULT_NOTICE, '7', &[]));
    }
    let (nu, ng) = (users.len(), groups.len());
    c.ok(lang::T_COMMAND_SEARCH_RESULT, &[aq(&(nu + ng).to_string()), aq(&nu.to_string()), aq(&ng.to_string())]);
    super::misc::search_results(c, users, lang::T_COMMAND_SEARCH_SHOWING_USERS, page, None);
    super::misc::search_results(c, groups, lang::T_COMMAND_SEARCH_SHOWING_GROUPS, page, None);
    Ok(())
}

fn set_weight(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let weight = a.priority(0)?;
    l.h.data_mut().remove_if(|n| matches!(n.kind, Kind::Weight(_)));
    l.h.data_mut().set(Node::permanent(key::weight(weight), true, ContextSet::new()));
    c.ok(lang::T_COMMAND_GROUP_SETWEIGHT_SET, &[aq(&weight.to_string()), l.h.arg()]);
    let w = weight.to_string();
    log_parts(c, l, &[Part::Text("setweight"), Part::Text(&w)]);
    save(c, l);
    Ok(())
}

fn set_display_name(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let Holder::Group(g) = &l.h else { return Ok(()) };
    let gname = g.name.clone();
    let name = a.get(0);
    let cx = a.contexts(1);
    if name.is_empty() {
        c.msg(format!("{}.", tr(lang::T_COMMAND_MISC_INVALID_INPUT_EMPTY_DISPLAY_NAME, 'c', &[])));
        return Ok(());
    }
    let previous = l.h.data().nodes.iter().find_map(|n| match &n.kind {
        Kind::DisplayName(d) if n.contexts == cx => Some(d.clone()),
        _ => None,
    });
    if previous.is_none() && name == gname {
        c.ok(lang::T_COMMAND_GROUP_SETDISPLAYNAME_DOESNT_HAVE, &[aq(&gname)]);
        return Ok(());
    }
    if previous.as_deref() == Some(name.as_str()) {
        c.ok(lang::T_COMMAND_GROUP_SETDISPLAYNAME_ALREADY_HAS, &[aq(&gname), aq(&name)]);
        return Ok(());
    }
    let existing = c.model().groups.values().find(|x| x.display_name().is_some_and(|d| d.eq_ignore_ascii_case(&name))).map(|x| x.name.clone());
    if let Some(e) = existing {
        if e != gname {
            c.ok(lang::T_COMMAND_GROUP_SETDISPLAYNAME_ALREADY_IN_USE, &[aq(&name), aq(&e)]);
            return Ok(());
        }
    }
    l.h.data_mut().remove_if(|n| matches!(n.kind, Kind::DisplayName(_)) && n.contexts == cx);
    if name == gname {
        c.ok(lang::T_COMMAND_GROUP_SETDISPLAYNAME_REMOVED, &[aq(&gname), ctx(&cx)]);
        log_parts(c, l, &[Part::Text("setdisplayname"), Part::Text(&name), Part::Contexts(&cx)]);
        save(c, l);
        return Ok(());
    }
    l.h.data_mut().set(Node::permanent(key::display_name(&name), true, cx.clone()));
    c.ok(lang::T_COMMAND_GROUP_SETDISPLAYNAME_SET, &[aq(&name), aq(&gname), ctx(&cx)]);
    log_parts(c, l, &[Part::Text("setdisplayname"), Part::Text(&name), Part::Contexts(&cx)]);
    save(c, l);
    Ok(())
}

fn rename(c: &mut Ctx<'_>, l: &mut Loaded, a: Args) -> R {
    let Holder::Group(g) = &l.h else { return Ok(()) };
    let old = g.clone();
    let new_name = a.get(0).to_lowercase();
    if !valid_group_name(&new_name) {
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID, &[fmt::dred(&new_name)]);
        return Ok(());
    }
    if c.group_exists(&new_name) {
        c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR_ALREADY_EXISTS, &[fmt::dred(&new_name)]);
        return Ok(());
    }
    if c.st.create_group(&new_name).is_err() {
        c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR, &[fmt::dred(&new_name)]);
        return Ok(());
    }
    if c.st.delete_group(&old.name).is_err() {
        let shown = Holder::Group(old.clone()).formatted();
        c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[fmt::dred(&shown)]);
        return Ok(());
    }
    {
        let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.groups.remove(&old.name);
        m.touch();
    }
    let mut ng = Group::new(&new_name);
    ng.data.nodes = old.data.nodes.clone();
    let mut other = Loaded { h: Holder::Group(ng), before: Vec::new() };
    c.ok(lang::T_COMMAND_GENERIC_RENAME_SUCCESS, &[Holder::Group(old.clone()).arg(), other.h.arg()]);
    log_parts(c, &other, &[Part::Text("rename"), Part::Text(&new_name)]);
    save(c, &mut other);
    c.out.sync = true;
    Ok(())
}

/// `UserPromote` / `UserDemote`
fn promote(c: &mut Ctx<'_>, l: &mut Loaded, mut a: Args, up: bool) -> R {
    if !l.h.is_user() {
        return Ok(());
    }
    let flag = if up { "--dont-add-to-first" } else { "--dont-remove-from-first" };
    let allow_first = !a.remove_flag(flag);
    if a.is_empty() {
        let names: Vec<String> = c.model().tracks.keys().cloned().collect();
        if names.len() == 1 {
            a.v.push(names[0].clone());
        } else if names.iter().any(|n| n == "default") {
            a.v.push("default".into());
        } else {
            c.bad(lang::T_COMMAND_USER_TRACK_UNSURE_WHICH_TRACK, &[]);
            return Ok(());
        }
    }
    let track_name = a.get(0).to_lowercase();
    if !valid_track_name(&track_name) {
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[fmt::dred(&track_name)]);
        return Ok(());
    }
    let Some(track) = c.load_track(&track_name) else { return Ok(()) };
    if track.groups.len() <= 1 {
        c.bad(lang::T_COMMAND_TRACK_ERROR_EMPTY, &[fmt::dred(&track.name)]);
        return Ok(());
    }
    let hide_progress = a.remove_flag("-s");
    let cx = a.contexts(1);
    let user_arg = l.h.arg();
    let uname = l.h.name();
    let tname = track.name.clone();
    let word = if up { "promote" } else { "demote" };
    let mut nodes: Vec<Node> = Vec::new();
    if let Holder::User(u) = &l.h {
        for (n, g) in u.data.inheritance() {
            if n.value && n.contexts == cx && track.groups.iter().any(|x| x == g) && !nodes.iter().any(|m| m == n) {
                nodes.push(n.clone());
            }
        }
    }
    if nodes.is_empty() {
        if !up {
            c.ok(lang::T_COMMAND_USER_TRACK_ERROR_NOT_CONTAIN_GROUP, &[user_arg, aq(&tname)]);
            return Ok(());
        }
        if !allow_first {
            c.ok(lang::T_COMMAND_USER_PROMOTE_NOT_ON_TRACK, &[user_arg, aq(&tname)]);
            return Ok(());
        }
        let first = track.groups[0].clone();
        if !c.group_exists(&first) {
            malformed(c, &first, true);
            return Ok(());
        }
        l.h.data_mut().set(Node::permanent(key::group(&first), true, cx.clone()));
        c.ok(lang::T_COMMAND_USER_PROMOTE_ADDED_TO_FIRST, &[user_arg, aq(&tname), aq(&first), ctx(&cx)]);
        log_parts(c, l, &[Part::Text("promote"), Part::Text(&tname), Part::Contexts(&cx)]);
        save(c, l);
        return Ok(());
    }
    if nodes.len() != 1 {
        let first = tr(lang::T_COMMAND_TRACK_ERROR_MULTIPLE_GROUPS, 'c', &[fmt::dred(&uname)]);
        c.msg(format!("{first}. {}.", lang::T_COMMAND_TRACK_ERROR_AMBIGUOUS));
        return Ok(());
    }
    let old_node = nodes.remove(0);
    let Kind::Inheritance(old) = old_node.kind.clone() else { return Ok(()) };
    let target = if up { track.next(&old).map(str::to_owned) } else { track.previous(&old).map(str::to_owned) };
    let Some(next) = target else {
        if up {
            c.ok(lang::T_COMMAND_USER_PROMOTE_END_OF_TRACK, &[aq(&tname), user_arg]);
            return Ok(());
        }
        // Demoting from the first group.
        if !allow_first {
            c.ok(lang::T_COMMAND_USER_DEMOTE_END_OF_TRACK_NOT_REMOVED, &[aq(&tname), user_arg]);
            return Ok(());
        }
        l.h.data_mut().unset(&old_node);
        c.ok(lang::T_COMMAND_USER_DEMOTE_END_OF_TRACK, &[aq(&tname), user_arg, aq(&old)]);
        log_parts(c, l, &[Part::Text(word), Part::Text(&tname), Part::Contexts(&cx)]);
        save(c, l);
        return Ok(());
    };
    if !c.group_exists(&next) {
        malformed(c, &next, up);
        return Ok(());
    }
    l.h.data_mut().unset(&old_node);
    l.h.data_mut().set(Node::new(key::group(&next), old_node.value, old_node.expiry, old_node.contexts.clone()));
    if let Holder::User(u) = &mut l.h {
        if cx.is_empty() && u.primary_group.eq_ignore_ascii_case(&old) {
            u.primary_group = next.clone();
        }
    }
    let t = if up { lang::T_COMMAND_USER_PROMOTE_SUCCESS } else { lang::T_COMMAND_USER_DEMOTE_SUCCESS };
    c.ok(t, &[user_arg, aq(&tname), aq(&old), aq(&next), ctx(&cx)]);
    if !hide_progress {
        let path = if up { fmt::track_path_progress(&track.groups, &old, &next, false) } else { fmt::track_path_progress(&track.groups, &next, &old, true) };
        c.msg(path);
    }
    log_parts(c, l, &[Part::Text(word), Part::Text(&tname), Part::Contexts(&cx)]);
    save(c, l);
    Ok(())
}

/// `USER_PROMOTE_ERROR_MALFORMED` / `USER_DEMOTE_ERROR_MALFORMED`
fn malformed(c: &mut Ctx<'_>, group: &str, up: bool) {
    let (t, tail) = if up {
        (lang::T_COMMAND_USER_PROMOTE_NEXT_GROUP_DELETED, lang::T_COMMAND_USER_PROMOTE_UNABLE_TO_PROMOTE)
    } else {
        (lang::T_COMMAND_USER_DEMOTE_PREVIOUS_GROUP_DELETED, lang::T_COMMAND_USER_DEMOTE_UNABLE_TO_DEMOTE)
    };
    let first = tr(t, 'a', &[aq(group)]);
    c.msg(format!("{first}. {tail}."));
    c.msg(format!("{}.", tr(lang::T_COMMAND_USER_TRACK_MISSING_GROUP_ADVICE, 'a', &[])));
}
