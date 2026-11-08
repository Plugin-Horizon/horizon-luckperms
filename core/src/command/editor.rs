//! The web editor: `/lp editor`, `/lp user|group <x> editor`, `/lp track <x> editor`,
//! `/lp applyedits <code>` and `/lp trusteditor`. Ported from `EditorCommand`, `HolderEditor`,
//! `TrackEditor`, `ApplyEditsCommand`, `WebEditorRequest`, `WebEditorSession` and
//! `WebEditorResponse`.
//!
//! Not ported: the live bytesocks connection. The editor then shows the `/lp applyedits <code>`
//! command to copy, which is what LuckPerms does when its socket is unreachable
//! (`/lp trusteditor` has no socket to trust and answers as LuckPerms does then).
//!
//! **Privacy.** The payload is built from typed fields only (names, UUIDs, nodes, tracks,
//! contexts, the uploader's name and UUID). No IP address and no other personal data is read
//! into it, and [`personal_data_in`] refuses the upload if anything in it looks like an IP
//! address (a node key someone typed, say). Downloaded editor data is untrusted: sizes are
//! capped and every name and node string is validated before it touches the model.

use super::args::Args;
use super::fmt::{self, aq, dred};
use super::{Ctx, Holder, Loaded, LABEL};
use crate::actionlog::{Part, TargetType};
use crate::bytebin::{self, HttpError};
use crate::calc;
use crate::context::ContextSet;
use crate::lang::{self, tr};
use crate::model::{Group, Track, User};
use crate::node::{valid_group_name, valid_track_name, Equality, Kind, Node, MAX_PERMISSION_LENGTH};
use crate::storage::{HolderKey, KeyMatch};
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// `WebEditorRequest.MAX_USERS`
pub const MAX_USERS: usize = 500;
/// Caps on what a reply may contain (the real editor sends a handful of entries).
const MAX_CHANGES: usize = 2000;
const MAX_DELETIONS: usize = 2000;
const MAX_NODES_PER_HOLDER: usize = 10_000;
const MAX_CONTEXTS_PER_NODE: usize = 32;
const MAX_TRACK_GROUPS: usize = 1000;
const MAX_KNOWN_PERMISSIONS: usize = 5000;
/// Sessions are remembered this long and at most this many.
const SESSION_TTL: Duration = Duration::from_secs(60 * 60 * 24);
const MAX_SESSIONS: usize = 100;

// ---- the payload ---------------------------------------------------------------------------

/// One user or group sent to the editor.
#[derive(Clone, Debug)]
pub(crate) struct EditorHolder {
    pub key: HolderKey,
    /// `getPlainDisplayName`
    pub display: String,
    pub nodes: Vec<Node>,
}

/// What an editor session was shown: the diff base when its reply comes back.
#[derive(Clone, Debug, Default)]
pub(crate) struct Request {
    pub holders: HashMap<HolderKey, Vec<Node>>,
}

/// `NodeType.name().toLowerCase()`
fn node_type(k: &Kind) -> &'static str {
    match k {
        Kind::Permission => "permission",
        Kind::Regex(_) => "regex_permission",
        Kind::Inheritance(_) => "inheritance",
        Kind::Prefix(..) => "prefix",
        Kind::Suffix(..) => "suffix",
        Kind::Meta(..) => "meta",
        Kind::Weight(_) => "weight",
        Kind::DisplayName(_) => "display_name",
    }
}

fn context_json(c: &ContextSet) -> Value {
    serde_json::from_str(&c.to_json()).unwrap_or_else(|_| json!({}))
}

/// `NodeJsonSerializer.serializeNode`: type, key, value, expiry (when temporary), context (when any).
pub(crate) fn node_json(n: &Node) -> Value {
    let mut o = Map::new();
    o.insert("type".into(), json!(node_type(&n.kind)));
    o.insert("key".into(), json!(n.key));
    o.insert("value".into(), json!(n.value));
    if n.has_expiry() {
        o.insert("expiry".into(), json!(n.expiry));
    }
    if !n.contexts.is_empty() {
        o.insert("context".into(), context_json(&n.contexts));
    }
    Value::Object(o)
}

/// The id the editor knows a holder by: a group's name, a user's UUID.
fn holder_id(k: &HolderKey) -> (&'static str, String) {
    match k {
        HolderKey::User(u) => ("user", crate::uuid::format(*u)),
        HolderKey::Group(g) => ("group", g.clone()),
    }
}

