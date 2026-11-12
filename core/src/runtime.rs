//! The running permission service of one server: the model in memory, a worker thread that
//! owns the database (loads, saves, `/lp` commands, the SQL messenger) and the game-thread
//! side (permission checks from a cache, lines to send, commands to run).
//!
//! The game thread never touches the database: it hands jobs to the worker and drains the
//! outbox once a tick.

use crate::actionlog::Action;
use crate::calc::{self, HolderRef, Resolved, Tristate};
use crate::command::{self, CmdOutput};
use crate::config::Config;
use crate::context::{self, ContextSet};
use crate::messaging::{Message, SeenIds, UpdateBuffer};
use crate::model::{Model, User};
use crate::storage::Storage;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Who a line goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Recipient {
    Player(u128),
    Console,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    pub to: Recipient,
    /// Legacy `§` codes.
    pub line: String,
}

/// What the game knows of a player on this server (refreshed every tick).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlinePlayer {
    pub uuid: u128,
    pub name: String,
    pub op_level: u8,
    /// `minecraft:overworld`...
    pub dimension: String,
    /// 0 survival, 1 creative, 2 adventure, 3 spectator.
    pub game_mode: i32,
}

/// Who runs an `/lp` command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSender {
    pub uuid: u128,
    pub name: String,
    pub console: bool,
}

impl CommandSender {
    pub fn console() -> Self {
        CommandSender { uuid: crate::uuid::CONSOLE, name: "Console".into(), console: true }
    }
}

/// Refused logins get this reason; the detail goes to the log.
pub const PRE_LOGIN_REFUSAL: &str = "Your permissions could not be loaded. Please try again in a moment.";

/// The answer to a pre-login load: Ok lets the player on, Err refuses them with the reason.
pub type PreLoginDone = Box<dyn FnOnce(Result<(), String>) + Send>;

enum Job {
    Login { uuid: u128, name: String },
    /// Loads the player's data before they are placed; `done` gets the answer on the worker thread.
    PreLogin { uuid: u128, name: String, done: PreLoginDone },
    Command { sender: CommandSender, args: Vec<String> },
    SyncAll { push: bool },
    ReloadConfig(Config),
    Shutdown,
}

type CacheKey = (u128, ContextSet, bool);

pub struct Runtime {
    cfg: RwLock<Arc<Config>>,
    pub model: RwLock<Model>,
    online: RwLock<HashMap<u128, OnlinePlayer>>,
    left: Mutex<HashMap<u128, Instant>>,
    level_name: RwLock<String>,
    outbox: Mutex<Vec<Outgoing>>,
    console_queue: Mutex<Vec<String>>,
    cache: Mutex<HashMap<CacheKey, (u64, Arc<Resolved>)>>,
    jobs: Mutex<Option<mpsc::Sender<Job>>>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
    storage_name: &'static str,
    messaging: AtomicBool,
}

fn slot() -> &'static RwLock<Option<Arc<Runtime>>> {
    static SLOT: OnceLock<RwLock<Option<Arc<Runtime>>>> = OnceLock::new();
    SLOT.get_or_init(|| RwLock::new(None))
}

/// The running service, if a server started it.
pub fn get() -> Option<Arc<Runtime>> {
    slot().read().ok().and_then(|g| g.clone())
}

fn log(line: &str) {
    eprintln!("[LuckPerms] {line}");
}

