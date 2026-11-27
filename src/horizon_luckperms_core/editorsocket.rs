//! The web editor's live connection (`WebEditorSocket`, `WebEditorSocketListener`, the `Handler*` classes,
//! `SignatureAlgorithm`, `WebEditorKeystore`, `BytesocksClient`): `/lp editor` opens a channel on a bytesocks
//! service, puts its id and this server's public key into the editor request, and the editor then talks to the
//! server over that channel: it says hello, the player trusts it with `/lp trusteditor <id>` the first time, and
//! from then on it can send its changes (`change-request`) without the player copying `/lp applyedits <code>`.
//!
//! Messages are JSON frames `{"msg": "<json>", "signature": "<base64>"}`; the signature is ECDSA P-256 with SHA-256
//! in the fixed 64-byte (P1363) form, `SignatureAlgorithm.V2_ECDSA`; the public keys travel as X.509
//! SubjectPublicKeyInfo in base64. The first connection of a browser is untrusted until the player runs
//! `/lp trusteditor <nonce>`; a trusted key is remembered (as a `lp-editor-key` meta node of the player, or in
//! `editor-keystore.json` for the console), as a SHA-1 hash of the key.
//!
//! Layout: [`SocketState`] is the protocol, with no I/O (it asks an [`Env`] for the few things that need the
//! server: sessions, trust, applying changes), so it is tested with a simulated editor. The transport thread
//! of a socket only moves frames between the websocket and the worker thread. The registry keeps one socket per
//! player (a new `/lp editor` replaces the old socket).
//!
//! Security: the frames of an editor that is not the one the player trusted are ignored (the signature has to
//! verify against the key that said hello and was trusted); nonces and codes are checked against the characters
//! the real ones use; text from the editor that is shown in chat (the browser name) is stripped of control
//! characters and colour codes and cut short; frames are bounded.

use crate::horizon_luckperms_core::bytebin::valid_key;
use crate::horizon_luckperms_core::netio;
use crate::horizon_luckperms_core::storage::HolderKey;
use base64::Engine as _;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, KeyPair, UnparsedPublicKey};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// `SignatureAlgorithm.V2_ECDSA.protocolVersion()`
pub const PROTOCOL_VERSION: u32 = 2;
/// A frame from the editor is a few hundred bytes; nothing this big is real.
const MAX_FRAME: usize = 256 * 1024;
/// Attempted connections kept per socket (a nonce each).
const MAX_ATTEMPTS: usize = 16;
const MAX_NONCE: usize = 64;

fn b64() -> base64::engine::GeneralPurpose {
    base64::engine::general_purpose::STANDARD
}

fn log(line: &str) {
    eprintln!("[LuckPerms] {line}");
}

// ---- keys ------------------------------------------------------------------------------------

/// The DER header of an X.509 SubjectPublicKeyInfo for a P-256 key; the 65-byte uncompressed point follows.
const SPKI_PREFIX: [u8; 26] = [
    0x30, 0x59, 0x30, 0x13, 0x06, 0x07, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x02, 0x01, 0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07, 0x03, 0x42, 0x00,
];

/// The point inside a P-256 SubjectPublicKeyInfo, or None when the bytes are anything else.
fn point_of(spki: &[u8]) -> Option<&[u8]> {
    (spki.len() == SPKI_PREFIX.len() + 65 && spki.starts_with(&SPKI_PREFIX) && spki[SPKI_PREFIX.len()] == 0x04).then(|| &spki[SPKI_PREFIX.len()..])
}

/// This server's key pair for the editor connections.
pub struct Keys {
    pair: EcdsaKeyPair,
    spki: Vec<u8>,
    rng: SystemRandom,
}

impl Keys {
    pub fn generate() -> Result<Keys, String> {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, &rng).map_err(|_| "could not generate the editor key".to_string())?;
        let pair = EcdsaKeyPair::from_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng).map_err(|_| "could not load the editor key".to_string())?;
        let mut spki = SPKI_PREFIX.to_vec();
        spki.extend_from_slice(pair.public_key().as_ref());
        Ok(Keys { pair, spki, rng })
    }

    /// `Base64.getEncoder().encodeToString(publicKey.getEncoded())`
    pub fn public_base64(&self) -> String {
        b64().encode(&self.spki)
    }

    /// `SignatureAlgorithm.sign`: base64 of the 64-byte signature.
    pub fn sign(&self, msg: &str) -> Result<String, String> {
        let sig = self.pair.sign(&self.rng, msg.as_bytes()).map_err(|_| "could not sign".to_string())?;
        Ok(b64().encode(sig.as_ref()))
    }
}

/// The key of this server, made when first needed (LuckPerms makes it at start, or lazily with
/// `editor-lazily-generate-key`; either way it lives as long as the process).
pub fn server_keys() -> Result<Arc<Keys>, String> {
    static KEYS: OnceLock<Result<Arc<Keys>, String>> = OnceLock::new();
    KEYS.get_or_init(|| Keys::generate().map(Arc::new)).clone()
}

/// `SignatureAlgorithm.parsePublicKey`: the SubjectPublicKeyInfo bytes of a base64 key.
pub fn parse_public_key(base64: &str) -> Result<Vec<u8>, String> {
    let bytes = b64().decode(base64.trim()).map_err(|_| "the public key is not base64".to_string())?;
    if point_of(&bytes).is_none() {
        return Err("the public key is not a P-256 key".into());
    }
    Ok(bytes)
}

/// `SignatureAlgorithm.verify`
pub fn verify(spki: &[u8], msg: &str, signature_base64: &str) -> bool {
    let Some(point) = point_of(spki) else { return false };
    let Ok(sig) = b64().decode(signature_base64.trim()) else { return false };
    UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_FIXED, point).verify(msg.as_bytes(), &sig).is_ok()
}

/// `WebEditorKeystore.hash`: base64 of the SHA-1 of the encoded key.
pub fn key_hash(spki: &[u8]) -> String {
    let mut h = Sha1::new();
    h.update(spki);
    b64().encode(h.finalize())
}

// ---- the keystore of the console ---------------------------------------------------------------

/// The file of the console's trusted keys, in the data directory.
pub const KEYSTORE_FILE: &str = "editor-keystore.json";
const MAX_KEYS: usize = 200;