/// `WebEditorRequest.createJsonPayload`. `known_permissions` is the editor's autocomplete list.
pub(crate) fn build_payload(
    holders: &[EditorHolder],
    tracks: &[Track],
    uploader_name: &str,
    uploader_uuid: u128,
    label: &str,
    potential_contexts: &ContextSet,
    now_ms: i64,
    server_known: &[String],
) -> Value {
    let mut potential = potential_contexts.clone();
    let mut known: Vec<String> = Vec::new();
    let holders_json: Vec<Value> = holders
        .iter()
        .map(|h| {
            for n in &h.nodes {
                potential.add_all(&n.contexts);
                if matches!(n.kind, Kind::Permission) && known.len() < MAX_KNOWN_PERMISSIONS && !known.contains(&n.key) {
                    known.push(n.key.clone());
                }
            }
            let (ty, id) = holder_id(&h.key);
            json!({
                "type": ty,
                "id": id,
                "displayName": h.display,
                "nodes": h.nodes.iter().map(node_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    let tracks_json: Vec<Value> =
        tracks.iter().map(|t| json!({"type": "track", "id": t.name, "groups": t.groups})).collect();
    known.extend(server_known.iter().cloned());
    known.sort();
    known.dedup();
    json!({
        "metadata": {
            "commandAlias": label,
            "uploader": {"name": uploader_name, "uuid": crate::uuid::format(uploader_uuid)},
            "time": now_ms,
            "pluginVersion": super::LUCKPERMS_VERSION,
            "platform": "Horizon",
        },
        "permissionHolders": holders_json,
        "tracks": tracks_json,
        "knownPermissions": known,
        "potentialContexts": context_json(&potential),
    })
}

fn request_of(holders: &[EditorHolder]) -> Request {
    Request { holders: holders.iter().map(|h| (h.key.clone(), h.nodes.clone())).collect() }
}

/// Whether the text carries something that looks like an IP address (v4, or a v6 with at least
/// four groups or a `::` between two groups). The privacy rule: those never leave the server.
pub(crate) fn personal_data_in(text: &str) -> bool {
    static RE: OnceLock<(regex::Regex, regex::Regex, regex::Regex)> = OnceLock::new();
    let (v4, v6, v6c) = RE.get_or_init(|| {
        let octet = r"(?:25[0-5]|2[0-4][0-9]|1[0-9][0-9]|[1-9]?[0-9])";
        (
            regex::Regex::new(&format!(r"(?:^|[^0-9]){octet}(?:\.{octet}){{3}}(?:$|[^0-9])")).unwrap(),
            regex::Regex::new(r"(?i)(?:^|[^0-9a-f:])(?:[0-9a-f]{1,4}:){3,7}[0-9a-f]{1,4}(?:$|[^0-9a-f:])").unwrap(),
            regex::Regex::new(r"(?i)(?:^|[^0-9a-f:])(?:[0-9a-f]{1,4}:){1,7}:[0-9a-f]{1,4}(?::[0-9a-f]{1,4}){0,6}(?:$|[^0-9a-f:])").unwrap(),
        )
    });
    v4.is_match(text) || v6.is_match(text) || v6c.is_match(text)
}

// ---- sessions ------------------------------------------------------------------------------

struct Session {
    request: Request,
    completed: bool,
    at: Instant,
}

fn sessions() -> &'static Mutex<HashMap<String, Session>> {
    static S: OnceLock<Mutex<HashMap<String, Session>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn lock_sessions() -> std::sync::MutexGuard<'static, HashMap<String, Session>> {
    sessions().lock().unwrap_or_else(|e| e.into_inner())
}

/// `WebEditorSessionMap.addNewSession`
fn add_session(id: &str, request: Request) {
    let mut s = lock_sessions();
    s.retain(|_, v| v.at.elapsed() < SESSION_TTL);
    while s.len() >= MAX_SESSIONS {
        let Some(oldest) = s.iter().min_by_key(|(_, v)| v.at).map(|(k, _)| k.clone()) else { break };
        s.remove(&oldest);
    }
    s.insert(id.to_owned(), Session { request, completed: false, at: Instant::now() });
}

// ---- opening a session -----------------------------------------------------------------------

/// `EditorCommand.Type`
#[derive(Clone, Copy, PartialEq, Eq)]
enum Type {
    All,
    Online,
    Users,
    Groups,
}

impl Type {
    fn parse(s: &str) -> Option<Type> {
        match s.to_uppercase().as_str() {
            "ALL" => Some(Type::All),
            "ONLINE" => Some(Type::Online),
            "USERS" => Some(Type::Users),
            "GROUPS" => Some(Type::Groups),
            _ => None,
        }
    }
    fn users(self) -> bool {
        self != Type::Groups
    }
    fn groups(self) -> bool {
        matches!(self, Type::All | Type::Online | Type::Groups)
    }
    fn offline(self) -> bool {
        matches!(self, Type::All | Type::Users | Type::Groups)
    }
}

fn group_holder(g: &Group) -> EditorHolder {
    EditorHolder { key: HolderKey::Group(g.name.clone()), display: g.display_name().unwrap_or_else(|| g.name.clone()), nodes: g.data.nodes.clone() }
}

fn user_holder(u: &User) -> EditorHolder {
    EditorHolder { key: HolderKey::User(u.uuid), display: u.display(), nodes: u.data.nodes.clone() }
}

/// `includeMatchingGroups`: by weight (high first), then name.
fn matching_groups(c: &Ctx<'_>, keep: impl Fn(&Group) -> bool) -> Vec<Group> {
    let mut v: Vec<Group> = c.model().groups.values().filter(|g| keep(g)).cloned().collect();
    v.sort_by(|a, b| {
        b.weight().unwrap_or(0).cmp(&a.weight().unwrap_or(0)).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    v
}

/// `includeMatchingUsers`: the loaded users with a node any matcher takes (all when there is
/// none), then offline ones from storage up to [`MAX_USERS`], sorted by the weight of their
/// groups, named users first, then by name.
fn matching_users(c: &mut Ctx<'_>, matchers: &[KeyMatch], include_offline: bool) -> Vec<User> {
    let any = |u: &User| matchers.is_empty() || u.data.nodes.iter().any(|n| matchers.iter().any(|m| m.matches(&n.key)));
    let mut users: Vec<User> = c.model().users.values().filter(|u| any(u)).cloned().collect();
    users.sort_by_key(|u| u.uuid);
    if include_offline && users.len() < MAX_USERS {
        let all = [KeyMatch::Like("%".into())];
        let search: &[KeyMatch] = if matchers.is_empty() { &all } else { matchers };
        for m in search {
            if users.len() >= MAX_USERS {
                break;
            }
            let Ok(hits) = c.st.search(m) else { continue };
            let mut ids: Vec<u128> = hits
                .into_iter()
                .filter_map(|(h, _)| if let HolderKey::User(u) = h { Some(u) } else { None })
                .filter(|u| !users.iter().any(|x| x.uuid == *u))
                .collect();
            ids.sort_unstable();
            ids.dedup();
            ids.truncate(MAX_USERS - users.len());
            for id in ids {
                if let Some(l) = c.load_user(id) {
                    if let Holder::User(u) = l.h {
                        users.push(u);
                    }
                }
            }
        }
    }
    let cfg = c.cfg.clone();
    let m = c.model();
    let weight = |u: &User| {
        u.data.inheritance().filter(|(n, _)| n.value && n.contexts.is_empty()).map(|(_, g)| calc::group_weight(&m, &cfg, g)).max().unwrap_or(0)
    };
    users.sort_by(|a, b| {
        weight(b)
            .cmp(&weight(a))
            .then_with(|| b.username.is_some().cmp(&a.username.is_some()))
            .then_with(|| a.display().to_lowercase().cmp(&b.display().to_lowercase()))
    });
    drop(m);
    users
}

/// `WebEditorSession.open` without the socket: build, check, upload, remember, print the link.
fn open_session(c: &mut Ctx<'_>, holders: Vec<EditorHolder>, tracks: Vec<Track>) {
    c.msg(tr(lang::T_COMMAND_EDITOR_START, '7', &[]));
    let mut potential = c.rt.static_contexts();
    for p in c.rt.online_players() {
        potential.add_all(&c.rt.contexts_of(&p));
    }
    let mut uploader = c.sender.name.clone();
    if c.sender.console && c.cfg.server != "global" {
        uploader = format!("{}@{}", uploader, c.cfg.server);
    }
    let now_ms = crate::duration::now() * 1000;
    let payload = build_payload(&holders, &tracks, &uploader, c.sender.uuid, LABEL, &potential, now_ms, &crate::known::all(&c.cfg));
    let json = payload.to_string();
    if personal_data_in(&json) {
        c.msg("\u{a7}cThe editor session was not opened: the data contains something that looks like an IP address, and those are never uploaded. Remove it from the permissions first.");
        return;
    }
    #[cfg(feature = "net")]
    let body = bytebin::gzip(json.as_bytes());
    #[cfg(not(feature = "net"))]
    let body: Vec<u8> = json.into_bytes();
    if body.len() > bytebin::MAX_UPLOAD {
        c.msg("\u{a7}cThe editor session was not opened: the data is too big. Narrow it with a type or a filter.");
        return;
    }
    let cfg = c.cfg.clone();
    match bytebin::with_client(&cfg.bytebin_url, |b| b.post(&body)) {
        Ok(id) => {
            add_session(&id, request_of(&holders));
            let url = format!("{}{}", cfg.web_editor_url, id);
            c.msg(format!("{}:", tr(lang::T_COMMAND_EDITOR_URL, 'a', &[])));
            c.raw(crate::text::rich(
                &format!("\u{a7}b{url}"),
                Some(crate::text::Click::OpenUrl(url.clone())),
                Some(&format!("\u{a7}7{}", tr(lang::T_COMMAND_EDITOR_URL, '7', &[]))),
            ));
        }
        Err(e) => http_failure(c, e),
    }
}

/// `EDITOR_HTTP_REQUEST_FAILURE` / `EDITOR_HTTP_UNKNOWN_FAILURE`
fn http_failure(c: &mut Ctx<'_>, e: HttpError) {
    match e {
        HttpError::Status(code, message) => c.msg(format!(
            "\u{a7}c{}. ({} \u{a7}4{code}\u{a7}c, {}='{message}')",
            lang::T_COMMAND_EDITOR_UNABLE_TO_COMMUNICATE,
            lang::T_COMMAND_MISC_RESPONSE_CODE_KEY,
            lang::T_COMMAND_MISC_ERROR_MESSAGE_KEY
        )),
        HttpError::Other(why) => {
            eprintln!("[LuckPerms] Error talking to bytebin: {why}");
            c.msg(format!("\u{a7}c{}. {}.", lang::T_COMMAND_EDITOR_UNABLE_TO_COMMUNICATE, lang::T_COMMAND_MISC_CHECK_CONSOLE_FOR_ERRORS));
        }
    }
}

/// `/lp editor [type] [filter]`
pub(crate) fn editor(c: &mut Ctx<'_>, a: Args) {
    let mut ty = Type::All;
    let mut filter: Option<String> = None;
    if a.len() > 0 {
        let arg0 = a.get(0);
        match Type::parse(&arg0) {
            Some(t) => {
                ty = t;
                if a.len() > 1 {
                    filter = Some(a.get(1));
                }
            }
            None => filter = Some(arg0),
        }
    }
    let mut holders: Vec<EditorHolder> = Vec::new();
    let mut tracks: Vec<Track> = Vec::new();
    if ty.groups() {
        holders.extend(matching_groups(c, |_| true).iter().map(group_holder));
        tracks.extend(c.model().tracks.values().cloned());
    }
    if ty.users() {
        let matchers: Vec<KeyMatch> = filter.iter().map(|f| KeyMatch::Like(format!("{f}%"))).collect();
        let users = matching_users(c, &matchers, ty.offline());
        holders.extend(users.iter().map(user_holder));
    }
    if holders.is_empty() {
        c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_NO_MATCH, 'c', &[])));
        return;
    }
    open_session(c, holders, tracks);
}

/// `/lp user|group <x> editor`
pub(crate) fn holder_editor(c: &mut Ctx<'_>, l: &Loaded) {
    let mut holders: Vec<EditorHolder> = Vec::new();
    if let Holder::Group(g) = &l.h {
        let key = KeyMatch::Exact(crate::node::key::group(&g.name));
        holders.extend(matching_users(c, &[key], true).iter().map(user_holder));
    }
    holders.push(match &l.h {
        Holder::User(u) => user_holder(u),
        Holder::Group(g) => group_holder(g),
    });
    open_session(c, holders, Vec::new());
}

/// `/lp track <x> editor`
pub(crate) fn track_editor(c: &mut Ctx<'_>, t: &Track) {
    let groups = matching_groups(c, |g| t.groups.contains(&g.name));
    if groups.is_empty() {
        c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_NO_MATCH, 'c', &[])));
        return;
    }
    let matchers: Vec<KeyMatch> = groups.iter().map(|g| KeyMatch::Exact(crate::node::key::group(&g.name))).collect();
    let users = matching_users(c, &matchers, true);
    let mut holders: Vec<EditorHolder> = groups.iter().map(group_holder).collect();
    holders.extend(users.iter().map(user_holder));
    open_session(c, holders, vec![t.clone()]);
}

