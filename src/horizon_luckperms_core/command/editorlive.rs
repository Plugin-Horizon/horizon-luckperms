//! The web editor's live socket on the worker thread: opening a channel for `/lp editor`, trusting a browser
//! (`/lp trusteditor`), and handling the frames the editor sends (`HandlerHello`, `HandlerChangeRequest`, ...).
//! The protocol itself is `editorsocket::SocketState`; this is its [`Env`] on the server: sessions, the
//! keystore (a `lp-editor-key` meta node of the player, `editor-keystore.json` for the console), applying the
//! editor's reply and making the follow-up session.

use super::editor::{apply_response, group_holder, lock_sessions, post, prepare, user_holder, EditorHolder};
use super::{CmdOutput, Ctx, Holder, LABEL};
use crate::horizon_luckperms_core::bytebin::{self, HttpError};
use crate::horizon_luckperms_core::context::ContextSet;
use crate::horizon_luckperms_core::editorsocket::{self as sock, Env, FrameSink, Notice, Who};
use crate::horizon_luckperms_core::lang::{self, tr};
use crate::horizon_luckperms_core::model::Track;
use crate::horizon_luckperms_core::node::{key, Kind, Node};
use crate::horizon_luckperms_core::runtime::{self, CommandSender, Runtime};
use crate::horizon_luckperms_core::storage::{HolderKey, Storage};
use crate::horizon_luckperms_core::text::{rich, Click};
use crate::horizon_luckperms_core::uuid;
use serde_json::Value;
use std::sync::Arc;

/// The meta key a player's trusted editor key is stored under.
const META_KEY: &str = "lp-editor-key";

/// Opens the sender's socket for an editor request: the generation of the socket and the `socket` detail the request
/// carries. None when the service is unreachable.
pub(super) fn open(c: &mut Ctx<'_>, holders: &[EditorHolder], tracks: &[Track]) -> Option<(u64, Value)> {
    let who = Who { uuid: c.sender.uuid, name: c.sender.name.clone(), console: c.sender.console };
    let keys: Vec<HolderKey> = holders.iter().map(|h| h.key.clone()).collect();
    let names: Vec<String> = tracks.iter().map(|t| t.name.clone()).collect();
    let sink: FrameSink = Arc::new(|owner, generation, text| {
        if let Some(rt) = runtime::get() {
            rt.submit_socket_frame(owner, generation, text);
        }
    });
    let info = sock::open(who, keys, names, &c.cfg.bytesocks_url, sink)?;
    Some((info.generation, info.detail()))
}

/// `/lp trusteditor <nonce>`
pub(super) fn trust(c: &mut Ctx<'_>, nonce: &str) -> bool {
    let owner = c.sender.uuid;
    let Some(mut socket) = sock::take(owner, None) else { return false };
    let ok = {
        let mut env = SocketEnv { c: &mut *c, holders: &mut socket.holders, tracks: &mut socket.tracks };
        socket.state.trust(&mut env, nonce)
    };
    sock::put_back(owner, socket);
    ok
}

/// A frame of the editor of `owner`'s socket (of that `generation`). The answers go out over the socket, what the
/// player is told comes back for the worker to deliver.
pub(crate) fn socket_frame(rt: &Runtime, st: &mut dyn Storage, owner: u128, generation: u64, text: &str) -> Option<(CommandSender, CmdOutput)> {
    let mut socket = sock::take(owner, Some(generation))?;
    let sender = CommandSender { uuid: owner, name: socket.state.who.name.clone(), console: socket.state.who.console };
    let mut c = Ctx { rt, st, sender: &sender, cfg: rt.config(), out: CmdOutput::default() };
    let result = {
        let mut env = SocketEnv { c: &mut c, holders: &mut socket.holders, tracks: &mut socket.tracks };
        socket.state.handle(&mut env, text)
    };
    if let Err(e) = result {
        eprintln!("[LuckPerms] Exception occurred handling message from socket: {e}");
    }
    let out = std::mem::take(&mut c.out);
    sock::put_back(owner, socket);
    Some((sender, out))
}

struct SocketEnv<'a, 'b> {
    c: &'a mut Ctx<'b>,
    holders: &'a mut Vec<HolderKey>,
    tracks: &'a mut Vec<String>,
}

impl SocketEnv<'_, '_> {
    fn keystore() -> std::path::PathBuf {
        super::backup::data_file(sock::KEYSTORE_FILE)
    }

    /// The `lp-editor-key` of a loaded player.
    fn user_key_is(&self, hash: &str) -> bool {
        let m = self.c.model();
        m.users.get(&self.c.sender.uuid).is_some_and(|u| u.data.nodes.iter().any(|n| n.value && matches!(&n.kind, Kind::Meta(k, v) if k == META_KEY && v == hash)))
    }
}

impl Env for SocketEnv<'_, '_> {
    fn sender_valid(&self) -> bool {
        self.c.sender.console || self.c.rt.online_player(self.c.sender.uuid).is_some()
    }

    fn session_state(&self, id: &str) -> Option<bool> {
        lock_sessions().get(id).map(|s| s.completed)
    }

    fn is_trusted(&mut self, hash: &str) -> bool {
        if self.c.sender.console { sock::console_trusted(&Self::keystore(), hash) } else { self.user_key_is(hash) }
    }