fn read_keystore(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let Ok(v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    v.get("consoleKeys").and_then(Value::as_array).map(|a| a.iter().filter_map(|k| k.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

pub fn console_trusted(file: &Path, hash: &str) -> bool {
    read_keystore(file).iter().any(|k| k == hash)
}

pub fn console_trust(file: &Path, hash: &str) -> Result<(), String> {
    let mut keys = read_keystore(file);
    if keys.iter().any(|k| k == hash) {
        return Ok(());
    }
    keys.push(hash.to_owned());
    if keys.len() > MAX_KEYS {
        keys.remove(0);
    }
    let body = json!({"_comment": "This file stores a list of trusted editor public keys", "consoleKeys": keys});
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    std::fs::write(file, serde_json::to_string_pretty(&body).unwrap_or_default()).map_err(|e| format!("{KEYSTORE_FILE}: {}", e.kind()))
}

// ---- the protocol --------------------------------------------------------------------------------

/// Who opened the editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    pub uuid: u128,
    pub name: String,
    pub console: bool,
}

/// What the player is told.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    Connected,
    Reconnected,
    ChangesReceived,
    /// A new browser connected: the nonce to trust it with and the browser name it gave (cleaned).
    Untrusted { nonce: String, browser: String },
}

/// What the protocol needs from the server.
pub trait Env {
    /// The sender is still there (a player online, or the console).
    fn sender_valid(&self) -> bool;
    /// `Some(completed)` for a session this server handed out.
    fn session_state(&self, session_id: &str) -> Option<bool>;
    fn is_trusted(&mut self, key_hash: &str) -> bool;
    fn trust(&mut self, key_hash: &str);
    fn notify(&mut self, notice: Notice);
    /// The sender may apply edits (`luckperms.applyedits`).
    fn may_apply_edits(&self) -> bool;
    /// Downloads the editor's reply under `code`, applies it, and returns the code of the follow-up session.
    fn change_request(&mut self, code: &str) -> Result<Option<String>, String>;
}

/// `SocketMessageType`
fn kind_of(id: &str) -> Option<Kind> {
    Some(match id {
        "hello" => Kind::Hello,
        "connected" => Kind::Connected,
        "change-request" => Kind::ChangeRequest,
        "ping" => Kind::Ping,
        _ => return None,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Hello,
    Connected,
    ChangeRequest,
    Ping,
}

fn text<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

/// A nonce the editor made: letters, digits, `_` and `-`, bounded.
fn valid_nonce(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NONCE && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Text from the editor that is shown in chat: no control characters or colour codes, at most 100 characters.
pub fn clean_text(s: &str) -> String {
    s.chars().filter(|c| !c.is_control() && *c != '\u{a7}' && *c != '\u{e000}' && *c != '\u{e001}' && *c != '\u{e002}').take(100).collect()
}

pub struct SocketState {
    pub who: Who,
    /// Distinguishes this socket from an earlier one of the same player.
    pub generation: u64,
    keys: Arc<Keys>,
    /// The key of the connected editor (encoded).
    remote: Option<Vec<u8>>,
    /// Keys that said hello and are not trusted yet, by nonce.
    attempts: BTreeMap<String, Vec<u8>>,
    last_ping: Option<Instant>,
    pub closed: bool,
    outbox: Vec<String>,
    armed_at: Option<Instant>,
    keepalive: bool,
    last_check: Instant,
}

impl SocketState {
    pub fn new(who: Who, generation: u64, keys: Arc<Keys>) -> SocketState {
        SocketState { who, generation, keys, remote: None, attempts: BTreeMap::new(), last_ping: None, closed: false, outbox: Vec::new(), armed_at: None, keepalive: false, last_check: Instant::now() }
    }

    pub fn public_key(&self) -> String {
        self.keys.public_base64()
    }

    pub fn is_connected(&self) -> bool {
        self.remote.is_some()
    }

    /// `WebEditorSocket.send`: the message as JSON, signed.
    fn send(&mut self, msg: Value) {
        let encoded = msg.to_string();
        match self.keys.sign(&encoded) {
            Ok(sig) => self.outbox.push(json!({"msg": encoded, "signature": sig}).to_string()),
            Err(e) => log(&format!("could not sign an editor message: {e}")),
        }
    }

    /// The frames to put on the socket.
    pub fn take_outbox(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outbox)
    }

    fn hello_reply(&mut self, nonce: &str, state: &str) {
        self.send(json!({"type": "hello-reply", "nonce": nonce, "state": state}));
    }

    /// `WebEditorSocketListener.handleMessageFrame`. `Err` is a message the editor sent that is refused (the
    /// worker logs it, as LuckPerms does); it never carries the message's content.
    pub fn handle(&mut self, env: &mut dyn Env, frame: &str) -> Result<(), String> {
        if self.closed {
            return Ok(());
        }
        if !env.sender_valid() {
            self.close();
            return Ok(());
        }
        if frame.len() > MAX_FRAME {
            return Err("a frame is too big".into());
        }
        let frame: Value = serde_json::from_str(frame).map_err(|_| "a frame is not JSON".to_string())?;
        let inner = text(&frame, "msg").filter(|s| !s.is_empty());
        let sig = text(&frame, "signature").filter(|s| !s.is_empty());
        let (Some(inner), Some(sig)) = (inner, sig) else { return Err("Incomplete message".into()) };

        // The signature must come from the connected editor.
        let verified = self.remote.as_deref().is_some_and(|k| verify(k, inner, sig));
        let msg: Value = serde_json::from_str(inner).map_err(|_| "a message is not JSON".to_string())?;
        let ty = text(&msg, "type").ok_or("a message has no type")?;
        let Some(kind) = kind_of(ty) else { return Err("Invalid message type".into()) };

        if kind == Kind::Hello {
            return self.hello(env, &msg);
        }
        if !verified {
            return Err("Signature not accepted".into());
        }
        match kind {
            Kind::Hello => unreachable!("handled above"),
            Kind::Connected => env.notify(Notice::Connected),
            Kind::Ping => {
                self.last_ping = Some(Instant::now());
                self.send(json!({"type": "pong", "ok": true}));
            }
            Kind::ChangeRequest => {
                if !env.may_apply_edits() {
                    return Err("Sender does not have applyedits permission".into());
                }
                let code = text(&msg, "code").filter(|c| valid_key(c)).ok_or("Invalid code")?.to_owned();
                self.send(json!({"type": "change-response", "state": "accepted"}));
                env.notify(Notice::ChangesReceived);
                let new_code = env.change_request(&code)?;
                self.send(json!({"type": "change-response", "state": "applied", "newSessionCode": new_code}));
            }
        }
        Ok(())
    }

    /// `HandlerHello`
    fn hello(&mut self, env: &mut dyn Env, msg: &Value) -> Result<(), String> {
        let nonce = text(msg, "nonce").filter(|n| valid_nonce(n)).ok_or("missing nonce")?.to_owned();
        let session_id = text(msg, "sessionId").filter(|s| valid_key(s)).ok_or("missing sessionId")?.to_owned();
        let browser = clean_text(text(msg, "browser").unwrap_or(""));
        let public = parse_public_key(text(msg, "publicKey").ok_or("missing publicKey")?)?;

        // The same editor may reconnect; another one may not take over.
        if self.remote.as_ref().is_some_and(|r| *r != public) {
            self.hello_reply(&nonce, "rejected");
            return Ok(());
        }
        // The editor's session must be one this server made and has not applied yet.
        if env.session_state(&session_id) != Some(false) {
            self.hello_reply(&nonce, "invalid");
            return Ok(());
        }
        if !env.is_trusted(&key_hash(&public)) {
            self.hello_reply(&nonce, "untrusted");
            if self.attempts.len() >= MAX_ATTEMPTS {
                if let Some(first) = self.attempts.keys().next().cloned() {
                    self.attempts.remove(&first);
                }
            }
            self.attempts.insert(nonce.clone(), public);
            env.notify(Notice::Untrusted { nonce, browser });
            return Ok(());
        }
        let reconnected = self.remote.is_some();
        self.remote = Some(public);
        self.last_ping.get_or_insert_with(Instant::now);
        self.hello_reply(&nonce, "accepted");
        env.notify(if reconnected { Notice::Reconnected } else { Notice::Connected });
        Ok(())
    }

    /// `WebEditorSocket.trustConnection`: the player ran `/lp trusteditor <nonce>`.
    pub fn trust(&mut self, env: &mut dyn Env, nonce: &str) -> bool {
        if self.closed || !env.sender_valid() || self.remote.is_some() {
            return false;
        }
        let Some(key) = self.attempts.get(nonce).cloned() else { return false };
        env.trust(&key_hash(&key));
        self.remote = Some(key);
        self.last_ping.get_or_insert_with(Instant::now);
        self.send(json!({"type": "hello-reply", "nonce": nonce, "state": "trusted"}));
        true
    }

    /// `scheduleCleanupIfUnused`: the clock starts when the session link was handed out.
    pub fn arm(&mut self, now: Instant) {
        self.armed_at = Some(now);
        self.last_check = now;
    }

    /// The periodic check: true when the socket is closed (and should be dropped). An editor that has not
    /// connected a minute after the link was shown is dropped; one that has is dropped when it has not pinged for
    /// a minute (checked every ten seconds).
    pub fn tick(&mut self, now: Instant, sender_valid: bool) -> bool {
        if self.closed {
            return true;
        }
        if !sender_valid {
            self.close();
            return true;
        }
        let Some(armed) = self.armed_at else { return false };
        if !self.keepalive {
            if now.saturating_duration_since(armed) >= Duration::from_secs(60) {
                if self.remote.is_none() && self.attempts.is_empty() {
                    self.close();
                    return true;
                }
                self.keepalive = true;
                self.last_check = now;
            }
            return false;
        }
        if now.saturating_duration_since(self.last_check) >= Duration::from_secs(10) {
            self.last_check = now;
            if self.last_ping.is_none_or(|t| now.saturating_duration_since(t) > Duration::from_secs(60)) {
                self.close();
                return true;
            }
        }
        false
    }

    /// Tests: a ping that arrived at a chosen time.
    #[cfg(test)]
    fn pinged_at(&mut self, t: Instant) {
        self.last_ping = Some(t);
    }

    /// `WebEditorSocket.close`: a last `pong` with `ok: false`.
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.send(json!({"type": "pong", "ok": false}));
        self.closed = true;
    }
}

// ---- sockets of the server ---------------------------------------------------------------------------

/// What goes to the transport thread of a socket.
#[derive(Debug, PartialEq, Eq)]
pub enum Outbound {
    Text(String),
    Close,
}

/// Where the transport hands a received frame: the owner, the socket's generation, the text.
pub type FrameSink = Arc<dyn Fn(u128, u64, String) + Send + Sync>;

#[derive(Debug)]
pub enum SocketError {
    /// The service answered with this status (502 and 503 mean it is offline and are not worth a warning).
    Status(u16),
    Other(String),
    /// No service is configured (an unusable URL, or a test without a double): nothing to report.
    Disabled,
}

/// `BytesocksClient.createSocket`
pub trait Bytesocks {
    /// Creates a channel and connects to it; frames received on it go to `sink`.
    fn create(&self, owner: u128, generation: u64, sink: FrameSink) -> Result<Channel, SocketError>;
}

pub struct Channel {
    pub id: String,
    pub tx: mpsc::Sender<Outbound>,
}

/// One live socket of a player.
pub struct Socket {
    pub state: SocketState,
    pub tx: mpsc::Sender<Outbound>,
    pub channel_id: String,
    /// What the editor was shown (for the follow-up session after changes).
    pub holders: Vec<HolderKey>,
    pub tracks: Vec<String>,
}

impl Socket {
    /// Sends what the protocol queued, and closes the channel when the protocol closed.
    pub fn flush(&mut self) {
        for f in self.state.take_outbox() {
            let _ = self.tx.send(Outbound::Text(f));
        }
        if self.state.closed {
            let _ = self.tx.send(Outbound::Close);
        }
    }
}

fn sockets() -> &'static Mutex<HashMap<u128, Socket>> {
    static S: OnceLock<Mutex<HashMap<u128, Socket>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

fn lock() -> std::sync::MutexGuard<'static, HashMap<u128, Socket>> {
    sockets().lock().unwrap_or_else(|e| e.into_inner())
}

/// Registers a socket; an earlier one of the same player is closed.
pub fn put(owner: u128, socket: Socket) {
    let old = lock().insert(owner, socket);
    if let Some(mut old) = old {
        old.state.close();
        old.flush();
    }
}

/// Takes the socket out while the worker handles a frame (put it back with [`put_back`]).
pub fn take(owner: u128, generation: Option<u64>) -> Option<Socket> {
    let mut g = lock();
    if let Some(want) = generation {
        if g.get(&owner).is_some_and(|s| s.state.generation != want) {
            return None;
        }
    }
    g.remove(&owner)
}

/// Puts a socket taken with [`take`] back, unless it closed meanwhile (then its last frames are sent and it is
/// dropped).
pub fn put_back(owner: u128, mut socket: Socket) {
    socket.flush();
    if !socket.state.closed {
        lock().entry(owner).or_insert(socket);
    }
}

/// `removeSocket`, for the transport thread that saw its channel close: only the socket of that generation.
pub fn forget(owner: u128, generation: u64) {
    let mut g = lock();
    if g.get(&owner).is_some_and(|s| s.state.generation == generation) {
        g.remove(&owner);
    }
}

/// Drops a socket whose session could not be shown.
pub fn discard(owner: u128, generation: u64) {
    let taken = {
        let mut g = lock();
        if g.get(&owner).is_some_and(|s| s.state.generation == generation) { g.remove(&owner) } else { None }
    };
    if let Some(mut s) = taken {
        s.state.close();
        s.flush();
    }
}

/// Starts the clock for an unused socket, once the link was shown.
pub fn arm(owner: u128, generation: u64) {
    if let Some(s) = lock().get_mut(&owner) {
        if s.state.generation == generation {
            s.state.arm(Instant::now());
        }
    }
}

/// Closes what timed out or lost its player. `valid` says whether an owner is still there.
pub fn tick(valid: &dyn Fn(u128) -> bool) {
    let now = Instant::now();
    let mut closed = Vec::new();
    {
        let mut g = lock();
        let owners: Vec<u128> = g.keys().copied().collect();
        for o in owners {
            let Some(s) = g.get_mut(&o) else { continue };
            if s.state.tick(now, valid(o)) {
                closed.push(o);
            }
        }
        for o in &closed {
            if let Some(mut s) = g.remove(o) {
                s.flush();
            }
        }
    }
}

/// How many sockets are open (tests, `info`).
pub fn open_count() -> usize {
    lock().len()
}

/// What the editor request needs of a socket that was opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub channel_id: String,
    pub public_key: String,
    pub generation: u64,
}

impl Info {
    /// `appendDetailToRequest`: the `socket` object of the editor request.
    pub fn detail(&self) -> Value {
        json!({"protocolVersion": PROTOCOL_VERSION, "channelId": self.channel_id, "publicKey": self.public_key})
    }
}

static GENERATION: AtomicU64 = AtomicU64::new(1);

/// `WebEditorSession.createSocket`: opens a channel for `who` and registers the socket. None when the service is
/// unreachable (the editor then shows the `/lp applyedits` command to copy, as LuckPerms does).
pub fn open(who: Who, holders: Vec<HolderKey>, tracks: Vec<String>, bytesocks_url: &str, sink: FrameSink) -> Option<Info> {
    let keys = match server_keys() {
        Ok(k) => k,
        Err(e) => {
            log(&format!("Unable to establish socket connection: {e}"));
            return None;
        }
    };
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    let created = with_bytesocks(bytesocks_url, |b| match b {
        Some(b) => b.create(who.uuid, generation, sink),
        None => Err(SocketError::Disabled),
    });
    let channel = match created {
        Ok(c) => c,
        Err(SocketError::Status(502 | 503) | SocketError::Disabled) => return None,
        Err(SocketError::Status(code)) => {
            log(&format!("Unable to establish socket connection (response code {code})"));
            return None;
        }
        Err(SocketError::Other(e)) => {
            log(&format!("Unable to establish socket connection: {e}"));
            return None;
        }
    };
    let owner = who.uuid;
    let state = SocketState::new(who, generation, keys);
    let info = Info { channel_id: channel.id.clone(), public_key: state.public_key(), generation };
    put(owner, Socket { state, tx: channel.tx, channel_id: channel.id, holders, tracks });
    Some(info)
}

// ---- bytesocks over HTTP and websocket -------------------------------------------------------------------

/// The real service: `GET <url>create` answers 201 with the channel id in `Location`; the channel is the
/// websocket `<url>` with `http` turned into `ws`, plus the id.
pub struct HttpBytesocks {
    url: String,
}

impl HttpBytesocks {
    pub fn new(url: &str) -> Result<HttpBytesocks, String> {
        let url = if url.ends_with('/') { url.to_owned() } else { format!("{url}/") };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err("Invalid URL".into());
        }
        Ok(HttpBytesocks { url })
    }

    fn ws_url(&self) -> String {
        match self.url.strip_prefix("https://") {
            Some(rest) => format!("wss://{rest}"),
            None => format!("ws://{}", self.url.strip_prefix("http://").unwrap_or(&self.url)),
        }
    }
}