/// `/lp trusteditor <id>`: trusting needs the live socket, which this port does not open.
pub(crate) fn trust_editor(c: &mut Ctx<'_>, a: Args) {
    let id = a.get(0);
    if id.is_empty() {
        c.msg(format!("\u{a7}c{}. \u{a7}7({id})", lang::T_COMMAND_MISC_INVALID_CODE));
        return;
    }
    c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_SOCKET_TRUST_FAILURE, 'c', &[])));
}

// ---- applying a reply ------------------------------------------------------------------------

/// The `--force` command as a click-to-suggest segment.
fn force_cmd(label: &str, code: &str) -> String {
    let cmd = format!("/{label} applyedits {code} --force");
    crate::text::rich(&format!("\u{a7}4{cmd}"), Some(crate::text::Click::SuggestCommand(cmd.clone())), Some("\u{a7}7Click to suggest the command"))
}

/// `/lp applyedits <code> [--force]`
pub(crate) fn apply_edits(c: &mut Ctx<'_>, mut a: Args) {
    let force = a.remove_flag("--force");
    let code = a.get(0);
    if code.is_empty() || !bytebin::valid_key(&code) {
        // A code with characters bytebin never uses is no code; it is never put into a URL.
        let shown: String = code.chars().filter(|c| !c.is_control()).take(40).collect();
        c.msg(format!("\u{a7}c{}. \u{a7}7({shown})", lang::T_COMMAND_MISC_INVALID_CODE));
        return;
    }
    let url = c.cfg.bytebin_url.clone();
    let body = match bytebin::with_client(&url, |b| b.get(&code)) {
        Ok(b) => b,
        Err(e) => return http_failure(c, e),
    };
    let data = match serde_json::from_slice::<Value>(&body) {
        Ok(v @ Value::Object(_)) => v,
        _ => return unable_to_read(c, &code),
    };
    apply_response(c, &code, &data, LABEL, force);
}

fn unable_to_read(c: &mut Ctx<'_>, code: &str) {
    c.msg(format!("\u{a7}c{}. \u{a7}7({code})", lang::T_COMMAND_EDITOR_APPLY_EDITS_UNABLE_TO_READ));
}

/// `WebEditorResponse.apply`
pub(crate) fn apply_response(c: &mut Ctx<'_>, code: &str, data: &Value, label: &str, force: bool) {
    let Some(session_id) = data.get("sessionId").and_then(Value::as_str) else { return unable_to_read(c, code) };
    let list = |key: &str| -> Result<&[Value], ()> {
        match data.get(key) {
            None | Some(Value::Null) => Ok(&[]),
            Some(Value::Array(a)) if a.len() <= if key == "changes" { MAX_CHANGES } else { MAX_DELETIONS } => Ok(a.as_slice()),
            _ => Err(()),
        }
    };
    let (Ok(changes), Ok(user_del), Ok(group_del), Ok(track_del)) =
        (list("changes"), list("userDeletions"), list("groupDeletions"), list("trackDeletions"))
    else {
        return unable_to_read(c, code);
    };

    let request = {
        let s = lock_sessions();
        match s.get(session_id) {
            None => {
                if !force {
                    drop(s);
                    c.msg(format!("\u{a7}4{}", lang::T_COMMAND_EDITOR_APPLY_EDITS_UNKNOWN_SESSION));
                    c.msg(format!("\u{a7}c{}", tr(lang::T_COMMAND_EDITOR_APPLY_EDITS_RIGHT_SERVER_QUESTION, 'c', &[format!("/{label} applyedits")])));
                    c.msg(format!("\u{a7}c{}: {}", lang::T_COMMAND_EDITOR_APPLY_EDITS_BYPASS_WARNING, force_cmd(label, code)));
                    return;
                }
                None
            }
            Some(sess) if sess.completed && !force => {
                drop(s);
                c.msg(format!("\u{a7}4{}", lang::T_COMMAND_EDITOR_APPLY_EDITS_ALREADY_APPLIED));
                c.msg(format!("\u{a7}c{}.", lang::T_COMMAND_EDITOR_APPLY_EDITS_HOW_TO_AVOID_CONFLICTS));
                c.msg(format!("\u{a7}c{}: {}", lang::T_COMMAND_EDITOR_APPLY_EDITS_BYPASS_WARNING, force_cmd(label, code)));
                return;
            }
            Some(sess) => Some(sess.request.clone()),
        }
    };

    let mut work = false;
    for change in changes {
        work |= apply_change(c, request.as_ref(), change);
    }
    for d in user_del {
        work |= delete_user(c, d);
    }
    for d in group_del {
        work |= delete_group(c, d);
    }
    for d in track_del {
        work |= delete_track(c, d);
    }
    if let Some(s) = lock_sessions().get_mut(session_id) {
        s.completed = true;
    }
    if !work {
        c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_APPLY_EDITS_NO_CHANGES, 'a', &[])));
    }
}

fn apply_change(c: &mut Ctx<'_>, request: Option<&Request>, change: &Value) -> bool {
    let ty = change.get("type").and_then(Value::as_str).unwrap_or("");
    match ty {
        "user" | "group" => holder_change(c, request, change, ty),
        "track" => track_change(c, request, change),
        other => {
            let shown: String = other.chars().filter(|c| !c.is_control()).take(40).collect();
            c.msg(format!("\u{a7}c{}. \u{a7}7({shown})", lang::T_COMMAND_EDITOR_APPLY_EDITS_UNKNOWN_TYPE));
            false
        }
    }
}