/// Starts the service: creates the tables if needed, loads every group and track (as
/// LuckPerms does on enable, on this thread), then starts the worker.
pub fn start(cfg: Config, mut storage: Box<dyn Storage>, level_name: &str) -> Result<Arc<Runtime>, String> {
    stop();
    storage.init()?;
    let mut model = Model::default();
    for g in storage.load_all_groups()? {
        model.groups.insert(g.name.clone(), g);
    }
    if model.ensure_default_group() {
        storage.create_group(crate::model::DEFAULT_GROUP)?;
    }
    for t in storage.load_all_tracks()? {
        model.tracks.insert(t.name.clone(), t);
    }
    command::mark_started();
    let messaging = cfg.sql_messaging();
    let last_id = if messaging { storage.messenger_init()? } else { -1 };
    let (tx, rx) = mpsc::channel();
    let rt = Arc::new(Runtime {
        cfg: RwLock::new(Arc::new(cfg)),
        model: RwLock::new(model),
        online: RwLock::new(HashMap::new()),
        left: Mutex::new(HashMap::new()),
        level_name: RwLock::new(level_name.to_owned()),
        outbox: Mutex::new(Vec::new()),
        console_queue: Mutex::new(Vec::new()),
        cache: Mutex::new(HashMap::new()),
        jobs: Mutex::new(Some(tx)),
        worker: Mutex::new(None),
        storage_name: storage.name(),
        messaging: AtomicBool::new(messaging),
    });
    let w = rt.clone();
    let handle = std::thread::Builder::new()
        .name("luckperms-worker".into())
        .spawn(move || Worker::new(w, storage, last_id).run(rx))
        .map_err(|e| e.to_string())?;
    *rt.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle);
    if let Ok(mut s) = slot().write() {
        *s = Some(rt.clone());
    }
    let groups = rt.model.read().map(|m| m.groups.len()).unwrap_or(0);
    log(&format!("loaded {groups} groups from {}; messaging {}", rt.storage_name, if messaging { "sql" } else { "off" }));
    Ok(rt)
}

/// A runtime without a worker or storage (tests, and code that only resolves).
pub fn detached(cfg: Config, model: Model) -> Runtime {
    Runtime {
        cfg: RwLock::new(Arc::new(cfg)),
        model: RwLock::new(model),
        online: RwLock::new(HashMap::new()),
        left: Mutex::new(HashMap::new()),
        level_name: RwLock::new("world".into()),
        outbox: Mutex::new(Vec::new()),
        console_queue: Mutex::new(Vec::new()),
        cache: Mutex::new(HashMap::new()),
        jobs: Mutex::new(None),
        worker: Mutex::new(None),
        storage_name: "memory",
        messaging: AtomicBool::new(false),
    }
}

/// Stops the worker after it finished its queue (a pending update ping is still sent).
pub fn stop() {
    let Some(rt) = slot().write().ok().and_then(|mut s| s.take()) else {
        return;
    };
    let tx = rt.jobs.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(tx) = tx {
        let _ = tx.send(Job::Shutdown);
    }
    let handle = rt.worker.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(h) = handle {
        let _ = h.join();
    }
}

impl Runtime {
    pub fn config(&self) -> Arc<Config> {
        self.cfg.read().map(|c| c.clone()).unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn storage_name(&self) -> &'static str {
        self.storage_name
    }

    pub fn messaging_on(&self) -> bool {
        self.messaging.load(Ordering::Relaxed)
    }