    fn trust(&mut self, hash: &str) {
        if self.c.sender.console {
            if let Err(e) = sock::console_trust(&Self::keystore(), hash) {
                eprintln!("[LuckPerms] could not save the trusted editor key: {e}");
            }
            return;
        }
        // `WebEditorKeystore.trust(User)`: the player's previous key goes, the new one is saved as a meta node. A
        // player who is not loaded cannot trust (`getIfLoaded`).
        let uuid = self.c.sender.uuid;
        if !self.c.model().users.contains_key(&uuid) {
            return;
        }
        let Some(mut l) = self.c.load_user(uuid) else { return };
        l.h.data_mut().nodes.retain(|n| !matches!(&n.kind, Kind::Meta(k, _) if k == META_KEY));
        l.h.data_mut().nodes.push(Node::permanent(key::meta(META_KEY, hash), true, ContextSet::new()));
        self.c.save(l);
    }

    fn notify(&mut self, n: Notice) {
        let c = &mut *self.c;
        match n {
            Notice::Connected => c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_SOCKET_CONNECTED, 'b', &[]))),
            Notice::Reconnected => c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_SOCKET_RECONNECTED, '7', &[]))),
            Notice::ChangesReceived => c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_SOCKET_CHANGES_RECEIVED, '7', &[]))),
            Notice::Untrusted { nonce, browser } => {
                c.msg(format!("{}.", tr(lang::T_COMMAND_EDITOR_SOCKET_UNTRUSTED, 'b', &[])));
                c.msg(format!("\u{a7}8({}\u{a7}8)", tr(lang::T_COMMAND_EDITOR_SOCKET_UNTRUSTED_SESSIONINFO, '7', &[format!("\u{a7}f{nonce}"), format!("\u{a7}f{browser}")])));
                let command = format!("/{LABEL} trusteditor {nonce}");
                if c.sender.console {
                    c.msg(tr(lang::T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_RUNCOMMAND, '7', &[format!("\u{a7}a{command}")]));
                } else {
                    let click = rich(&format!("\u{a7}a{}", lang::T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_CLICK_ACTION), Some(Click::RunCommand(command)), None);
                    c.msg(tr(lang::T_COMMAND_EDITOR_SOCKET_UNTRUSTED_PROMPT_CLICK, '7', &[click]));
                }
            }
        }
    }

    fn may_apply_edits(&self) -> bool {
        self.c.has("applyedits")
    }

    /// `HandlerChangeRequest`: download, apply, then the follow-up session.
    fn change_request(&mut self, code: &str) -> Result<Option<String>, String> {
        let url = self.c.cfg.bytebin_url.clone();
        let body = bytebin::with_client(&url, |b| b.get(code)).map_err(|e| match e {
            HttpError::Status(status, _) => format!("Error reading data (the paste service answered {status})"),
            HttpError::Other(_) => "Error reading data".to_owned(),
        })?;
        let data = match serde_json::from_slice::<Value>(&body) {
            Ok(v @ Value::Object(_)) => v,
            _ => return Err("Error reading data".into()),
        };
        apply_response(self.c, code, &data, "lp", false);
        update_set(self.c, self.holders, self.tracks, &data);
        Ok(follow_up(self.c, self.holders, self.tracks))
    }
}

fn entries<'a>(data: &'a Value, key: &str) -> &'a [Value] {
    data.get(key).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

/// What the session shows after the editor's changes: groups and tracks it deleted go, ones it created join
/// (`excludeDeleted*`, `includeCreated*`); a deleted user leaves too.
fn update_set(c: &Ctx<'_>, holders: &mut Vec<HolderKey>, tracks: &mut Vec<String>, data: &Value) {
    for v in entries(data, "userDeletions") {
        if let Some(u) = v.as_str().and_then(uuid::parse) {
            holders.retain(|h| *h != HolderKey::User(u));
        }
    }
    for v in entries(data, "groupDeletions") {
        if let Some(name) = v.as_str().map(str::to_lowercase) {
            if !c.model().groups.contains_key(&name) {
                holders.retain(|h| *h != HolderKey::Group(name.clone()));
            }
        }
    }
    for v in entries(data, "trackDeletions") {
        if let Some(name) = v.as_str().map(str::to_lowercase) {
            if !c.model().tracks.contains_key(&name) {
                tracks.retain(|t| *t != name);
            }
        }
    }
    for change in entries(data, "changes") {
        let (Some(ty), Some(id)) = (change.get("type").and_then(Value::as_str), change.get("id").and_then(Value::as_str)) else { continue };
        let name = id.to_lowercase();
        match ty {
            "group" if c.model().groups.contains_key(&name) && !holders.contains(&HolderKey::Group(name.clone())) => holders.push(HolderKey::Group(name)),
            "track" if c.model().tracks.contains_key(&name) && !tracks.contains(&name) => tracks.push(name),
            _ => {}
        }
    }
}

/// `WebEditorSession.createFollowUpSession`: the same holders and tracks as they are now, uploaded again.
fn follow_up(c: &mut Ctx<'_>, keys: &[HolderKey], track_names: &[String]) -> Option<String> {
    let mut holders: Vec<EditorHolder> = Vec::new();
    for k in keys {
        match k {
            HolderKey::Group(name) => {
                let group = c.model().groups.get(name).cloned();
                holders.extend(group.map(|g| group_holder(&g)));
            }
            HolderKey::User(u) => {
                if let Some(l) = c.load_user(*u) {
                    if let Holder::User(user) = l.h {
                        holders.push(user_holder(&user));
                    }
                }
            }
        }
    }
    let tracks: Vec<Track> = track_names.iter().filter_map(|n| c.model().tracks.get(n).cloned()).collect();
    let payload = prepare(c, &holders, &tracks)?;
    post(c, payload, &holders, None)
}