/// A node from the editor's JSON, or why it is refused. The key is the node string: it is
/// bounded like any permission and free of control characters, the contexts are bounded too.
pub(crate) fn parse_node(v: &Value, now: i64) -> Result<Option<Node>, &'static str> {
    let o = v.as_object().ok_or("node is not an object")?;
    let key = o.get("key").and_then(Value::as_str).ok_or("node has no key")?;
    if key.is_empty() {
        return Ok(None); // LuckPerms skips these
    }
    if key.chars().count() > MAX_PERMISSION_LENGTH || key.chars().any(|c| c.is_control()) {
        return Err("node key is too long or has control characters");
    }
    let value = o.get("value").and_then(Value::as_bool).ok_or("node value is not a boolean")?;
    let expiry = match o.get("expiry") {
        None | Some(Value::Null) => 0,
        Some(e) => {
            let e = e.as_i64().ok_or("node expiry is not a number")?;
            if e <= 0 || e > 253_402_300_799 {
                return Err("node expiry is out of range");
            }
            e
        }
    };
    let mut contexts = ContextSet::new();
    match o.get("context") {
        None | Some(Value::Null) => {}
        Some(Value::Object(m)) => {
            let mut count = 0;
            for (k, val) in m {
                let vals: Vec<&Value> = match val {
                    Value::Array(a) => a.iter().collect(),
                    other => vec![other],
                };
                for x in vals {
                    let s = x.as_str().ok_or("context value is not a string")?;
                    count += 1;
                    if count > MAX_CONTEXTS_PER_NODE
                        || k.chars().count() > MAX_PERMISSION_LENGTH
                        || s.chars().count() > MAX_PERMISSION_LENGTH
                        || k.chars().chain(s.chars()).any(|c| c.is_control())
                    {
                        return Err("node context is too big");
                    }
                    contexts.add(k, s);
                }
            }
        }
        Some(_) => return Err("node context is not an object"),
    }
    let node = Node::new(key, value, expiry, contexts);
    // A node that already ran out has nothing to apply; LuckPerms would audit it away at once.
    Ok(if node.has_expired(now) { None } else { Some(node) })
}

/// `NodeJsonSerializer.deserializeNodes`, bounded; the second value counts refused nodes.
pub(crate) fn parse_nodes(v: Option<&Value>, now: i64) -> Option<(Vec<Node>, usize)> {
    let arr = v?.as_array()?;
    if arr.len() > MAX_NODES_PER_HOLDER {
        return None;
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(arr.len());
    let mut refused = 0;
    for e in arr {
        match parse_node(e, now) {
            Ok(Some(n)) => {
                if !nodes.contains(&n) {
                    nodes.push(n);
                }
            }
            Ok(None) => {}
            Err(_) => refused += 1,
        }
    }
    Some((nodes, refused))
}

/// `Difference<Node>` applied the way `NodeMapMutable.applyChanges` does: the removals (exact
/// matches), then the additions (an equal node of another value or expiry is replaced).
pub(crate) fn apply_node_diff(set: &mut crate::model::NodeSet, before: &[Node], after: &[Node]) -> (Vec<Node>, Vec<Node>) {
    // recordChanges(REMOVE, before); recordChanges(ADD, after): equal ones cancel.
    let remove: Vec<&Node> = before.iter().filter(|b| !after.contains(b)).collect();
    let add: Vec<&Node> = after.iter().filter(|n| !before.contains(n)).collect();
    let (mut added, mut removed): (Vec<Node>, Vec<Node>) = (Vec::new(), Vec::new());
    for n in remove {
        if let Some(i) = set.nodes.iter().position(|x| x == n) {
            removed.push(set.nodes.remove(i));
        }
    }
    for n in add {
        if set.nodes.contains(n) {
            continue;
        }
        let (gone, kept): (Vec<Node>, Vec<Node>) = set.nodes.drain(..).partition(|x| x.equals(n, Equality::IgnoreExpiryTimeAndValue));
        set.nodes = kept;
        removed.extend(gone);
        set.nodes.push(n.clone());
        added.push(n.clone());
    }
    (added, removed)
}

fn holder_change(c: &mut Ctx<'_>, request: Option<&Request>, change: &Value, ty: &str) -> bool {
    let Some(id) = change.get("id").and_then(Value::as_str) else { return false };
    let now = crate::duration::now();
    let (mut l, key) = if ty == "user" {
        let Some(uuid) = crate::uuid::parse(id) else {
            let shown: String = id.chars().filter(|c| !c.is_control()).take(40).collect();
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_NOT_UUID, &[dred(&shown)]);
            return false;
        };
        let Some(l) = c.load_user(uuid) else {
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_SPECIFIC, &[dred(&crate::uuid::format(uuid))]);
            return false;
        };
        (l, HolderKey::User(uuid))
    } else {
        let name = id.to_lowercase();
        if !valid_group_name(&name) {
            let shown: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID, &[dred(&shown)]);
            return false;
        }
        let existing = c.model().groups.get(&name).cloned();
        let l = match existing {
            Some(g) => Loaded { before: g.data.nodes.clone(), h: Holder::Group(g) },
            None => {
                if c.st.create_group(&name).is_err() {
                    c.bad(lang::T_COMMAND_GENERIC_CREATE_ERROR, &[dred(&name)]);
                    return false;
                }
                Loaded { h: Holder::Group(Group::new(&name)), before: Vec::new() }
            }
        };
        (l, HolderKey::Group(name))
    };

    let Some((new_nodes, refused)) = parse_nodes(change.get("nodes"), now) else {
        let shown: String = id.chars().filter(|c| !c.is_control()).take(40).collect();
        unable_to_read(c, &shown);
        return false;
    };
    if refused > 0 {
        c.msg(format!("\u{a7}cIgnored {refused} invalid node(s) in the editor data for {}.", dred(&l.h.name())));
    }

    let (added, removed) = match request.and_then(|r| r.holders.get(&key)) {
        // The data the editor was shown is known: apply only what the editor changed.
        Some(before) => {
            let before = before.clone();
            apply_node_diff(l.h.data_mut(), &before, &new_nodes)
        }
        // Otherwise the editor's list replaces the holder's nodes (`setNodes`).
        None => {
            let (a, r) = crate::storage::diff(&l.h.data().nodes, &new_nodes);
            l.h.data_mut().nodes = new_nodes;
            (a, r)
        }
    };
    if added.is_empty() && removed.is_empty() {
        return false;
    }

    let (target, uuid, name) = (l.h.target_type(), l.h.uuid(), l.h.name());
    for (verb, nodes) in [("add", &added), ("remove", &removed)] {
        for n in nodes.iter() {
            let v = n.value.to_string();
            c.log(target, uuid, &name, &[Part::Text("webeditor"), Part::Text(verb), Part::Text(&n.key), Part::Text(&v), Part::Contexts(&n.contexts)]);
        }
    }
    c.ok(lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS, &[ty.to_owned(), l.h.arg()]);
    summary(c, added.len(), removed.len());
    for n in &added {
        c.raw(format!("\u{a7}a+  {}", node_text(n, now)));
    }
    for n in &removed {
        c.raw(format!("\u{a7}c-  {}", node_text(n, now)));
    }
    if let Holder::User(u) = &mut l.h {
        u.give_default_if_needed();
    }
    c.save(l);
    true
}

/// `APPLY_EDITS_SUCCESS_SUMMARY`: `(2 additions and 1 deletion)`
fn summary(c: &mut Ctx<'_>, added: usize, removed: usize) {
    let a = if added == 1 { lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_ADDITIONS_SINGULAR } else { lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_ADDITIONS };
    let d = if removed == 1 { lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_DELETIONS_SINGULAR } else { lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS_DELETIONS };
    c.msg(format!("\u{a7}7(\u{a7}a{added} \u{a7}7{a} and \u{a7}c{removed} \u{a7}7{d})"));
}

