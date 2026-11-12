//! `/lp track <track> ...`, `/lp listtracks`, `/lp createtrack`, `/lp deletetrack`.

use super::args::Args;
use super::fmt::{self, aq, dred};
use super::tables::TRACK_CHILDREN;
use super::{tracks_log_usage, Ctx, Holder};
use crate::actionlog::{Part, TargetType};
use crate::lang::{self, tr};
use crate::model::Track;
use crate::node::{valid_group_name, valid_track_name};

fn log_track(c: &mut Ctx<'_>, t: &str, parts: &[Part<'_>]) {
    c.log(TargetType::Track, None, t, parts);
}

/// `StorageAssistant.save(track)`
pub(super) fn save_track(c: &mut Ctx<'_>, t: Track) -> bool {
    if c.st.save_track(&t).is_err() {
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_SAVE_ERROR, &[dred(&t.name)]);
        return false;
    }
    let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
    m.tracks.insert(t.name.clone(), t);
    m.touch();
    drop(m);
    c.out.group_changed = true;
    true
}

/// `/lp listtracks`
pub(crate) fn list_tracks(c: &mut Ctx<'_>) {
    let names: Vec<String> = c.model().tracks.keys().cloned().collect();
    c.msg(format!("\u{a7}a{}: {}", lang::T_COMMAND_TRACK_LIST_TITLE, fmt::string_list(&names)));
}

/// `/lp createtrack <name>`
pub(crate) fn create_track(c: &mut Ctx<'_>, a: Args) {
    let name = a.get(0).to_lowercase();
    if !valid_track_name(&name) {
        return c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[dred(&name)]);
    }
    if c.model().tracks.contains_key(&name) {
        return c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR_ALREADY_EXISTS, &[dred(&name)]);
    }
    if save_track(c, Track { name: name.clone(), groups: Vec::new() }) {
        c.ok(lang::T_COMMAND_GENERIC_CREATE_SUCCESS, &[aq(&name)]);
        log_track(c, &name, &[Part::Text("create")]);
    }
}

/// `/lp deletetrack <name>`
pub(crate) fn delete_track(c: &mut Ctx<'_>, a: Args) {
    let name = a.get(0).to_lowercase();
    if !c.model().tracks.contains_key(&name) {
        return c.load_error(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK);
    }
    if c.st.delete_track(&name).is_err() {
        return c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[dred(&name)]);
    }
    {
        let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.tracks.remove(&name);
        m.touch();
    }
    c.out.group_changed = true;
    c.ok(lang::T_COMMAND_GENERIC_DELETE_SUCCESS, &[aq(&name)]);
    log_track(c, &name, &[Part::Text("delete")]);
}

/// `TrackParentCommand`
pub(crate) fn track_command(c: &mut Ctx<'_>, a: Args) {
    if a.len() < 2 {
        tracks_log_usage(c, "Track", TRACK_CHILDREN, "track");
        return;
    }
    let Some(ch) = TRACK_CHILDREN.iter().find(|ch| ch.name.eq_ignore_ascii_case(&a.get(1))) else {
        c.not_recognised();
        return;
    };
    if !c.has(&format!("track.{}", ch.node)) {
        c.no_permission();
        return;
    }
    if ch.check.test(a.len() - 2) {
        c.child_detailed(ch);
        return;
    }
    let Some(t) = c.load_track(&a.get(0)) else { return };
    let rest = a.sub(2);
    run(c, t, ch.name, ch.spec, rest);
}

fn group_arg(c: &mut Ctx<'_>, a: &Args, name: &str, spec: &str) -> Option<String> {
    let g = a.get(0).to_lowercase();
    if !valid_group_name(&g) {
        c.detailed_usage(name, spec);
        return None;
    }
    Some(g)
}

/// A group that exists, with its formatted name, or the "not found" answer.
fn group_formatted(c: &mut Ctx<'_>, name: &str) -> Option<String> {
    let found = c.model().groups.get(name).cloned();
    match found {
        Some(g) => Some(Holder::Group(g).formatted()),
        None => {
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_NOT_FOUND, &[dred(name)]);
            None
        }
    }
}