    fn send(&self, job: Job) {
        if let Some(tx) = self.jobs.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
            let _ = tx.send(job);
        }
    }

    pub fn push_out(&self, to: Recipient, line: String) {
        self.outbox.lock().unwrap_or_else(|e| e.into_inner()).push(Outgoing { to, line });
    }

    /// Lines to deliver this tick.
    pub fn drain_outbox(&self) -> Vec<Outgoing> {
        std::mem::take(&mut *self.outbox.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// Console commands to run this tick (`grant-via-execute`).
    pub fn drain_console_commands(&self) -> Vec<String> {
        std::mem::take(&mut *self.console_queue.lock().unwrap_or_else(|e| e.into_inner()))
    }

    // ---- players ----------------------------------------------------------------------

    /// A player was accepted: their data is loaded on the worker. Until it is, they resolve as
    /// a member of `default` only.
    pub fn on_login(&self, uuid: u128, name: &str) {
        self.left.lock().unwrap_or_else(|e| e.into_inner()).remove(&uuid);
        self.send(Job::Login { uuid, name: name.to_owned() });
    }

    /// `native_player_pre_login`: the worker loads the player's data, then `done` gets the answer. False when
    /// no worker runs (nothing is queued and the player is let in as before).
    pub fn pre_login(&self, uuid: u128, name: &str, done: PreLoginDone) -> bool {
        let guard = self.jobs.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_ref() {
            Some(tx) => tx.send(Job::PreLogin { uuid, name: name.to_owned(), done }).is_ok(),
            None => false,
        }
    }

    /// A player left: they stay loaded for `unload-delay-seconds`.
    pub fn on_logout(&self, uuid: u128) {
        if let Ok(mut o) = self.online.write() {
            o.remove(&uuid);
        }
        self.left.lock().unwrap_or_else(|e| e.into_inner()).insert(uuid, Instant::now());
    }

    /// The players on the server now, with their level, dimension and game mode.
    pub fn update_players(&self, players: Vec<OnlinePlayer>) {
        let map: HashMap<u128, OnlinePlayer> = players.into_iter().map(|p| (p.uuid, p)).collect();
        if let Ok(mut o) = self.online.write() {
            if *o != map {
                *o = map;
            }
        }
    }

    pub fn online_player(&self, uuid: u128) -> Option<OnlinePlayer> {
        self.online.read().ok()?.get(&uuid).cloned()
    }

    pub fn online_players(&self) -> Vec<OnlinePlayer> {
        self.online.read().map(|o| o.values().cloned().collect()).unwrap_or_default()
    }

    pub fn set_level_name(&self, name: &str) {
        if let Ok(mut l) = self.level_name.write() {
            *l = name.to_owned();
        }
    }

    /// A player's contexts: `server` (unless global), `world`, `dimension-type`, `gamemode`
    /// and the static ones, without the disabled keys.
    pub fn contexts_of(&self, p: &OnlinePlayer) -> ContextSet {
        let cfg = self.config();
        let level = self.level_name.read().map(|l| l.clone()).unwrap_or_else(|_| "world".into());
        let mut set = ContextSet::new();
        set.add(context::SERVER, &cfg.server);
        for world in cfg.world_names(&p.dimension, &level) {
            set.add(context::WORLD, &world);
        }
        let dim = p.dimension.rsplit(':').next().unwrap_or(&p.dimension);
        set.add(context::DIMENSION_TYPE, dim);
        let gm = match p.game_mode {
            1 => "creative",
            2 => "adventure",
            3 => "spectator",
            _ => "survival",
        };
        set.add(context::GAMEMODE, gm);
        for (k, v) in &cfg.static_contexts {
            set.add(k, v);
        }
        for k in &cfg.disabled_contexts {
            set.remove_key(&k.to_lowercase());
        }
        set
    }

    /// The server's own contexts (for a command's sender without a player, or a player not
    /// placed yet).
    pub fn static_contexts(&self) -> ContextSet {
        let cfg = self.config();
        let mut set = ContextSet::new();
        set.add(context::SERVER, &cfg.server);
        for (k, v) in &cfg.static_contexts {
            set.add(k, v);
        }
        set
    }

    pub fn is_op(&self, level: u8) -> bool {
        let cfg = self.config();
        cfg.horizon.enable_ops && level >= cfg.horizon.op_level
    }

    /// The resolved data of a user in contexts, cached until the model changes.
    pub fn resolved(&self, uuid: u128, contexts: &ContextSet, op: bool) -> Arc<Resolved> {
        let model = self.model.read().unwrap_or_else(|e| e.into_inner());
        let version = model.version;
        let key = (uuid, contexts.clone(), op);
        if let Some((v, r)) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            if *v == version {
                return r.clone();
            }
        }
        let cfg = self.config();
        let r = if model.users.contains_key(&uuid) {
            Arc::new(calc::resolve(&model, &cfg, &HolderRef::User(uuid), contexts, op, crate::duration::now()))
        } else {
            // Not loaded (yet): a member of default, as LuckPerms treats a user without data.
            let mut m = model.clone();
            let mut u = User::new(uuid);
            u.give_default_if_needed();
            m.users.insert(uuid, u);
            Arc::new(calc::resolve(&m, &cfg, &HolderRef::User(uuid), contexts, op, crate::duration::now()))
        };
        drop(model);
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        if cache.len() > 2048 {
            cache.clear();
        }
        cache.insert(key, (version, r.clone()));
        r
    }

    /// The permission check for an online player, in their current contexts; Undefined for a
    /// player not on this server.
    pub fn check(&self, uuid: u128, permission: &str) -> Tristate {
        let set = match self.online_player(uuid) {
            Some(p) => {
                let ctx = self.contexts_of(&p);
                self.resolved(uuid, &ctx, self.is_op(p.op_level)).check(permission).result
            }
            None => Tristate::Undefined,
        };
        // Bukkit's plugin.yml `default: true`: a node a plugin registered as default-true, which the
        // permissions leave undefined, is true (`apply-bukkit-default-permissions`).
        if set == Tristate::Undefined && self.config().apply_bukkit_default_permissions && crate::api::is_default_true(permission) {
            return Tristate::True;
        }
        set
    }

    /// The model's change counter: it moves on every change of a group, user or track.
    pub fn model_version(&self) -> u64 {
        self.model.read().map(|m| m.version).unwrap_or_else(|e| e.into_inner().version)
    }

    /// `hasPermission`: the check, undefined counting as false.
    pub fn has(&self, uuid: u128, permission: &str) -> bool {
        self.check(uuid, permission) == Tristate::True
    }

    /// Whether an `/lp` sender may use a LuckPerms command permission (`luckperms.<node>`).
    pub fn sender_may(&self, sender: &CommandSender, node: &str) -> bool {
        if sender.console {
            return true;
        }
        let Some(p) = self.online_player(sender.uuid) else {
            return false;
        };
        let cfg = self.config();
        let ctx = self.contexts_of(&p);
        // Ops are not "op" here: `commands-allow-op` decides for them.
        let r = self.resolved(sender.uuid, &ctx, false).check(&format!("luckperms.{node}")).result;
        match r {
            Tristate::True => true,
            Tristate::False => false,
            Tristate::Undefined => cfg.horizon.commands_allow_op && self.is_op(p.op_level),
        }
    }

    /// The decision for a typed command (`/<root> ...`): Some(false) refuses it, Some(true)
    /// grants it beyond the operator level, None leaves it to the game.
    pub fn command_decision(&self, uuid: u128, root: &str) -> Option<bool> {
        let cfg = self.config();
        if !cfg.horizon.command_permissions {
            return None;
        }
        let root = root.trim_start_matches('/').to_lowercase();
        if command::ROOTS.contains(&root.as_str()) {
            return None; // ours: /lp checks its own permissions
        }
        let p = self.online_player(uuid)?;
        let ctx = self.contexts_of(&p);
        // Not "op" here: an op keeps the game's own rights unless a node says false.
        let node = cfg.command_node(&root);
        self.resolved(uuid, &ctx, false).check(&node).result.as_bool()
    }

    /// Queues `execute as <name> at <name> run <command>` for the next tick.
    pub fn queue_console_command(&self, line: String) {
        self.console_queue.lock().unwrap_or_else(|e| e.into_inner()).push(line);
    }

    // ---- /lp ------------------------------------------------------------------------

    /// Runs `/lp <args>` on the worker; the answer arrives through the outbox.
    pub fn submit_command(&self, sender: CommandSender, line: &str) {
        let args = crate::text::split_args(line);
        self.send(Job::Command { sender, args });
    }

    pub fn reload_config(&self, cfg: Config) {
        self.send(Job::ReloadConfig(cfg));
    }

    pub fn request_sync(&self, push: bool) {
        self.send(Job::SyncAll { push });
    }

    /// Completions for the argument being typed of `/lp <args>`.
    pub fn complete(&self, sender: &CommandSender, args: &[String]) -> Vec<String> {
        command::complete(self, sender, args)
    }
}