/// `APPLY_EDIT_NODE`: `[Type] key (true) (context) (expiry)`
pub(crate) fn node_text(n: &Node, now: i64) -> String {
    let (ty, key) = match &n.kind {
        Kind::Inheritance(g) => (format!("\u{a7}9{}", lang::T_COMMAND_MISC_NODE_INHERITANCE), g.clone()),
        Kind::Prefix(p, v) | Kind::Suffix(p, v) => {
            let prefix = matches!(n.kind, Kind::Prefix(..));
            let (col, label) = if prefix { ("\u{a7}a", lang::T_COMMAND_MISC_NODE_PREFIX) } else { ("\u{a7}e", lang::T_COMMAND_MISC_NODE_SUFFIX) };
            (
                format!("{col}{label}"),
                format!("\u{a7}f'{}\u{a7}f' \u{a7}7[{}]", fmt::colored_value(v), tr(lang::T_COMMAND_MISC_PRIORITY_LABEL, '7', &[p.to_string()])),
            )
        }
        Kind::Meta(k, v) => (format!("\u{a7}c{}", lang::T_COMMAND_MISC_NODE_META), format!("\u{a7}f{k}\u{a7}7 = \u{a7}f{v}")),
        Kind::Weight(w) => (format!("\u{a7}b{}", lang::T_COMMAND_MISC_NODE_WEIGHT), w.to_string()),
        Kind::DisplayName(d) => (format!("\u{a7}5{}", lang::T_COMMAND_MISC_NODE_DISPLAYNAME), d.clone()),
        _ => (format!("\u{a7}7{}", lang::T_COMMAND_MISC_NODE_PERMISSION), n.key.clone()),
    };
    let mut out = format!("\u{a7}f\u{a7}8[{ty}\u{a7}8] \u{a7}f{key} \u{a7}7(\u{a7}{}{}\u{a7}7)", if n.value { 'a' } else { 'c' }, n.value);
    if !n.contexts.is_empty() {
        out.push(' ');
        out.push_str(&fmt::ctx_bracketed(&n.contexts, ""));
    }
    if n.has_expiry() {
        out.push_str(&format!(" \u{a7}7({})", crate::duration::format_concise((n.expiry - now).max(0))));
    }
    out
}

fn track_change(c: &mut Ctx<'_>, _request: Option<&Request>, change: &Value) -> bool {
    let Some(id) = change.get("id").and_then(Value::as_str) else { return false };
    let name = id.to_lowercase();
    if !valid_track_name(&name) {
        let shown: String = name.chars().filter(|c| !c.is_control()).take(40).collect();
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_TRACK_INVALID, &[dred(&shown)]);
        return false;
    }
    let Some(groups) = change.get("groups").and_then(Value::as_array).filter(|a| a.len() <= MAX_TRACK_GROUPS) else {
        return false;
    };
    let mut after: Vec<String> = Vec::new();
    for g in groups {
        let Some(g) = g.as_str().map(str::to_lowercase) else { return false };
        if !valid_group_name(&g) {
            c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_GROUP_INVALID, &[dred(&g.chars().filter(|c| !c.is_control()).take(40).collect::<String>())]);
            return false;
        }
        if !after.contains(&g) {
            after.push(g);
        }
    }
    let existing = c.model().tracks.get(&name).cloned();
    let before = match existing {
        Some(t) => t.groups,
        None => {
            // `createAndLoadTrack`
            if !super::track::save_track(c, Track { name: name.clone(), groups: Vec::new() }) {
                return false;
            }
            Vec::new()
        }
    };
    if before == after {
        return false;
    }
    let added: Vec<&String> = after.iter().filter(|g| !before.contains(g)).collect();
    let removed: Vec<&String> = before.iter().filter(|g| !after.contains(g)).collect();
    if !super::track::save_track(c, Track { name: name.clone(), groups: after.clone() }) {
        return false;
    }
    let kept_before: Vec<&String> = before.iter().filter(|g| !removed.contains(g)).collect();
    let kept_after: Vec<&String> = after.iter().filter(|g| !added.contains(g)).collect();
    if kept_before != kept_after {
        let list = format!("[{}]", after.join(", "));
        c.log(TargetType::Track, None, &name, &[Part::Text("webeditor"), Part::Text("reorder"), Part::Owned(list)]);
    }
    for g in &added {
        c.log(TargetType::Track, None, &name, &[Part::Text("webeditor"), Part::Text("add"), Part::Text(g)]);
    }
    for g in &removed {
        c.log(TargetType::Track, None, &name, &[Part::Text("webeditor"), Part::Text("remove"), Part::Text(g)]);
    }
    c.ok(lang::T_COMMAND_EDITOR_APPLY_EDITS_SUCCESS, &["track".into(), aq(&name)]);
    summary(c, added.len(), removed.len());
    c.raw(format!("\u{a7}c-  {}", fmt::track_path(&before)));
    c.raw(format!("\u{a7}a+  {}", fmt::track_path(&after)));
    true
}

fn shown(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(40).collect()
}

fn delete_user(c: &mut Ctx<'_>, v: &Value) -> bool {
    let Some(id) = v.as_str() else { return false };
    let Some(uuid) = crate::uuid::parse(id) else {
        c.bad(lang::T_COMMAND_MISC_LOADING_ERROR_USER_NOT_UUID, &[dred(&shown(id))]);
        return false;
    };
    let Some(mut l) = c.load_user(uuid) else { return false };
    let name = l.h.name();
    l.h.data_mut().nodes.clear();
    if let Holder::User(u) = &mut l.h {
        u.give_default_if_needed();
    }
    if !c.save(l) {
        return true;
    }
    c.ok(lang::T_COMMAND_GENERIC_DELETE_SUCCESS, &[aq(&name)]);
    c.log(TargetType::User, Some(uuid), &name, &[Part::Text("webeditor"), Part::Text("delete")]);
    true
}

fn delete_group(c: &mut Ctx<'_>, v: &Value) -> bool {
    let Some(name) = v.as_str().map(str::to_lowercase) else { return false };
    if name == crate::model::DEFAULT_GROUP {
        c.msg(format!("{}.", tr(lang::T_COMMAND_GROUP_DELETE_NOT_DEFAULT, 'c', &[])));
        return true;
    }
    let Some(group) = c.model().groups.get(&name).cloned() else { return false };
    let display = Holder::Group(group).arg();
    if c.st.delete_group(&name).is_err() {
        c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[dred(&name)]);
        return true;
    }
    {
        let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.groups.remove(&name);
        m.touch();
    }
    c.out.group_changed = true;
    c.out.sync = true;
    c.ok(lang::T_COMMAND_GENERIC_DELETE_SUCCESS, &[display]);
    c.log(TargetType::Group, None, &name, &[Part::Text("webeditor"), Part::Text("delete")]);
    true
}

fn delete_track(c: &mut Ctx<'_>, v: &Value) -> bool {
    let Some(name) = v.as_str().map(str::to_lowercase) else { return false };
    if !c.model().tracks.contains_key(&name) {
        return false;
    }
    if c.st.delete_track(&name).is_err() {
        c.bad(lang::T_COMMAND_GENERIC_DELETE_ERROR, &[dred(&name)]);
        return true;
    }
    {
        let mut m = c.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.tracks.remove(&name);
        m.touch();
    }
    c.out.group_changed = true;
    c.ok(lang::T_COMMAND_GENERIC_DELETE_SUCCESS, &[aq(&name)]);
    c.log(TargetType::Track, None, &name, &[Part::Text("webeditor"), Part::Text("delete")]);
    true
}