fn run(c: &mut Ctx<'_>, mut t: Track, name: &str, spec: &str, a: Args) {
    match name {
        "editor" => super::editor::track_editor(c, &t),
        "info" => {
            c.msg(format!("\u{a7}b\u{a7}l>\u{a7}b {}: \u{a7}f{}", lang::T_COMMAND_TRACK_INFO_SHOWING_TRACK, t.name));
            c.msg(format!("\u{a7}7\u{a7}f- \u{a7}7{}: {}", lang::T_COMMAND_TRACK_INFO_PATH_PROPERTY, fmt::track_path(&t.groups)));
        }
        "append" => {
            let Some(g) = group_arg(c, &a, name, spec) else { return };
            let Some(shown) = group_formatted(c, &g) else { return };
            if t.groups.contains(&g) {
                return c.ok(lang::T_COMMAND_TRACK_ALREADY_CONTAINS, &[aq(&t.name), aq(&shown)]);
            }
            t.groups.push(g.clone());
            let tn = t.name.clone();
            let path = t.groups.clone();
            if save_track(c, t) {
                c.ok(lang::T_COMMAND_TRACK_APPEND_SUCCESS, &[aq(&g), aq(&tn)]);
                if path.len() > 1 {
                    c.msg(fmt::track_path_hl(&path, &g));
                }
                log_track(c, &tn, &[Part::Text("append"), Part::Text(&g)]);
            }
        }
        "insert" => {
            let Some(g) = group_arg(c, &a, name, spec) else { return };
            let raw_pos = a.get(1);
            let Ok(pos) = raw_pos.parse::<i32>() else {
                return c.bad(lang::T_COMMAND_TRACK_INSERT_ERROR_NUMBER, &[raw_pos]);
            };
            let Some(shown) = group_formatted(c, &g) else { return };
            if t.groups.contains(&g) {
                return c.ok(lang::T_COMMAND_TRACK_ALREADY_CONTAINS, &[aq(&t.name), aq(&shown)]);
            }
            let idx = pos as i64 - 1;
            if idx < 0 || idx as usize > t.groups.len() {
                let first = tr(lang::T_COMMAND_TRACK_INSERT_ERROR_INVALID_POS, 'c', &[dred(&pos.to_string())]);
                c.msg(format!("{first}. \u{a7}7({})", lang::T_COMMAND_TRACK_INSERT_ERROR_INVALID_POS_REASON));
                return;
            }
            t.groups.insert(idx as usize, g.clone());
            let tn = t.name.clone();
            let path = t.groups.clone();
            if save_track(c, t) {
                c.ok(lang::T_COMMAND_TRACK_INSERT_SUCCESS, &[aq(&g), aq(&tn), aq(&pos.to_string())]);
                if path.len() > 1 {
                    c.msg(fmt::track_path_hl(&path, &g));
                }
                let ps = pos.to_string();
                log_track(c, &tn, &[Part::Text("insert"), Part::Text(&g), Part::Text(&ps)]);
            }
        }
        "remove" => {
            let Some(g) = group_arg(c, &a, name, spec) else { return };
            let Some(i) = t.groups.iter().position(|x| *x == g) else {
                return c.ok(lang::T_COMMAND_TRACK_DOESNT_CONTAIN, &[aq(&t.name), aq(&g)]);
            };
            t.groups.remove(i);
            let tn = t.name.clone();
            let path = t.groups.clone();
            if save_track(c, t) {
                c.ok(lang::T_COMMAND_TRACK_REMOVE_SUCCESS, &[aq(&g), aq(&tn)]);
                if path.len() > 1 {
                    c.msg(fmt::track_path(&path));
                }
                log_track(c, &tn, &[Part::Text("remove"), Part::Text(&g)]);
            }
        }
        "clear" => {
            t.groups.clear();
            let tn = t.name.clone();
            if save_track(c, t) {
                c.ok(lang::T_COMMAND_TRACK_CLEAR, &[aq(&tn)]);
                log_track(c, &tn, &[Part::Text("clear")]);
            }
        }
        "rename" | "clone" => {
            let new = a.get(0).to_lowercase();
            if !valid_track_name(&new) {
                return c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[dred(&new)]);
            }
            let old = t.name.clone();
            if name == "rename" {
                if c.model().tracks.contains_key(&new) {
                    return c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR_ALREADY_EXISTS, &[dred(&new)]);
                }
                if !save_track(c, Track { name: new.clone(), groups: t.groups.clone() }) {
                    return;
                }
                if c.st.delete_track(&old).is_err() {
                    return c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[dred(&old)]);
                }
                c.rt.model.write().unwrap_or_else(|e| e.into_inner()).tracks.remove(&old);
                c.ok(lang::T_COMMAND_GENERIC_RENAME_SUCCESS, &[aq(&old), aq(&new)]);
            } else {
                if !save_track(c, Track { name: new.clone(), groups: t.groups.clone() }) {
                    return;
                }
                c.ok(lang::T_COMMAND_GENERIC_CLONE_SUCCESS, &[aq(&old), aq(&new)]);
            }
            log_track(c, &old, &[Part::Text(name), Part::Text(&new)]);
        }
        _ => c.not_recognised(),
    }
}