/// The worker: owns the storage.
struct Worker {
    rt: Arc<Runtime>,
    storage: Box<dyn Storage>,
    last_id: i64,
    seen: SeenIds,
    buffer: UpdateBuffer,
    last_poll: Instant,
    last_housekeep: Instant,
    last_audit: Instant,
    last_sync: Instant,
    /// Players whose data a pre-login load already brought in: their login does not load it again.
    preloaded: HashSet<u128>,
}

impl Worker {
    fn new(rt: Arc<Runtime>, storage: Box<dyn Storage>, last_id: i64) -> Self {
        let now = Instant::now();
        Worker {
            rt,
            storage,
            last_id,
            seen: SeenIds::default(),
            buffer: UpdateBuffer::default(),
            last_poll: now,
            last_housekeep: now,
            last_audit: now,
            last_sync: now,
            preloaded: HashSet::new(),
        }
    }

    fn run(mut self, rx: mpsc::Receiver<Job>) {
        loop {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(Job::Shutdown) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if self.buffer.pending() {
                        self.push(Message::Update { id: crate::uuid::random_v4() });
                    }
                    return;
                }
                Ok(job) => self.handle(job),
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            self.periodic();
        }
    }

    fn handle(&mut self, job: Job) {
        match job {
            Job::Login { uuid, name } => {
                // Already loaded by the pre-login answer (`native_player_pre_login`), unless that never came.
                if !self.preloaded.remove(&uuid) {
                    if let Err(e) = self.load_user(uuid, Some(&name)) {
                        log(&e);
                    }
                }
            }
            Job::PreLogin { uuid, name, done } => match self.load_user(uuid, Some(&name)) {
                Ok(()) => {
                    self.preloaded.insert(uuid);
                    done(Ok(()));
                }
                Err(e) => {
                    log(&e);
                    done(Err(PRE_LOGIN_REFUSAL.to_owned()));
                }
            },
            Job::Command { sender, args } => {
                let out = command::execute(&self.rt, self.storage.as_mut(), &sender, &args);
                self.finish(&sender, out);
            }
            Job::SyncAll { push } => {
                self.sync_all();
                if push {
                    self.push(Message::Update { id: crate::uuid::random_v4() });
                }
            }
            Job::ReloadConfig(cfg) => {
                self.rt.messaging.store(cfg.sql_messaging(), Ordering::Relaxed);
                if let Ok(mut c) = self.rt.cfg.write() {
                    *c = Arc::new(cfg);
                }
                self.rt.model.write().unwrap_or_else(|e| e.into_inner()).touch();
            }
            Job::Shutdown => {}
        }
    }

    /// Loads a player's nodes into the model. A failed load is returned (the caller logs it or refuses the
    /// login); a failed name save is only logged, as LuckPerms does.
    fn load_user(&mut self, uuid: u128, name: Option<&str>) -> Result<(), String> {
        if let Some(name) = name {
            if let Err(e) = self.storage.save_player_data(uuid, name) {
                log(&format!("could not save the name of {}: {e}", crate::uuid::format(uuid)));
            }
        }
        let (nodes, row) = self
            .storage
            .load_user(uuid)
            .map_err(|e| format!("could not load {}: {e}", crate::uuid::format(uuid)))?;
        let mut u = User::new(uuid);
        u.data.nodes = nodes.clone();
        u.saved = nodes;
        if let Some(r) = row {
            u.primary_group = if r.primary_group.is_empty() { crate::model::DEFAULT_GROUP.into() } else { r.primary_group.to_lowercase() };
            if r.username != "null" {
                u.username = Some(r.username);
            }
        }
        if let Some(n) = name {
            u.username = Some(n.to_owned());
        }
        u.give_default_if_needed();
        if self.rt.config().primary_group_calculation.eq_ignore_ascii_case("stored") {
            u.repair_stored_primary_group();
        }
        let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
        m.users.insert(uuid, u);
        m.touch();
        Ok(())
    }

    fn sync_all(&mut self) {
        let groups = match self.storage.load_all_groups() {
            Ok(g) => g,
            Err(e) => {
                log(&format!("sync failed: {e}"));
                return;
            }
        };
        let tracks = self.storage.load_all_tracks().unwrap_or_default();
        {
            let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
            m.groups = groups.into_iter().map(|g| (g.name.clone(), g)).collect();
            m.ensure_default_group();
            m.tracks = tracks.into_iter().map(|t| (t.name.clone(), t)).collect();
            m.touch();
        }
        let users: Vec<u128> = self.rt.model.read().map(|m| m.users.keys().copied().collect()).unwrap_or_default();
        for u in users {
            if let Err(e) = self.load_user(u, None) {
                log(&e);
            }
        }
    }

    fn push(&mut self, msg: Message) {
        if !self.rt.messaging_on() {
            return;
        }
        self.seen.add(msg.id());
        if let Err(e) = self.storage.messenger_send(&msg.encode()) {
            log(&format!("messenger send failed: {e}"));
        }
    }

    /// After a command: the log, the notifications, the pings to the other servers.
    fn finish(&mut self, sender: &CommandSender, out: CmdOutput) {
        let to = if sender.console { Recipient::Console } else { Recipient::Player(sender.uuid) };
        for l in out.lines {
            self.rt.push_out(to.clone(), l);
        }
        let cfg = self.rt.config();
        for a in &out.actions {
            if let Err(e) = self.storage.log_action(a) {
                log(&format!("could not log an action: {e}"));
            }
            self.notify(a, false, Some(sender.uuid));
            if cfg.push_log_entries {
                self.push(Message::Log { id: crate::uuid::random_v4(), action: a.clone() });
            }
        }
        if out.sync {
            self.sync_all();
        }
        if cfg.auto_push_updates {
            for u in out.user_updates {
                self.push(Message::UserUpdate { id: crate::uuid::random_v4(), user: u });
            }
            if out.group_changed {
                self.buffer.request();
            }
        }
        if out.network_sync {
            self.push(Message::Update { id: crate::uuid::random_v4() });
        }
    }

    /// `LogDispatcher`: the console and everyone with `luckperms.log.notify` (who did not turn
    /// it off), except the one who did it.
    fn notify(&self, a: &Action, remote: bool, except: Option<u128>) {
        let cfg = self.rt.config();
        if !cfg.log_notify {
            return;
        }
        if remote && !cfg.broadcast_received_log_entries {
            return;
        }
        for pattern in &cfg.log_notify_filtered_descriptions {
            if regex::Regex::new(&format!("^(?:{pattern})$")).is_ok_and(|re| re.is_match(&a.description)) {
                return;
            }
        }
        let line = a.notify_line(remote);
        // The console is a sender too: it is skipped for what it did itself.
        if except != Some(crate::uuid::CONSOLE) {
            self.rt.push_out(Recipient::Console, line.clone());
        }
        for p in self.rt.online_players() {
            if Some(p.uuid) == except {
                continue;
            }
            let ctx = self.rt.contexts_of(&p);
            let r = self.rt.resolved(p.uuid, &ctx, self.rt.is_op(p.op_level));
            let may = r.check("luckperms.log.notify").result == Tristate::True;
            let ignoring = r.direct("luckperms.log.notify.ignoring").is_some_and(|o| o.value);
            if may && !ignoring {
                self.rt.push_out(Recipient::Player(p.uuid), line.clone());
            }
        }
    }

    fn periodic(&mut self) {
        let cfg = self.rt.config();
        if self.rt.messaging_on() && self.last_poll.elapsed() >= Duration::from_secs(1) {
            self.last_poll = Instant::now();
            match self.storage.messenger_poll(self.last_id) {
                Ok(rows) => {
                    for (id, msg) in rows {
                        self.last_id = self.last_id.max(id);
                        self.incoming(&msg, &cfg);
                    }
                }
                Err(e) => log(&format!("messenger poll failed: {e}")),
            }
        }
        if self.rt.messaging_on() && self.last_housekeep.elapsed() >= Duration::from_secs(30) {
            self.last_housekeep = Instant::now();
            let _ = self.storage.messenger_housekeep();
        }
        if self.buffer.take_due() {
            self.push(Message::Update { id: crate::uuid::random_v4() });
        }
        if cfg.sync_minutes > 0 && self.last_sync.elapsed() >= Duration::from_secs(cfg.sync_minutes as u64 * 60) {
            self.last_sync = Instant::now();
            self.sync_all();
        }
        if self.last_audit.elapsed() >= Duration::from_secs(3) {
            self.last_audit = Instant::now();
            self.audit_expired();
            self.unload_left(&cfg);
        }
    }

    fn incoming(&mut self, raw: &str, cfg: &Config) {
        let Some(msg) = Message::decode(raw) else {
            return;
        };
        if !self.seen.add(msg.id()) {
            return;
        }
        match msg {
            Message::Update { id } => {
                log(&format!("[Messaging] Received update ping with id: {}", crate::uuid::format(id)));
                self.sync_all();
            }
            Message::UserUpdate { user, .. } => {
                let loaded = self.rt.model.read().map(|m| m.users.contains_key(&user)).unwrap_or(false);
                if loaded {
                    if let Err(e) = self.load_user(user, None) {
                        log(&e);
                    }
                }
            }
            Message::Log { action, .. } => {
                if cfg.broadcast_received_log_entries {
                    self.notify(&action, true, None);
                }
            }
            Message::Custom { .. } => {}
        }
    }

    /// `ExpireTemporaryTask`: expired temporary nodes leave, and the change is saved.
    fn audit_expired(&mut self) {
        let now = crate::duration::now();
        let (users, groups) = {
            let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
            let mut users = Vec::new();
            for u in m.users.values_mut() {
                if u.data.audit_expired(now) {
                    users.push(u.clone());
                }
            }
            let mut groups = Vec::new();
            for g in m.groups.values_mut() {
                let before = g.data.nodes.clone();
                if g.data.audit_expired(now) {
                    groups.push((g.clone(), before));
                }
            }
            if !users.is_empty() || !groups.is_empty() {
                m.touch();
            }
            (users, groups)
        };
        for u in users {
            if self.storage.save_user(&u).is_ok() {
                if let Some(m) = self.rt.model.write().ok().as_mut().and_then(|m| m.users.get_mut(&u.uuid)) {
                    m.saved = u.data.nodes.clone();
                }
            }
        }
        for (g, before) in groups {
            let _ = self.storage.save_group(&g, &before);
        }
    }

    fn unload_left(&mut self, cfg: &Config) {
        let delay = Duration::from_secs(cfg.horizon.unload_delay_seconds);
        let gone: Vec<u128> = {
            let mut left = self.rt.left.lock().unwrap_or_else(|e| e.into_inner());
            let g: Vec<u128> = left.iter().filter(|(_, t)| t.elapsed() >= delay).map(|(u, _)| *u).collect();
            for u in &g {
                left.remove(u);
            }
            g
        };
        if gone.is_empty() {
            return;
        }
        let online: Vec<u128> = self.rt.online_players().into_iter().map(|p| p.uuid).collect();
        let mut m = self.rt.model.write().unwrap_or_else(|e| e.into_inner());
        for u in gone {
            if !online.contains(&u) {
                m.users.remove(&u);
            }
        }
        m.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryStorage;

    fn wait_for(f: impl Fn() -> bool) {
        for _ in 0..100 {
            if f() {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("timed out");
    }

    #[test]
    fn login_command_and_check() {
        let mut cfg = Config::default();
        cfg.storage.method = "memory".into();
        cfg.messaging_service = "none".into();
        let storage = MemoryStorage::default();
        let rt = start(cfg, Box::new(storage.clone()), "world").unwrap();
        let alice = 0xa11ce;
        rt.update_players(vec![OnlinePlayer { uuid: alice, name: "Alice".into(), op_level: 0, dimension: "minecraft:overworld".into(), game_mode: 0 }]);
        rt.on_login(alice, "Alice");
        wait_for(|| rt.model.read().unwrap().users.contains_key(&alice));
        assert_eq!(rt.check(alice, "essentials.fly"), Tristate::Undefined);
        rt.submit_command(CommandSender::console(), "group default permission set essentials.fly true world=world");
        wait_for(|| rt.check(alice, "essentials.fly") == Tristate::True);
        rt.submit_command(CommandSender::console(), "user Alice permission set essentials.fly false");
        wait_for(|| rt.check(alice, "essentials.fly") == Tristate::False);
        // The console got answers; the change is stored.
        wait_for(|| !storage.inner.lock().unwrap().actions.is_empty());
        let out = rt.drain_outbox();
        assert!(out.iter().any(|o| o.to == Recipient::Console));
        assert_eq!(storage.inner.lock().unwrap().user_nodes.get(&alice).map(|n| n.len()), Some(2));
        // The API other mods use (api.rs): the answers match what the runtime resolved.
        assert_eq!(crate::api::permission(alice, "essentials.fly"), Some(false));
        assert_eq!(crate::api::permission(alice, "essentials.speed"), None);
        assert_eq!(crate::api::primary_group(alice).as_deref(), Some("default"));
        assert_eq!(crate::api::weight(alice), Some(0), "the default group has no weight node");
        assert_eq!(crate::api::weight(0xdead), None, "not on this server");
        stop();
    }
}

#[cfg(test)]
mod pre_login_tests {
    use super::*;
    use crate::storage::MemoryStorage;
    use std::sync::atomic::Ordering;

    fn worker(storage: MemoryStorage) -> (Arc<Runtime>, Worker) {
        let mut cfg = Config::default();
        cfg.storage.method = "memory".into();
        cfg.messaging_service = "none".into();
        let rt = Arc::new(detached(cfg, Model::default()));
        let w = Worker::new(rt.clone(), Box::new(storage), -1);
        (rt, w)
    }

    #[test]
    fn pre_login_loads_before_placement_and_the_login_does_not_load_again() {
        let storage = MemoryStorage::default();
        let (rt, mut w) = worker(storage.clone());
        let (tx, rx) = mpsc::channel();
        let alice = 0xa11ce;
        w.handle(Job::PreLogin { uuid: alice, name: "Alice".into(), done: Box::new(move |r| { let _ = tx.send(r); }) });
        assert_eq!(rx.recv().unwrap(), Ok(()));
        assert!(rt.model.read().unwrap().users.contains_key(&alice), "loaded before the player is placed");
        assert!(w.preloaded.contains(&alice));
        // The login that follows does not take the load again.
        w.handle(Job::Login { uuid: alice, name: "Alice".into() });
        assert!(!w.preloaded.contains(&alice));
    }

    #[test]
    fn a_failed_load_refuses_the_login_with_the_generic_reason() {
        let storage = MemoryStorage::default();
        storage.fail_loads.store(true, Ordering::Relaxed);
        let (rt, mut w) = worker(storage);
        let (tx, rx) = mpsc::channel();
        let bob = 0xb0b;
        w.handle(Job::PreLogin { uuid: bob, name: "Bob".into(), done: Box::new(move |r| { let _ = tx.send(r); }) });
        assert_eq!(rx.recv().unwrap(), Err(PRE_LOGIN_REFUSAL.to_owned()));
        assert!(!rt.model.read().unwrap().users.contains_key(&bob));
        assert!(!w.preloaded.contains(&bob));
    }

    #[test]
    fn a_login_without_pre_login_still_loads() {
        let storage = MemoryStorage::default();
        let (rt, mut w) = worker(storage);
        let owner = 0x0ff;
        w.handle(Job::Login { uuid: owner, name: "Owner".into() });
        assert!(rt.model.read().unwrap().users.contains_key(&owner));
    }

    #[test]
    fn no_worker_means_no_pre_login() {
        let rt = detached(Config::default(), Model::default());
        assert!(!rt.pre_login(1, "x", Box::new(|_| {})));
    }

    #[test]
    fn default_true_nodes_are_true_only_while_undefined_and_enabled() {
        let (rt, _) = worker(MemoryStorage::default());
        let p = 0xd3f;
        rt.update_players(vec![OnlinePlayer { uuid: p, name: "Dee".into(), op_level: 0, dimension: "minecraft:overworld".into(), game_mode: 0 }]);
        crate::api::register_default_true("Test.Default.Node");
        assert!(crate::api::is_default_true("test.default.node"));
        assert_eq!(rt.check(p, "test.default.node"), Tristate::True);
        // An explicit false still wins.
        {
            let mut m = rt.model.write().unwrap();
            let node = crate::node::Node::permanent("test.default.node", false, crate::context::ContextSet::new());
            m.users.entry(p).or_insert_with(|| crate::model::User::new(p)).data.set(node);
            m.touch();
        }
        assert_eq!(rt.check(p, "test.default.node"), Tristate::False);
        // Switched off in the config: nothing is registered for the player.
        let mut cfg = (*rt.config()).clone();
        cfg.apply_bukkit_default_permissions = false;
        *rt.cfg.write().unwrap() = Arc::new(cfg);
        assert_eq!(rt.check(p, "test.default.node"), Tristate::False);
        assert_eq!(rt.check(p, "test.other.node"), Tristate::Undefined);
    }
}