#[cfg(all(test, feature = "net"))]
mod tests {
    use super::*;
    use crate::bytebin::{set_test_client, Bytebin};
    use crate::command::execute;
    use crate::config::Config;
    use crate::model::Model;
    use crate::runtime::{detached, CommandSender, OnlinePlayer, Runtime};
    use crate::storage::{MemoryStorage, Storage};
    use crate::text::{plain, split_args};
    use std::cell::RefCell;
    use std::rc::Rc;

    const ALICE: u128 = 0xa11ce;
    const BOB: u128 = 0xb0b;

    /// A bytebin that keeps what was posted and serves what the test puts in.
    #[derive(Default)]
    struct Fake {
        prefix: String,
        posted: RefCell<Vec<Vec<u8>>>,
        replies: RefCell<HashMap<String, Vec<u8>>>,
        fail: RefCell<Option<HttpError>>,
    }

    impl Bytebin for Fake {
        fn post(&self, gz: &[u8]) -> Result<String, HttpError> {
            if let Some(e) = self.fail.borrow_mut().take() {
                return Err(e);
            }
            self.posted.borrow_mut().push(gz.to_vec());
            Ok(format!("{}x{}", self.prefix, self.posted.borrow().len()))
        }
        fn get(&self, id: &str) -> Result<Vec<u8>, HttpError> {
            self.replies.borrow().get(id).cloned().ok_or(HttpError::Status(404, "Not Found".into()))
        }
    }

    fn setup() -> (Runtime, MemoryStorage, Rc<Fake>) {
        let mut st = MemoryStorage::default();
        st.init().unwrap();
        let mut m = Model::default();
        m.ensure_default_group();
        let rt = detached(Config::default(), m);
        rt.update_players(vec![OnlinePlayer { uuid: ALICE, name: "Alice".into(), op_level: 0, dimension: "minecraft:overworld".into(), game_mode: 0 }]);
        st.save_player_data(ALICE, "Alice").unwrap();
        st.save_player_data(BOB, "Bob").unwrap();
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let fake = Rc::new(Fake { prefix: format!("t{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)), ..Default::default() });
        set_test_client(Some(fake.clone()));
        (rt, st, fake)
    }

    fn run(rt: &Runtime, st: &mut MemoryStorage, line: &str) -> Vec<String> {
        let out = execute(rt, st, &CommandSender::console(), &split_args(line));
        for a in &out.actions {
            st.log_action(a).unwrap();
        }
        out.lines.iter().map(|l| plain(l)).collect()
    }

    fn posted_json(fake: &Fake, i: usize) -> Value {
        let gz = fake.posted.borrow()[i].clone();
        serde_json::from_slice(&bytebin::gunzip(&gz, 1 << 24).unwrap()).unwrap()
    }

    fn node(key: &str, value: bool) -> Node {
        Node::permanent(key, value, ContextSet::new())
    }

    // ---- payload ----

    #[test]
    fn payload_has_the_editors_shape() {
        let mut ctx = ContextSet::new();
        ctx.add("world", "nether");
        let mut temp = Node::new("essentials.fly", true, 4_000_000_000, ctx.clone());
        temp.contexts = ctx;
        let holders = vec![
            EditorHolder { key: HolderKey::Group("admin".into()), display: "Admin".into(), nodes: vec![node("group.default", true), temp] },
            EditorHolder { key: HolderKey::User(ALICE), display: "Alice".into(), nodes: vec![node("prefix.100.&cA", true)] },
        ];
        let tracks = vec![Track { name: "staff".into(), groups: vec!["default".into(), "admin".into()] }];
        let p = build_payload(&holders, &tracks, "Console", 0, "lp", &ContextSet::of(&[("server", "lobby")]), 1_700_000_000_000, &["minecraft.command.gamemode".to_owned()]);

        assert_eq!(p["metadata"]["commandAlias"], "lp");
        assert_eq!(p["metadata"]["uploader"]["name"], "Console");
        assert_eq!(p["metadata"]["uploader"]["uuid"], "00000000-0000-0000-0000-000000000000");
        assert_eq!(p["metadata"]["platform"], "Horizon");
        assert_eq!(p["permissionHolders"][0]["type"], "group");
        assert_eq!(p["permissionHolders"][0]["id"], "admin");
        assert_eq!(p["permissionHolders"][0]["displayName"], "Admin");
        let n = &p["permissionHolders"][0]["nodes"];
        assert_eq!(n[0], json!({"type": "inheritance", "key": "group.default", "value": true}));
        assert_eq!(n[1], json!({"type": "permission", "key": "essentials.fly", "value": true, "expiry": 4_000_000_000i64, "context": {"world": "nether"}}));
        assert_eq!(p["permissionHolders"][1]["type"], "user");
        assert_eq!(p["permissionHolders"][1]["id"], crate::uuid::format(ALICE));
        assert_eq!(p["permissionHolders"][1]["nodes"][0]["type"], "prefix");
        assert_eq!(p["tracks"][0], json!({"type": "track", "id": "staff", "groups": ["default", "admin"]}));
        assert_eq!(p["knownPermissions"], json!(["essentials.fly", "minecraft.command.gamemode"]));
        // The node contexts join the potential ones.
        assert_eq!(p["potentialContexts"], json!({"server": "lobby", "world": "nether"}));
    }

    /// The privacy rule: the payload holds names, UUIDs, nodes, tracks and contexts, and nothing else.
    #[test]
    fn payload_carries_no_personal_data_beyond_names_and_uuids() {
        let holders = vec![EditorHolder { key: HolderKey::User(ALICE), display: "Alice".into(), nodes: vec![node("a.b", true)] }];
        let p = build_payload(&holders, &[], "Alice", ALICE, "lp", &ContextSet::new(), 1, &[]);
        fn keys(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::Object(m) => {
                    for (k, x) in m {
                        out.push(k.clone());
                        keys(x, out);
                    }
                }
                Value::Array(a) => a.iter().for_each(|x| keys(x, out)),
                _ => {}
            }
        }
        let mut found = Vec::new();
        keys(&p, &mut found);
        let allowed = [
            "metadata", "commandAlias", "uploader", "name", "uuid", "time", "pluginVersion", "platform", "permissionHolders", "type", "id",
            "displayName", "nodes", "key", "value", "expiry", "context", "tracks", "groups", "knownPermissions", "potentialContexts",
        ];
        for k in &found {
            assert!(allowed.contains(&k.as_str()), "unexpected field in the payload: {k}");
        }
        for banned in ["ip", "address", "lastIp", "ipAddress", "host", "email", "primaryGroup", "username"] {
            assert!(!found.iter().any(|k| k.eq_ignore_ascii_case(banned)), "{banned}");
        }
        assert!(!personal_data_in(&p.to_string()));
    }

    #[test]
    fn ip_addresses_are_recognised() {
        for yes in ["203.0.113.7", "x.10.0.0.1.y", "{\"key\":\"ban.192.168.1.20\"}", "2001:db8:85a3:0:0:8a2e:370:7334", "fe80::1ff:fe23:4567:890a"] {
            assert!(personal_data_in(yes), "{yes}");
        }
        for no in ["essentials.fly", "5.5.0", "prefix.100.&c[Admin]", "1700000000000", "group.default", "time 12:30:45", "a.b.c.d", "kit.1.2.3"] {
            assert!(!personal_data_in(no), "{no}");
        }
    }

    // ---- /lp editor ----

    #[test]
    fn editor_uploads_gzip_and_prints_the_link() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "creategroup mod 50");
        run(&rt, &mut st, "group mod permission set essentials.fly true");
        run(&rt, &mut st, "createtrack staff");
        run(&rt, &mut st, "track staff append mod");
        run(&rt, &mut st, "user Bob permission set bob.perm true");
        let out = run(&rt, &mut st, "editor");
        assert!(out[0].contains("Preparing a new editor session, please wait..."), "{out:?}");
        assert!(out[1].contains("Click the link below to open the editor:"), "{out:?}");
        assert_eq!(out[2], format!("https://luckperms.net/editor/{}x1", fake.prefix));

        let gz = fake.posted.borrow()[0].clone();
        assert_eq!(&gz[..2], &[0x1f, 0x8b], "the upload is gzip");
        let p = posted_json(&fake, 0);
        let ids: Vec<&str> = p["permissionHolders"].as_array().unwrap().iter().map(|h| h["id"].as_str().unwrap()).collect();
        assert_eq!(&ids[..2], ["mod", "default"], "groups by weight, high first: {ids:?}");
        assert!(ids.contains(&crate::uuid::format(BOB).as_str()), "offline users come from storage: {ids:?}");
        assert_eq!(p["tracks"][0]["id"], "staff");
        assert!(!personal_data_in(&p.to_string()));
    }