impl Bytesocks for HttpBytesocks {
    fn create(&self, owner: u128, generation: u64, sink: FrameSink) -> Result<Channel, SocketError> {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(15)).redirects(0).build();
        let response = match agent.get(&format!("{}create", self.url)).set("User-Agent", "luckperms/editor").call() {
            Ok(r) => r,
            Err(ureq::Error::Status(code, _)) => return Err(SocketError::Status(code)),
            Err(e) => return Err(SocketError::Other(e.to_string())),
        };
        if response.status() != 201 {
            return Err(SocketError::Status(response.status()));
        }
        let id = response.header("Location").map(str::to_owned).ok_or_else(|| SocketError::Other("no channel id in the answer".into()))?;
        if !valid_key(&id) {
            return Err(SocketError::Other("the service returned an unusable channel id".into()));
        }
        // `waitForConnect(5, SECONDS)`: the connection is made here, then the thread takes it over.
        let conn = netio::ws_connect(&format!("{}{id}", self.ws_url())).map_err(SocketError::Other)?;
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("luckperms-editor-socket".into())
            .spawn(move || transport(conn, rx, owner, generation, sink))
            .map_err(|e| SocketError::Other(e.to_string()))?;
        Ok(Channel { id, tx })
    }
}

/// Moves frames between the websocket and the worker thread until either side closes.
fn transport(mut conn: netio::Connection, rx: mpsc::Receiver<Outbound>, owner: u128, generation: u64, sink: FrameSink) {
    use tungstenite::Message;
    let _ = conn.tcp.set_read_timeout(Some(Duration::from_millis(250)));
    loop {
        loop {
            match rx.try_recv() {
                Ok(Outbound::Text(t)) => {
                    if conn.socket.send(Message::text(t)).is_err() {
                        forget(owner, generation);
                        return;
                    }
                }
                Ok(Outbound::Close) | Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = conn.socket.close(None);
                    let _ = conn.socket.flush();
                    return;
                }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        match conn.socket.read() {
            Ok(Message::Text(t)) => {
                if t.len() <= MAX_FRAME {
                    sink(owner, generation, t.to_string());
                }
            }
            Ok(Message::Close(_)) => {
                forget(owner, generation);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => {
                forget(owner, generation);
                return;
            }
        }
    }
}

thread_local! {
    /// Tests put a fake service here (no network in tests).
    static OVERRIDE: std::cell::RefCell<Option<std::rc::Rc<dyn Bytesocks>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
pub fn set_test_bytesocks(b: Option<std::rc::Rc<dyn Bytesocks>>) {
    OVERRIDE.with(|o| *o.borrow_mut() = b);
}

/// Runs `f` with the service at `url`: the test double when one is set, none at all in a test without one
/// (so the editor tests never reach the network), else the real one (None when the URL is unusable).
fn with_bytesocks<R>(url: &str, f: impl FnOnce(Option<&dyn Bytesocks>) -> R) -> R {
    let fake = OVERRIDE.with(|o| o.borrow().clone());
    match fake {
        Some(b) => f(Some(&*b)),
        None => default_service(url, f),
    }
}

#[cfg(test)]
fn default_service<R>(_url: &str, f: impl FnOnce(Option<&dyn Bytesocks>) -> R) -> R {
    f(None)
}

#[cfg(not(test))]
fn default_service<R>(url: &str, f: impl FnOnce(Option<&dyn Bytesocks>) -> R) -> R {
    match HttpBytesocks::new(url) {
        Ok(b) => f(Some(&b)),
        Err(_) => f(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    // ---- a simulated editor ----

    struct Editor {
        keys: Keys,
    }

    impl Editor {
        fn new() -> Editor {
            Editor { keys: Keys::generate().unwrap() }
        }

        fn frame(&self, msg: Value) -> String {
            let inner = msg.to_string();
            json!({"msg": inner, "signature": self.keys.sign(&inner).unwrap()}).to_string()
        }

        fn hello(&self, nonce: &str, session: &str) -> String {
            self.frame(json!({"type": "hello", "nonce": nonce, "sessionId": session, "browser": "Firefox on Linux", "publicKey": self.keys.public_base64()}))
        }
    }

    /// What the server sent, with the signature checked against the server's key.
    fn sent(state: &mut SocketState) -> Vec<Value> {
        let server = state.public_key();
        let spki = parse_public_key(&server).unwrap();
        state
            .take_outbox()
            .into_iter()
            .map(|f| {
                let v: Value = serde_json::from_str(&f).unwrap();
                let inner = v["msg"].as_str().unwrap();
                assert!(verify(&spki, inner, v["signature"].as_str().unwrap()), "the server signs what it sends");
                serde_json::from_str(inner).unwrap()
            })
            .collect()
    }

    #[derive(Default)]
    struct FakeEnv {
        valid: Option<bool>,
        sessions: HashMap<String, bool>,
        trusted: Vec<String>,
        notices: Vec<Notice>,
        may_apply: bool,
        applied: Vec<String>,
        follow_up: Option<String>,
        fail_apply: bool,
    }

    impl Env for FakeEnv {
        fn sender_valid(&self) -> bool {
            self.valid.unwrap_or(true)
        }
        fn session_state(&self, id: &str) -> Option<bool> {
            self.sessions.get(id).copied()
        }
        fn is_trusted(&mut self, hash: &str) -> bool {
            self.trusted.iter().any(|t| t == hash)
        }
        fn trust(&mut self, hash: &str) {
            self.trusted.push(hash.to_owned());
        }
        fn notify(&mut self, n: Notice) {
            self.notices.push(n);
        }
        fn may_apply_edits(&self) -> bool {
            self.may_apply
        }
        fn change_request(&mut self, code: &str) -> Result<Option<String>, String> {
            if self.fail_apply {
                return Err("Error reading data".into());
            }
            self.applied.push(code.to_owned());
            Ok(self.follow_up.clone())
        }
    }

    fn state() -> SocketState {
        SocketState::new(Who { uuid: 0xa11ce, name: "Alice".into(), console: false }, 1, Arc::new(Keys::generate().unwrap()))
    }

    fn env() -> FakeEnv {
        let mut e = FakeEnv { may_apply: true, follow_up: Some("newcode123".into()), ..Default::default() };
        e.sessions.insert("sess1".into(), false);
        e.sessions.insert("done1".into(), true);
        e
    }

    // ---- crypto ----

    #[test]
    fn keys_sign_verify_and_encode_like_java() {
        let k = Keys::generate().unwrap();
        let spki = parse_public_key(&k.public_base64()).unwrap();
        assert_eq!(spki.len(), 91, "an X.509 SubjectPublicKeyInfo of a P-256 key");
        assert_eq!(&spki[..4], &[0x30, 0x59, 0x30, 0x13]);
        assert_eq!(spki[26], 0x04, "uncompressed point");
        let sig = k.sign("{\"type\":\"pong\"}").unwrap();
        assert_eq!(b64().decode(&sig).unwrap().len(), 64, "the fixed r||s form (P1363) of SHA256withECDSAinP1363Format");
        assert!(verify(&spki, "{\"type\":\"pong\"}", &sig));
        assert!(!verify(&spki, "{\"type\":\"ping\"}", &sig));
        assert!(!verify(&spki, "{\"type\":\"pong\"}", "not base64!"));
        let other = Keys::generate().unwrap();
        assert!(!verify(&parse_public_key(&other.public_base64()).unwrap(), "{\"type\":\"pong\"}", &sig));
        assert!(!verify(&[1, 2, 3], "x", &sig));
    }

    #[test]
    fn public_keys_that_are_not_p256_are_refused() {
        assert!(parse_public_key("###").is_err());
        assert!(parse_public_key(&b64().encode([0u8; 91])).is_err());
        assert!(parse_public_key(&b64().encode(vec![0x30u8; 294])).is_err(), "an RSA-sized key");
        let k = Keys::generate().unwrap();
        let mut spki = parse_public_key(&k.public_base64()).unwrap();
        spki[26] = 0x02;
        assert!(parse_public_key(&b64().encode(&spki)).is_err(), "compressed points are not what the editor sends");
    }

    #[test]
    fn key_hashes_are_base64_sha1() {
        // SHA-1("abc") = a9993e364706816aba3e25717850c26c9cd0d89d
        assert_eq!(key_hash(b"abc"), "qZk+NkcGgWq6PiVxeFDCbJzQ2J0=");
    }

    // ---- the protocol ----

    #[test]
    fn an_unknown_editor_is_untrusted_until_the_player_trusts_its_nonce() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        s.handle(&mut e, &ed.hello("nonce-1", "sess1")).unwrap();
        assert_eq!(sent(&mut s)[0], json!({"type": "hello-reply", "nonce": "nonce-1", "state": "untrusted"}));
        assert_eq!(e.notices, vec![Notice::Untrusted { nonce: "nonce-1".into(), browser: "Firefox on Linux".into() }]);
        assert!(!s.is_connected());
        // Frames are ignored until it is trusted (its signature has no key to be checked against).
        assert_eq!(s.handle(&mut e, &ed.frame(json!({"type": "ping"}))), Err("Signature not accepted".into()));
        // Trusting a nonce nobody used, or twice, fails; the right one works.
        assert!(!s.trust(&mut e, "other"));
        assert!(s.trust(&mut e, "nonce-1"));
        assert_eq!(sent(&mut s)[0], json!({"type": "hello-reply", "nonce": "nonce-1", "state": "trusted"}));
        assert_eq!(e.trusted, vec![key_hash(&parse_public_key(&ed.keys.public_base64()).unwrap())]);
        assert!(!s.trust(&mut e, "nonce-1"), "a connected socket is not trusted again");
        // Now its signed messages are accepted.
        s.handle(&mut e, &ed.frame(json!({"type": "ping"}))).unwrap();
        assert_eq!(sent(&mut s), vec![json!({"type": "pong", "ok": true})]);
        s.handle(&mut e, &ed.frame(json!({"type": "connected"}))).unwrap();
        assert_eq!(e.notices.last(), Some(&Notice::Connected));
    }

    #[test]
    fn a_trusted_key_connects_and_reconnects_but_nobody_else_takes_over() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        e.trusted.push(key_hash(&parse_public_key(&ed.keys.public_base64()).unwrap()));
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        assert_eq!(sent(&mut s)[0]["state"], "accepted");
        assert_eq!(e.notices, vec![Notice::Connected]);
        s.handle(&mut e, &ed.hello("n2", "sess1")).unwrap();
        assert_eq!(sent(&mut s)[0]["state"], "accepted");
        assert_eq!(e.notices[1], Notice::Reconnected);
        // A second editor (another key) is rejected, even a trusted one.
        let intruder = Editor::new();
        e.trusted.push(key_hash(&parse_public_key(&intruder.keys.public_base64()).unwrap()));
        s.handle(&mut e, &intruder.hello("n3", "sess1")).unwrap();
        assert_eq!(sent(&mut s)[0]["state"], "rejected");
        assert_eq!(s.handle(&mut e, &intruder.frame(json!({"type": "ping"}))), Err("Signature not accepted".into()));
    }

    #[test]
    fn a_session_that_is_unknown_or_already_applied_is_invalid() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        s.handle(&mut e, &ed.hello("n1", "nosuch")).unwrap();
        assert_eq!(sent(&mut s)[0]["state"], "invalid");
        s.handle(&mut e, &ed.hello("n2", "done1")).unwrap();
        assert_eq!(sent(&mut s)[0]["state"], "invalid");
        assert!(e.notices.is_empty());
    }

    #[test]
    fn a_change_request_downloads_applies_and_answers_with_the_next_session() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        e.trusted.push(key_hash(&parse_public_key(&ed.keys.public_base64()).unwrap()));
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        s.take_outbox();
        s.handle(&mut e, &ed.frame(json!({"type": "change-request", "code": "Ab12_x-9"}))).unwrap();
        let out = sent(&mut s);
        assert_eq!(out[0], json!({"type": "change-response", "state": "accepted"}));
        assert_eq!(out[1], json!({"type": "change-response", "state": "applied", "newSessionCode": "newcode123"}));
        assert_eq!(e.applied, vec!["Ab12_x-9"]);
        assert_eq!(e.notices.last(), Some(&Notice::ChangesReceived));
        // Without the permission nothing is applied.
        e.may_apply = false;
        assert_eq!(s.handle(&mut e, &ed.frame(json!({"type": "change-request", "code": "Ab12"}))), Err("Sender does not have applyedits permission".into()));
        assert_eq!(e.applied.len(), 1);
        // A code that is no bytebin key never reaches the download.
        e.may_apply = true;
        for bad in [json!(""), json!("a/b"), json!("../x"), json!(5), Value::Null] {
            assert!(s.handle(&mut e, &ed.frame(json!({"type": "change-request", "code": bad}))).is_err());
        }
        assert_eq!(e.applied.len(), 1);
        // A failed download is an error, and no `applied` is sent.
        s.take_outbox();
        e.fail_apply = true;
        assert!(s.handle(&mut e, &ed.frame(json!({"type": "change-request", "code": "okcode"}))).is_err());
        assert_eq!(sent(&mut s).len(), 1, "only the accepted");
    }

    #[test]
    fn bad_frames_are_refused_without_echoing_them() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        for (frame, why) in [
            ("not json".to_owned(), "a frame is not JSON"),
            ("{}".to_owned(), "Incomplete message"),
            (json!({"msg": "", "signature": "x"}).to_string(), "Incomplete message"),
            (json!({"msg": "{}", "signature": ""}).to_string(), "Incomplete message"),
            (json!({"msg": "not json", "signature": "x"}).to_string(), "a message is not JSON"),
            (json!({"msg": "{\"type\":\"nope\"}", "signature": "x"}).to_string(), "Invalid message type"),
            (ed.frame(json!({"type": "hello", "nonce": "bad nonce!", "sessionId": "sess1", "publicKey": ed.keys.public_base64()})), "missing nonce"),
            (ed.frame(json!({"type": "hello", "nonce": "n", "sessionId": "../x", "publicKey": ed.keys.public_base64()})), "missing sessionId"),
            (ed.frame(json!({"type": "hello", "nonce": "n", "sessionId": "sess1", "publicKey": "AAAA"})), "the public key is not a P-256 key"),
            ("x".repeat(MAX_FRAME + 1), "a frame is too big"),
        ] {
            assert_eq!(s.handle(&mut e, &frame), Err(why.to_owned()), "{why}");
        }
        assert!(s.take_outbox().is_empty());
    }

    #[test]
    fn the_browser_name_cannot_inject_codes_into_chat() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        let hello = ed.frame(json!({"type": "hello", "nonce": "n1", "sessionId": "sess1", "browser": format!("\u{a7}cEvil\u{a7}k\n{}", "x".repeat(300)), "publicKey": ed.keys.public_base64()}));
        s.handle(&mut e, &hello).unwrap();
        let Notice::Untrusted { browser, .. } = &e.notices[0] else { panic!() };
        assert!(!browser.contains('\u{a7}') && !browser.contains('\n'));
        assert_eq!(browser.chars().count(), 100);
        assert!(browser.starts_with("cEvilk"));
    }

    #[test]
    fn a_sender_that_left_closes_the_socket() {
        let (mut s, mut e, ed) = (state(), env(), Editor::new());
        e.valid = Some(false);
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        assert!(s.closed);
        assert_eq!(sent(&mut s)[0], json!({"type": "pong", "ok": false}));
        // Closed sockets ignore everything and cannot trust.
        e.valid = Some(true);
        s.handle(&mut e, &ed.hello("n2", "sess1")).unwrap();
        assert!(s.take_outbox().is_empty());
        assert!(!s.trust(&mut e, "n1"));
    }

    #[test]
    fn unused_sockets_are_closed_after_a_minute_and_silent_editors_after_another() {
        let ed = Editor::new();
        let t0 = Instant::now();
        // Never connected.
        let mut s = state();
        assert!(!s.tick(t0 + Duration::from_secs(1000), true), "the clock only runs once armed");
        s.arm(t0);
        assert!(!s.tick(t0 + Duration::from_secs(59), true));
        assert!(s.tick(t0 + Duration::from_secs(61), true));
        assert_eq!(sent(&mut s)[0], json!({"type": "pong", "ok": false}));
        // An editor that said hello (untrusted) keeps the socket past the first minute, then needs pings.
        let (mut s, mut e) = (state(), env());
        s.arm(t0);
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        assert!(!s.tick(t0 + Duration::from_secs(61), true));
        assert!(!s.tick(t0 + Duration::from_secs(65), true), "keepalive is checked every ten seconds");
        // Nobody trusted it and nothing pinged for over a minute: closed at the next check.
        assert!(s.tick(t0 + Duration::from_secs(200), true));
        // Silence for over a minute closes it at the next check.
        let (mut s, mut e) = (state(), env());
        e.trusted.push(key_hash(&parse_public_key(&ed.keys.public_base64()).unwrap()));
        s.arm(t0);
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        assert!(!s.tick(t0 + Duration::from_secs(61), true));
        assert!(s.tick(t0 + Duration::from_secs(72), true), "no ping for over a minute");
        // Pings keep it alive.
        let (mut s, mut e) = (state(), env());
        e.trusted.push(key_hash(&parse_public_key(&ed.keys.public_base64()).unwrap()));
        s.arm(t0);
        s.handle(&mut e, &ed.hello("n1", "sess1")).unwrap();
        assert!(!s.tick(t0 + Duration::from_secs(61), true));
        s.pinged_at(t0 + Duration::from_secs(70));
        assert!(!s.tick(t0 + Duration::from_secs(72), true), "pinged moments ago");
        // A player who left closes it at once.
        assert!(s.tick(t0 + Duration::from_secs(73), false));
    }

    // ---- the keystore of the console ----

    #[test]
    fn the_console_keystore_is_a_json_file() {
        let dir = std::env::temp_dir().join(format!("lp-keystore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join(KEYSTORE_FILE);
        assert!(!console_trusted(&file, "abc="));
        console_trust(&file, "abc=").unwrap();
        console_trust(&file, "abc=").unwrap();
        console_trust(&file, "def=").unwrap();
        assert!(console_trusted(&file, "abc=") && console_trusted(&file, "def="));
        assert!(!console_trusted(&file, "ghi="));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["consoleKeys"], json!(["abc=", "def="]));
        assert!(v["_comment"].as_str().unwrap().contains("trusted editor public keys"));
        std::fs::write(&file, "garbage").unwrap();
        assert!(!console_trusted(&file, "abc="), "an unreadable keystore trusts nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- the registry, with a fake service ----

    struct FakeService {
        made: RefCell<Vec<(u128, u64, mpsc::Receiver<Outbound>)>>,
        fail: RefCell<Option<SocketError>>,
    }

    impl Bytesocks for FakeService {
        fn create(&self, owner: u128, generation: u64, _sink: FrameSink) -> Result<Channel, SocketError> {
            if let Some(e) = self.fail.borrow_mut().take() {
                return Err(e);
            }
            let (tx, rx) = mpsc::channel();
            self.made.borrow_mut().push((owner, generation, rx));
            Ok(Channel { id: format!("chan{}", self.made.borrow().len()), tx })
        }
    }

    fn sink() -> FrameSink {
        Arc::new(|_, _, _| {})
    }

    #[test]
    fn opening_registers_a_socket_and_a_second_one_replaces_the_first() {
        let svc = Rc::new(FakeService { made: RefCell::new(Vec::new()), fail: RefCell::new(None) });
        set_test_bytesocks(Some(svc.clone()));
        let owner = 0x5eed_0001;
        let who = Who { uuid: owner, name: "Alice".into(), console: false };
        let a = open(who.clone(), vec![HolderKey::Group("default".into())], vec![], "https://sockets.example/", sink()).unwrap();
        assert_eq!(a.channel_id, "chan1");
        assert_eq!(a.detail()["protocolVersion"], 2);
        assert_eq!(a.detail()["channelId"], "chan1");
        assert!(parse_public_key(a.detail()["publicKey"].as_str().unwrap()).is_ok());
        let b = open(who, vec![], vec![], "https://sockets.example/", sink()).unwrap();
        assert!(b.generation > a.generation);
        // The first channel was closed with a last pong; the second is the registered one.
        {
            let made = svc.made.borrow();
            let first: Vec<Outbound> = made[0].2.try_iter().collect();
            assert_eq!(first.len(), 2);
            assert!(matches!(&first[0], Outbound::Text(t) if t.contains("pong")));
            assert_eq!(first[1], Outbound::Close);
        }
        assert!(take(owner, Some(a.generation)).is_none(), "a frame of the old generation finds no socket");
        let sock = take(owner, Some(b.generation)).unwrap();
        assert_eq!(sock.channel_id, "chan2");
        put_back(owner, sock);
        // Only the matching generation may remove it.
        forget(owner, a.generation);
        assert!(take(owner, None).map(|s| put_back(owner, s)).is_some());
        discard(owner, b.generation);
        assert!(take(owner, None).is_none());
        set_test_bytesocks(None);
    }

    #[test]
    fn an_unreachable_service_means_no_socket() {
        let svc = Rc::new(FakeService { made: RefCell::new(Vec::new()), fail: RefCell::new(Some(SocketError::Status(503))) });
        set_test_bytesocks(Some(svc.clone()));
        let who = Who { uuid: 0x5eed_0002, name: "Bob".into(), console: false };
        assert!(open(who.clone(), vec![], vec![], "u", sink()).is_none());
        *svc.fail.borrow_mut() = Some(SocketError::Other("no route".into()));
        assert!(open(who.clone(), vec![], vec![], "u", sink()).is_none());
        assert!(take(who.uuid, None).is_none());
        set_test_bytesocks(None);
        // Without a service set, a test never opens one.
        assert!(open(who, vec![], vec![], "https://sockets.example/", sink()).is_none());
    }

    #[test]
    fn the_periodic_tick_drops_sockets_of_players_who_left() {
        let svc = Rc::new(FakeService { made: RefCell::new(Vec::new()), fail: RefCell::new(None) });
        set_test_bytesocks(Some(svc.clone()));
        let (stays, leaves) = (0x5eed_0003, 0x5eed_0004);
        for owner in [stays, leaves] {
            open(Who { uuid: owner, name: "x".into(), console: false }, vec![], vec![], "u", sink()).unwrap();
        }
        // Only `leaves` has left: the registry is shared with the tests that run in parallel.
        tick(&|o| o != leaves);
        assert!(take(leaves, None).is_none(), "the player who left lost the socket");
        let kept = take(stays, None).expect("the other one stays");
        let generation = kept.state.generation;
        put_back(stays, kept);
        discard(stays, generation);
        assert!(take(stays, None).is_none());
        set_test_bytesocks(None);
    }

    // ---- the real transport, against a websocket server of the test's own ----

    #[test]
    fn http_bytesocks_creates_a_channel_and_moves_frames_both_ways() {
        use std::io::{BufRead, BufReader, Write};
        use std::net::TcpListener;
        use tungstenite::protocol::Role;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let from_client: Arc<Mutex<Vec<String>>> = Arc::default();
        let server_log = from_client.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = Vec::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    head.push(line);
                }
                let mut out = stream;
                if head[0].starts_with("GET /create ") {
                    out.write_all(b"HTTP/1.1 201 Created\r\nLocation: abc123\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
                    continue;
                }
                assert!(head[0].starts_with("GET /abc123 "), "{}", head[0]);
                let key = head.iter().find_map(|l| l.strip_prefix("Sec-WebSocket-Key: ")).unwrap().trim().to_owned();
                out.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {}\r\n\r\n", netio::accept_key(&key)).as_bytes()).unwrap();
                let mut ws = tungstenite::WebSocket::from_raw_socket(out, Role::Server, None);
                ws.send(tungstenite::Message::text("hello from the editor")).unwrap();
                while let Ok(m) = ws.read() {
                    if let tungstenite::Message::Text(t) = m {
                        server_log.lock().unwrap().push(t.to_string());
                    }
                }
            }
        });
        let got: Arc<Mutex<Vec<(u128, u64, String)>>> = Arc::default();
        let got2 = got.clone();
        let sink: FrameSink = Arc::new(move |o, g, t| got2.lock().unwrap().push((o, g, t)));
        let svc = HttpBytesocks::new(&format!("http://127.0.0.1:{port}")).unwrap();
        let channel = svc.create(77, 5, sink).unwrap();
        assert_eq!(channel.id, "abc123");
        let wait = |f: &dyn Fn() -> bool| {
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(8) {
                if f() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out");
        };
        wait(&|| !got.lock().unwrap().is_empty());
        assert_eq!(got.lock().unwrap()[0], (77, 5, "hello from the editor".to_owned()));
        channel.tx.send(Outbound::Text("{\"msg\":\"x\"}".into())).unwrap();
        wait(&|| !from_client.lock().unwrap().is_empty());
        assert_eq!(from_client.lock().unwrap()[0], "{\"msg\":\"x\"}");
        channel.tx.send(Outbound::Close).unwrap();
    }

    #[test]
    fn http_bytesocks_urls() {
        assert_eq!(HttpBytesocks::new("https://sockets.example").unwrap().ws_url(), "wss://sockets.example/");
        assert_eq!(HttpBytesocks::new("http://127.0.0.1:9/x/").unwrap().ws_url(), "ws://127.0.0.1:9/x/");
        assert!(HttpBytesocks::new("ftp://x").is_err());
    }
}