    #[test]
    fn editor_types_and_filter() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "user Bob permission set bob.perm true");
        run(&rt, &mut st, "user Bob permission set other.perm true");
        run(&rt, &mut st, "editor groups");
        let p = posted_json(&fake, 0);
        assert!(p["permissionHolders"].as_array().unwrap().iter().all(|h| h["type"] == "group"));
        run(&rt, &mut st, "editor users bob.");
        let p = posted_json(&fake, 1);
        let hs = p["permissionHolders"].as_array().unwrap();
        assert_eq!(hs.len(), 1, "{hs:?}");
        assert_eq!(hs[0]["id"], crate::uuid::format(BOB));
        assert!(p["tracks"].as_array().unwrap().is_empty(), "users only: no tracks");
        // a first argument that is no type is the filter
        run(&rt, &mut st, "editor bob.");
        // (groups are always all of them; the filter narrows the users)
        let hs = posted_json(&fake, 2);
        let users: Vec<&Value> = hs["permissionHolders"].as_array().unwrap().iter().filter(|h| h["type"] == "user").collect();
        assert_eq!(users.len(), 1);
        let out = run(&rt, &mut st, "editor users nothing.like.this");
        assert!(out[0].contains("Unable to open editor, no objects matched the desired type."), "{out:?}");
        assert_eq!(fake.posted.borrow().len(), 3);
    }

    #[test]
    fn editor_failures_are_answered() {
        let (rt, mut st, fake) = setup();
        *fake.fail.borrow_mut() = Some(HttpError::Status(503, "Service Unavailable".into()));
        let out = run(&rt, &mut st, "editor");
        assert!(out.last().unwrap().contains("Unable to communicate with the editor. (response code 503, message='Service Unavailable')"), "{out:?}");
        *fake.fail.borrow_mut() = Some(HttpError::Other("connection refused".into()));
        let out = run(&rt, &mut st, "editor");
        assert!(out.last().unwrap().contains("Unable to communicate with the editor. Check the console for errors."), "{out:?}");
        assert!(!out.iter().any(|l| l.contains("refused")), "the cause stays in the console");
    }

    #[test]
    fn a_node_that_looks_like_an_ip_stops_the_upload() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "group default permission set ipban.203.0.113.7 true");
        let out = run(&rt, &mut st, "editor groups");
        assert!(out.last().unwrap().contains("not opened"), "{out:?}");
        assert!(!out.iter().any(|l| l.contains("203.0.113.7")), "the address is not echoed");
        assert!(fake.posted.borrow().is_empty());
    }

    #[test]
    fn holder_and_track_editors() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "creategroup mod 50");
        run(&rt, &mut st, "user Bob parent add mod");
        run(&rt, &mut st, "createtrack staff");
        run(&rt, &mut st, "track staff append mod");
        run(&rt, &mut st, "group mod editor");
        let ids: Vec<String> = posted_json(&fake, 0)["permissionHolders"].as_array().unwrap().iter().map(|h| h["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, vec![crate::uuid::format(BOB), "mod".to_owned()], "members first, the group last");
        run(&rt, &mut st, "track staff editor");
        let p = posted_json(&fake, 1);
        assert_eq!(p["tracks"][0]["id"], "staff");
        assert_eq!(p["permissionHolders"][0]["id"], "mod");
        let out = run(&rt, &mut st, "createtrack empty");
        assert!(out[0].contains("successfully created"));
        let out = run(&rt, &mut st, "track empty editor");
        assert!(out[0].contains("no objects matched"), "{out:?}");
    }

    // ---- /lp applyedits ----

    fn reply(fake: &Fake, key: &str, v: Value) {
        fake.replies.borrow_mut().insert(key.to_owned(), serde_json::to_vec(&v).unwrap());
    }

    #[test]
    fn applyedits_applies_only_what_the_editor_changed() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "creategroup mod 50");
        run(&rt, &mut st, "group mod permission set keep.me true");
        run(&rt, &mut st, "group mod permission set remove.me true");
        run(&rt, &mut st, "editor groups");
        // Someone adds a node after the editor was opened; the apply must not undo it.
        run(&rt, &mut st, "group mod permission set added.later true");
        let id = format!("{}x1", fake.prefix);
        reply(
            &fake,
            &id,
            json!({"sessionId": id, "changes": [{"type": "group", "id": "mod", "nodes": [
                {"type": "weight", "key": "weight.50", "value": true},
                {"type": "permission", "key": "keep.me", "value": true},
                {"type": "permission", "key": "new.node", "value": false, "context": {"world": "nether"}},
            ]}]}),
        );
        let out = run(&rt, &mut st, &format!("applyedits {id}"));
        assert!(out[0].contains("Web editor data was applied to group mod successfully."), "{out:?}");
        assert!(out[1].contains("(1 addition and 1 deletion)"), "{out:?}");
        assert!(out.iter().any(|l| l.starts_with("+  [Permission] new.node (false) (world=nether)")), "{out:?}");
        assert!(out.iter().any(|l| l.starts_with("-  [Permission] remove.me (true)")), "{out:?}");
        let keys: Vec<String> = rt.model.read().unwrap().groups["mod"].data.nodes.iter().map(|n| n.key.clone()).collect();
        assert!(keys.contains(&"added.later".to_owned()), "{keys:?}");
        assert!(keys.contains(&"new.node".to_owned()) && keys.contains(&"keep.me".to_owned()), "{keys:?}");
        assert!(!keys.contains(&"remove.me".to_owned()), "{keys:?}");
        // it is in the action log and in storage
        let log = st.actions(50, None).unwrap();
        assert!(log.iter().any(|a| a.description == "webeditor add new.node false world=nether"), "{log:?}");
        assert!(log.iter().any(|a| a.description == "webeditor remove remove.me true"));
        let (_, g) = (0, st.load_group("mod").unwrap().unwrap());
        assert!(g.data.nodes.iter().any(|n| n.key == "new.node"));

        // the same session again is refused without --force
        let out = run(&rt, &mut st, &format!("applyedits {id}"));
        assert!(out[0].contains("already been applied"), "{out:?}");
        let out = run(&rt, &mut st, &format!("applyedits {id} --force"));
        assert!(!out[0].contains("already been applied"), "{out:?}");
    }

    #[test]
    fn applyedits_unknown_session_and_force() {
        let (rt, mut st, fake) = setup();
        reply(&fake, "other", json!({"sessionId": "elsewhere", "changes": [{"type": "group", "id": "newgroup", "nodes": [{"type":"permission","key":"x.y","value":true}]}]}));
        let out = run(&rt, &mut st, "applyedits other");
        assert!(out[0].contains("were not made in a session started on this server!"), "{out:?}");
        assert!(out[1].contains("Are you sure you're running the /lp applyedits command in the right place?"), "{out:?}");
        assert!(out[2].contains("/lp applyedits other --force"), "{out:?}");
        assert!(!rt.model.read().unwrap().groups.contains_key("newgroup"));
        let out = run(&rt, &mut st, "applyedits other --force");
        assert!(out[0].contains("applied to group newgroup"), "{out:?}");
        assert!(rt.model.read().unwrap().groups["newgroup"].data.nodes.iter().any(|n| n.key == "x.y"), "created from the editor");
    }

    #[test]
    fn applyedits_tracks_and_deletions() {
        let (rt, mut st, fake) = setup();
        run(&rt, &mut st, "creategroup mod 50");
        run(&rt, &mut st, "creategroup admin 80");
        run(&rt, &mut st, "createtrack staff");
        run(&rt, &mut st, "createtrack old");
        run(&rt, &mut st, "track staff append mod");
        run(&rt, &mut st, "user Bob permission set bob.perm true");
        reply(
            &fake,
            "d1",
            json!({"sessionId": "none", "changes": [{"type": "track", "id": "staff", "groups": ["admin", "mod"]}],
                   "groupDeletions": ["admin", "default", "ghost"], "trackDeletions": ["old"], "userDeletions": [crate::uuid::format(BOB)]}),
        );
        let out = run(&rt, &mut st, "applyedits d1 --force");
        let all = out.join("\n");
        assert!(all.contains("applied to track staff successfully."), "{all}");
        assert!(all.contains("(1 addition and 0 deletions)"), "{all}");
        assert!(all.contains("You cannot delete the default group."), "{all}");
        assert!(all.contains("admin was successfully deleted."), "{all}");
        assert!(all.contains("old was successfully deleted."), "{all}");
        let m = rt.model.read().unwrap();
        assert_eq!(m.tracks["staff"].groups, vec!["admin", "mod"]);
        assert!(!m.tracks.contains_key("old") && !m.groups.contains_key("admin") && m.groups.contains_key("default"));
        drop(m);
        assert!(st.load_user(BOB).unwrap().0.iter().all(|n| n.key != "bob.perm"), "user nodes cleared");
        let out = run(&rt, &mut st, "applyedits d1 --force");
        assert!(out.iter().any(|l| l.contains("default group")), "{out:?}");
    }

    #[test]
    fn applyedits_no_changes_and_bad_input() {
        let (rt, mut st, fake) = setup();
        reply(&fake, "empty", json!({"sessionId": "x"}));
        let out = run(&rt, &mut st, "applyedits empty --force");
        assert!(out[0].contains("No changes were applied from the web editor, the returned data didn't contain any edits."), "{out:?}");
        let out = run(&rt, &mut st, "applyedits missing");
        assert!(out[0].contains("Unable to communicate with the editor. (response code 404, message='Not Found')"), "{out:?}");
        fake.replies.borrow_mut().insert("junk".into(), b"not json".to_vec());
        let out = run(&rt, &mut st, "applyedits junk");
        assert!(out[0].contains("Unable to read data using the given code. (junk)"), "{out:?}");
        reply(&fake, "ty", json!({"sessionId": "x", "changes": [{"type": "planet", "id": "x"}]}));
        let out = run(&rt, &mut st, "applyedits ty --force");
        assert!(out[0].contains("Unable to apply edit to the specified object type. (planet)"), "{out:?}");
        // a code that could rewrite the request path is not a code
        let out = run(&rt, &mut st, "applyedits ../../etc/passwd");
        assert!(out[0].contains("Invalid code."), "{out:?}");
        let out = run(&rt, &mut st, "applyedits");
        assert!(out[0].contains("Usage") || out[0].contains("applyedits"), "{out:?}");
    }

    #[test]
    fn downloaded_data_is_validated_and_capped() {
        let now = 1_700_000_000;
        // refused: wrong types, control characters, absurd length, bad expiry
        for bad in [
            json!({"key": "a\nb", "value": true}),
            json!({"key": "a".repeat(201), "value": true}),
            json!({"key": "a", "value": "yes"}),
            json!({"key": "a", "value": true, "expiry": -5}),
            json!({"key": "a", "value": true, "context": {"k": 5}}),
            json!({"key": "a", "value": true, "context": ["x"]}),
            json!("a"),
        ] {
            assert!(parse_node(&bad, now).is_err(), "{bad}");
        }
        assert_eq!(parse_node(&json!({"key": "", "value": true}), now), Ok(None), "empty keys are skipped as LuckPerms does");
        assert_eq!(parse_node(&json!({"key": "gone", "value": true, "expiry": 5}), now), Ok(None), "expired nodes are dropped");
        let n = parse_node(&json!({"key": "Group.Admin", "value": true, "context": {"world": ["a", "b"]}}), now).unwrap().unwrap();
        assert_eq!(n.key, "group.admin");
        assert_eq!(n.contexts.values("world").len(), 2);
        // too many nodes for one holder
        let big = Value::Array((0..MAX_NODES_PER_HOLDER + 1).map(|i| json!({"key": format!("k{i}"), "value": true})).collect());
        assert!(parse_nodes(Some(&big), now).is_none());
        let (nodes, refused) = parse_nodes(Some(&json!([{"key": "ok", "value": true}, {"key": "ok", "value": true}, {"key": "x\u{7}", "value": true}])), now).unwrap();
        assert_eq!((nodes.len(), refused), (1, 1), "duplicates collapse, bad nodes are counted");

        // too many changes in one reply
        let (rt, mut st, fake) = setup();
        let changes: Vec<Value> = (0..MAX_CHANGES + 1).map(|_| json!({"type": "group", "id": "g", "nodes": []})).collect();
        reply(&fake, "huge", json!({"sessionId": "s", "changes": changes}));
        let out = run(&rt, &mut st, "applyedits huge --force");
        assert!(out[0].contains("Unable to read data"), "{out:?}");
        // bad names never reach the model
        reply(&fake, "names", json!({"sessionId": "s", "changes": [
            {"type": "group", "id": "has space", "nodes": []},
            {"type": "user", "id": "not-a-uuid", "nodes": []},
            {"type": "track", "id": "bad track", "groups": []},
        ]}));
        let out = run(&rt, &mut st, "applyedits names --force");
        let all = out.join("\n");
        assert!(all.contains("has space is not a valid group name"), "{all}");
        assert!(all.contains("Target user not-a-uuid is not a valid uuid"), "{all}");
        assert!(all.contains("bad track is not a valid track name"), "{all}");
        assert_eq!(rt.model.read().unwrap().groups.len(), 1);
    }

    #[test]
    fn node_diff_replaces_and_cancels() {
        let mut set = crate::model::NodeSet { nodes: vec![node("a", true), node("b", true), node("c", true)] };
        let before = vec![node("a", true), node("b", true), node("c", true)];
        // a unchanged, b flipped to false, c gone, d new
        let after = vec![node("a", true), node("b", false), node("d", true)];
        let (added, removed) = apply_node_diff(&mut set, &before, &after);
        let keys = |v: &Vec<Node>| v.iter().map(|n| format!("{}={}", n.key, n.value)).collect::<Vec<_>>();
        assert_eq!(keys(&added), ["b=false", "d=true"]);
        assert_eq!(keys(&removed), ["b=true", "c=true"]);
        let mut left = keys(&set.nodes);
        left.sort();
        assert_eq!(left, ["a=true", "b=false", "d=true"]);
        // a node somebody else added meanwhile survives
        let mut set = crate::model::NodeSet { nodes: vec![node("a", true), node("later", true)] };
        let (added, removed) = apply_node_diff(&mut set, &[node("a", true)], &[node("a", true)]);
        assert!(added.is_empty() && removed.is_empty() && set.nodes.len() == 2);
    }

    #[test]
    fn trusteditor_has_no_socket() {
        let (rt, mut st, _fake) = setup();
        let out = run(&rt, &mut st, "trusteditor abc");
        assert!(out[0].contains("Unable to trust the given session because the socket is closed"), "{out:?}");
    }
}
