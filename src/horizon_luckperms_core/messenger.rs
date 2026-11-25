//! The messaging transports besides the SQL one (`messaging-service`): Redis (`RedisMessenger`) and NATS
//! (`NatsMessenger`). Both carry the JSON messages of `messaging.rs` on the channel `luckperms:update`, so a
//! Horizon server shares updates with Paper and Velocity servers on the same broker.
//!
//! Redis runs on the `redis` client crate (plain, TLS, sentinel; a cluster is reached through its first node,
//! pub/sub messages reach every node). NATS is spoken by hand (its text protocol is a page long) over TCP with
//! optional TLS. Both keep a thread that reads and reconnects after five seconds; `send` and `poll` never block
//! on the network for long and run on the worker thread, never on the tick.
//!
//! RabbitMQ (an AMQP client), PostgreSQL's LISTEN/NOTIFY (a PostgreSQL client) and plugin messaging (a player
//! connection plus a relay on the proxy, which a mod's registered payload channels do not give) are refused.

use crate::horizon_luckperms_core::config::{Config, Messaging};

/// The channel LuckPerms publishes on.
pub const CHANNEL: &str = "luckperms:update";

pub trait Messenger: Send {
    fn name(&self) -> &'static str;
    /// Publishes one encoded message.
    fn send(&mut self, message: &str) -> Result<(), String>;
    /// The messages received since the last call.
    fn poll(&mut self) -> Vec<String>;
    /// Stops the threads.
    fn close(&mut self) {}
}

/// The messenger the config selects, started. `None` for SQL (the storage carries it) and for no messaging.
pub fn open(cfg: &Config) -> Result<Option<Box<dyn Messenger>>, String> {
    match cfg.messaging() {
        Messaging::Redis => open_redis(cfg).map(Some),
        Messaging::Nats => open_nats(cfg).map(Some),
        Messaging::PluginMsg => Ok(Some(Box::new(crate::horizon_luckperms_core::messenger_wave2::PluginMessenger))),
        #[cfg(feature = "wave2")]
        Messaging::Postgres => crate::horizon_luckperms_core::messenger_wave2::open_postgres(cfg).map(Some),
        #[cfg(feature = "wave2")]
        Messaging::Rabbit => crate::horizon_luckperms_core::messenger_wave2::open_rabbitmq(cfg).map(Some),
        #[cfg(not(feature = "wave2"))]
        Messaging::Postgres | Messaging::Rabbit => Err("built without the wave2 feature".into()),
        Messaging::Refused(what) => Err(refusal(what)),
        Messaging::Sql | Messaging::Off => Ok(None),
    }
}

/// Why a named service cannot run.
pub fn refusal(what: &str) -> String {
    match what {
        "rabbitmq" => "messaging service rabbitmq needs the wave2 build (the AMQP client of fam-mod-deps-db-clients)".into(),
        "postgresql" => "messaging service postgresql needs the wave2 build (the PostgreSQL client of fam-mod-deps-db-clients)".into(),
        "pluginmsg" => "messaging service pluginmsg needs the wave2 build (the plugin channels of fam-provider-bridges)".into(),
        other => format!("messaging service {other} is not available"),
    }
}

/// `HostAndPort.requireBracketsForIPv6().withDefaultPort(..)`: `host`, `host:port`, `[v6]`, `[v6]:port`.
pub fn parse_address(addr: &str, default_port: u16) -> Result<(String, u16), String> {
    let a = addr.trim();
    let (host, port) = if let Some(rest) = a.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or("an IPv6 address is missing its closing bracket")?;
        match after {
            "" => (host, None),
            p => (host, Some(p.strip_prefix(':').ok_or("unexpected text after the IPv6 address")?)),
        }
    } else {
        match a.matches(':').count() {
            0 => (a, None),
            1 => {
                let (h, p) = a.split_once(':').unwrap_or((a, ""));
                (h, Some(p))
            }
            _ => return Err("an IPv6 address needs brackets".into()),
        }
    };
    if host.is_empty() {
        return Err("the address has no host".into());
    }
    let port = match port {
        None => default_port,
        Some(p) => p.parse::<u16>().map_err(|_| "the address has a bad port".to_string())?,
    };
    Ok((host.to_owned(), port))
}

/// Replaces every secret in an error text.
fn scrub(mut text: String, secrets: &[&str]) -> String {
    for s in secrets.iter().filter(|s| s.len() >= 3) {
        text = text.replace(s, "***");
    }
    text
}

fn log(line: &str) {
    eprintln!("[LuckPerms] {line}");
}

// ---- Java's DataOutput.writeUTF / readUTF (the NATS payload) --------------------------------

/// `DataOutput.writeUTF`: a big-endian u16 length and the text in modified UTF-8 (NUL as `C0 80`, characters outside
/// the BMP as two 3-byte surrogates). None when it would not fit the length.
pub fn write_utf(s: &str) -> Option<Vec<u8>> {
    let mut body = Vec::with_capacity(s.len());
    for u in s.encode_utf16() {
        match u {
            0x0001..=0x007f => body.push(u as u8),
            0x0000 | 0x0080..=0x07ff => {
                body.push(0xc0 | (u >> 6) as u8);
                body.push(0x80 | (u & 0x3f) as u8);
            }
            _ => {
                body.push(0xe0 | (u >> 12) as u8);
                body.push(0x80 | ((u >> 6) & 0x3f) as u8);
                body.push(0x80 | (u & 0x3f) as u8);
            }
        }
    }
    let len = u16::try_from(body.len()).ok()?;
    let mut out = len.to_be_bytes().to_vec();
    out.extend(body);
    Some(out)
}

/// `DataInput.readUTF` on a whole payload.
pub fn read_utf(data: &[u8]) -> Option<String> {
    let len = u16::from_be_bytes([*data.first()?, *data.get(1)?]) as usize;
    let body = data.get(2..2 + len)?;
    let mut units: Vec<u16> = Vec::with_capacity(len);
    let mut i = 0;
    while i < body.len() {
        let b = body[i] as u16;
        match b >> 4 {
            0..=7 => {
                units.push(b);
                i += 1;
            }
            0xc | 0xd => {
                let c = *body.get(i + 1)? as u16;
                if c & 0xc0 != 0x80 {
                    return None;
                }
                units.push(((b & 0x1f) << 6) | (c & 0x3f));
                i += 2;
            }
            0xe => {
                let (c, d) = (*body.get(i + 1)? as u16, *body.get(i + 2)? as u16);
                if c & 0xc0 != 0x80 || d & 0xc0 != 0x80 {
                    return None;
                }
                units.push(((b & 0x0f) << 12) | ((c & 0x3f) << 6) | (d & 0x3f));
                i += 3;
            }
            _ => return None,
        }
    }
    String::from_utf16(&units).ok()
}

// ---- Redis -----------------------------------------------------------------------------------

#[cfg(feature = "redis")]
mod redis_messenger {
    use super::*;
    use crate::horizon_luckperms_core::config::Redis;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::Duration;

    const SENTINEL_PORT: u16 = 26379;
    const TIMEOUT: Duration = Duration::from_secs(5);

    fn percent(s: &str) -> String {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    }

    /// The `user:password@` part of a Redis URL (empty when neither is set).
    pub(super) fn auth_part(user: &str, password: &str) -> String {
        match (user.is_empty(), password.is_empty()) {
            (true, true) => String::new(),
            (true, false) => format!(":{}@", percent(password)),
            (false, true) => format!("{}@", percent(user)),
            (false, false) => format!("{}:{}@", percent(user), percent(password)),
        }
    }

    fn host_part(host: &str, port: u16) -> String {
        if host.contains(':') { format!("[{host}]:{port}") } else { format!("{host}:{port}") }
    }

    /// The URL of the one server to talk to: the address, or the first node of `addresses` (a cluster).
    pub(super) fn url(cfg: &Redis) -> Result<String, String> {
        let addr = cfg.addresses.iter().map(|a| a.trim()).find(|a| !a.is_empty()).or_else(|| Some(cfg.address.trim()).filter(|a| !a.is_empty()));
        let addr = addr.ok_or("redis.address is empty")?;
        let (host, port) = parse_address(addr, 6379).map_err(|e| format!("redis.address: {e}"))?;
        let scheme = if cfg.ssl { "rediss" } else { "redis" };
        Ok(format!("{scheme}://{}{}/", auth_part(&cfg.username, &cfg.password), host_part(&host, port)))
    }

    /// The URLs of the sentinels (default port 26379).
    pub(super) fn sentinel_nodes(cfg: &Redis) -> Result<Vec<String>, String> {
        let scheme = if cfg.ssl { "rediss" } else { "redis" };
        let auth = auth_part(&cfg.sentinel.username, &cfg.sentinel.password);
        let nodes = cfg
            .sentinel
            .addresses
            .iter()
            .filter(|a| !a.trim().is_empty())
            .map(|a| parse_address(a, SENTINEL_PORT).map(|(h, p)| format!("{scheme}://{auth}{}/", host_part(&h, p))).map_err(|e| format!("redis.sentinel.addresses: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        if nodes.is_empty() {
            return Err("redis.sentinel.addresses is empty".into());
        }
        Ok(nodes)
    }

    enum Link {
        Direct(redis::Client),
        Sentinel(redis::sentinel::SentinelClient),
    }

    impl Link {
        fn build(cfg: &Redis) -> Result<Link, String> {
            if cfg.sentinel.enabled {
                let mut primary = redis::RedisConnectionInfo::default();
                if !cfg.username.is_empty() {
                    primary = primary.set_username(&cfg.username);
                }
                if !cfg.password.is_empty() {
                    primary = primary.set_password(&cfg.password);
                }
                let mut node_info = redis::sentinel::SentinelNodeConnectionInfo::default().set_redis_connection_info(primary);
                if cfg.ssl {
                    node_info = node_info.set_tls_mode(redis::TlsMode::Secure);
                }
                let client = redis::sentinel::SentinelClient::build(
                    sentinel_nodes(cfg)?,
                    cfg.sentinel.master.trim().to_owned(),
                    Some(node_info),
                    redis::sentinel::SentinelServerType::Master,
                )
                .map_err(|e| scrub(format!("redis sentinel: {e}"), &secrets(cfg)))?;
                Ok(Link::Sentinel(client))
            } else {
                let client = redis::Client::open(url(cfg)?.as_str()).map_err(|e| scrub(format!("redis: {e}"), &secrets(cfg)))?;
                Ok(Link::Direct(client))
            }
        }

        fn connection(&mut self, secrets: &[&str]) -> Result<redis::Connection, String> {
            let c = match self {
                Link::Direct(client) => client.get_connection_with_timeout(TIMEOUT),
                Link::Sentinel(s) => s.get_connection(),
            }
            .map_err(|e| scrub(format!("redis: {e}"), secrets))?;
            let _ = c.set_read_timeout(Some(TIMEOUT));
            let _ = c.set_write_timeout(Some(TIMEOUT));
            Ok(c)
        }
    }

    fn secrets(cfg: &Redis) -> Vec<&str> {
        vec![cfg.password.as_str(), cfg.sentinel.password.as_str(), cfg.username.as_str()]
    }

    pub struct RedisMessenger {
        publish: Link,
        connection: Option<redis::Connection>,
        inbox: mpsc::Receiver<String>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
        secrets: Vec<String>,
    }

    impl RedisMessenger {
        /// Builds the links and starts the subscriber thread; connecting happens on the threads, so a broker that is
        /// down at start does not hold the server (it is retried every five seconds).
        pub fn start(cfg: &Redis) -> Result<RedisMessenger, String> {
            let publish = Link::build(cfg)?;
            let listen = Link::build(cfg)?;
            let (tx, rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let owned: Vec<String> = secrets(cfg).into_iter().map(str::to_owned).collect();
            let (stop2, owned2) = (stop.clone(), owned.clone());
            let thread = std::thread::Builder::new()
                .name("luckperms-redis".into())
                .spawn(move || subscribe(listen, tx, stop2, owned2))
                .map_err(|e| e.to_string())?;
            Ok(RedisMessenger { publish, connection: None, inbox: rx, stop, thread: Some(thread), secrets: owned })
        }

        fn publish_once(&mut self, message: &str) -> Result<(), String> {
            let secrets: Vec<&str> = self.secrets.iter().map(String::as_str).collect();
            if self.connection.is_none() {
                self.connection = Some(self.publish.connection(&secrets)?);
            }
            let Some(c) = self.connection.as_mut() else { return Err("redis: no connection".into()) };
            redis::cmd("PUBLISH").arg(CHANNEL).arg(message).query::<i64>(c).map(|_| ()).map_err(|e| scrub(format!("redis: {e}"), &secrets))
        }
    }

    /// Waits up to `total`, in steps, unless stopped.
    fn nap(stop: &AtomicBool, total: Duration) {
        let step = Duration::from_millis(100);
        let mut left = total;
        while !stop.load(Ordering::Relaxed) && left > Duration::ZERO {
            let d = step.min(left);
            std::thread::sleep(d);
            left -= d;
        }
    }

    fn subscribe(mut link: Link, tx: mpsc::Sender<String>, stop: Arc<AtomicBool>, secrets: Vec<String>) {
        let secrets: Vec<&str> = secrets.iter().map(String::as_str).collect();
        let mut was_up = false;
        while !stop.load(Ordering::Relaxed) {
            let mut con = match link.connection(&secrets) {
                Ok(c) => c,
                Err(e) => {
                    log(&format!("Redis pubsub connection dropped, trying to re-open the connection: {e}"));
                    nap(&stop, Duration::from_secs(5));
                    continue;
                }
            };
            let mut pubsub = con.as_pubsub();
            let _ = pubsub.set_read_timeout(Some(Duration::from_millis(500)));
            if let Err(e) = pubsub.subscribe(CHANNEL) {
                log(&format!("Redis subscribe failed: {}", scrub(e.to_string(), &secrets)));
                nap(&stop, Duration::from_secs(5));
                continue;
            }
            if was_up {
                log("Redis pubsub connection re-established");
            }
            was_up = true;
            while !stop.load(Ordering::Relaxed) {
                match pubsub.get_message() {
                    Ok(msg) => {
                        if msg.get_channel_name() == CHANNEL {
                            let text = String::from_utf8_lossy(msg.get_payload_bytes()).into_owned();
                            if tx.send(text).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) if e.is_timeout() => continue,
                    Err(e) => {
                        log(&format!("Redis pubsub connection dropped, trying to re-open the connection: {}", scrub(e.to_string(), &secrets)));
                        break;
                    }
                }
            }
            if !stop.load(Ordering::Relaxed) {
                nap(&stop, Duration::from_secs(5));
            }
        }
    }

    impl Messenger for RedisMessenger {
        fn name(&self) -> &'static str {
            "Redis"
        }

        fn send(&mut self, message: &str) -> Result<(), String> {
            match self.publish_once(message) {
                Ok(()) => Ok(()),
                Err(_) => {
                    // One reconnect: the cached connection may have gone stale.
                    self.connection = None;
                    self.publish_once(message)
                }
            }
        }

        fn poll(&mut self) -> Vec<String> {
            self.inbox.try_iter().collect()
        }

        fn close(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Drop for RedisMessenger {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;
        use std::io::{BufRead, BufReader, Read, Write};
        use std::net::TcpListener;
        use std::sync::Mutex;
        use std::time::Instant;

        /// A Redis stand-in: answers `+OK` to everything, counts `PUBLISH`, and pushes queued messages to subscribers.
        #[derive(Default)]
        pub(crate) struct FakeRedis {
            pub published: Mutex<Vec<(String, String)>>,
            pub deliver: Mutex<Vec<String>>,
            pub subscribed: AtomicBool,
            pub auth: Mutex<Vec<Vec<String>>>,
        }

        fn command(r: &mut BufReader<std::net::TcpStream>) -> Option<Vec<String>> {
            let mut line = String::new();
            if r.read_line(&mut line).ok()? == 0 {
                return None;
            }
            let n: usize = line.trim().strip_prefix('*')?.parse().ok()?;
            let mut parts = Vec::new();
            for _ in 0..n {
                let mut l = String::new();
                r.read_line(&mut l).ok()?;
                let len: usize = l.trim().strip_prefix('$')?.parse().ok()?;
                let mut buf = vec![0u8; len + 2];
                r.read_exact(&mut buf).ok()?;
                buf.truncate(len);
                parts.push(String::from_utf8_lossy(&buf).into_owned());
            }
            Some(parts)
        }

        pub(crate) fn serve(fake: Arc<FakeRedis>) -> u16 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let fake = fake.clone();
                    std::thread::spawn(move || {
                        let mut out = stream.try_clone().unwrap();
                        let mut reader = BufReader::new(stream);
                        while let Some(c) = command(&mut reader) {
                            match c[0].to_uppercase().as_str() {
                                "PUBLISH" => {
                                    fake.published.lock().unwrap().push((c[1].clone(), c[2].clone()));
                                    let _ = out.write_all(b":1\r\n");
                                }
                                "SUBSCRIBE" => {
                                    let frame = format!("*3\r\n$9\r\nsubscribe\r\n${}\r\n{}\r\n:1\r\n", c[1].len(), c[1]);
                                    let _ = out.write_all(frame.as_bytes());
                                    fake.subscribed.store(true, Ordering::SeqCst);
                                    // From now on this connection gets what the test queues.
                                    let (mut push, fake) = (out.try_clone().unwrap(), fake.clone());
                                    std::thread::spawn(move || loop {
                                        std::thread::sleep(Duration::from_millis(20));
                                        let queued: Vec<String> = std::mem::take(&mut *fake.deliver.lock().unwrap());
                                        for m in queued {
                                            let frame = format!("*3\r\n$7\r\nmessage\r\n${}\r\n{CHANNEL}\r\n${}\r\n{m}\r\n", CHANNEL.len(), m.len());
                                            if push.write_all(frame.as_bytes()).is_err() {
                                                return;
                                            }
                                        }
                                    });
                                }
                                "AUTH" => {
                                    fake.auth.lock().unwrap().push(c[1..].to_vec());
                                    let _ = out.write_all(b"+OK\r\n");
                                }
                                _ => {
                                    let _ = out.write_all(b"+OK\r\n");
                                }
                            }
                        }
                    });
                }
            });
            port
        }

        fn wait(mut f: impl FnMut() -> bool) {
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(5) {
                if f() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out");
        }

        #[test]
        fn urls_follow_the_original_address_rules() {
            let mut c = Redis::default();
            assert!(url(&c).is_err());
            c.address = "cache.example".into();
            assert_eq!(url(&c).unwrap(), "redis://cache.example:6379/");
            c.address = "cache.example:6380".into();
            c.username = "lp".into();
            c.password = "p@ss:word/not-real".into();
            assert_eq!(url(&c).unwrap(), "redis://lp:p%40ss%3Aword%2Fnot-real@cache.example:6380/");
            c.ssl = true;
            assert!(url(&c).unwrap().starts_with("rediss://"));
            c.address = "[::1]:6381".into();
            c.username.clear();
            c.password.clear();
            assert_eq!(url(&c).unwrap(), "rediss://[::1]:6381/");
            // A cluster uses its first node.
            c.addresses = vec!["n1.example:7000".into(), "n2.example:7001".into()];
            assert_eq!(url(&c).unwrap(), "rediss://n1.example:7000/");
            c.address = "bad:port:x".into();
            c.addresses.clear();
            assert!(url(&c).is_err());
        }

        #[test]
        fn sentinel_nodes_default_to_26379_and_carry_the_sentinel_login() {
            let mut c = Redis::default();
            c.sentinel.enabled = true;
            assert!(sentinel_nodes(&c).is_err());
            c.sentinel.addresses = vec!["s1.example".into(), "s2.example:26380".into()];
            c.sentinel.password = "sp".into();
            assert_eq!(sentinel_nodes(&c).unwrap(), vec!["redis://:sp@s1.example:26379/", "redis://:sp@s2.example:26380/"]);
        }

        #[test]
        fn publishes_and_receives_on_the_luckperms_channel_and_logs_in() {
            let fake = Arc::new(FakeRedis::default());
            let port = serve(fake.clone());
            let mut cfg = Redis::default();
            cfg.enabled = true;
            cfg.address = format!("127.0.0.1:{port}");
            cfg.password = "not-a-real-password".into();
            let mut m = RedisMessenger::start(&cfg).unwrap();
            wait(|| fake.subscribed.load(Ordering::SeqCst));
            m.send("{\"id\":\"x\",\"type\":\"update\"}").unwrap();
            assert_eq!(fake.published.lock().unwrap().clone(), vec![(CHANNEL.to_owned(), "{\"id\":\"x\",\"type\":\"update\"}".to_owned())]);
            fake.deliver.lock().unwrap().push("{\"from\":\"elsewhere\"}".into());
            let mut got = Vec::new();
            wait(|| {
                got.extend(m.poll());
                !got.is_empty()
            });
            assert_eq!(got, vec!["{\"from\":\"elsewhere\"}".to_owned()]);
            assert!(fake.auth.lock().unwrap().iter().any(|a| a.last().map(String::as_str) == Some("not-a-real-password")), "the password was sent to the broker");
            m.close();
        }

        #[test]
        fn a_broker_that_is_down_is_an_error_without_the_password() {
            let mut cfg = Redis::default();
            cfg.address = "127.0.0.1:1".into();
            cfg.password = "very-secret-not-real".into();
            let mut m = RedisMessenger::start(&cfg).unwrap();
            let e = m.send("x").unwrap_err();
            assert!(!e.contains("very-secret"), "{e}");
            m.close();
        }
    }

    pub fn open(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
        log("Loading messaging service... [REDIS]");
        Ok(Box::new(RedisMessenger::start(&cfg.redis)?))
    }
}

#[cfg(feature = "redis")]
fn open_redis(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
    redis_messenger::open(cfg)
}

#[cfg(not(feature = "redis"))]
fn open_redis(_cfg: &Config) -> Result<Box<dyn Messenger>, String> {
    Err("built without the redis feature".into())
}

// ---- NATS ------------------------------------------------------------------------------------

#[cfg(feature = "wire")]
mod nats_messenger {
    use super::*;
    use crate::horizon_luckperms_core::config::Nats;
    use crate::horizon_luckperms_core::netio::{self, Stream};
    use std::io::{ErrorKind, Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    const DEFAULT_PORT: u16 = 4222;
    /// jnats `reconnectWait`.
    const RECONNECT: Duration = Duration::from_secs(5);
    /// Nothing for this long means the link is dead.
    const SILENCE: Duration = Duration::from_secs(90);
    const PING_EVERY: Duration = Duration::from_secs(30);
    /// A server may allow 64 MiB; nothing LuckPerms sends is near this.
    const MAX_PAYLOAD: usize = 1024 * 1024;

    #[derive(Clone)]
    pub struct Settings {
        pub host: String,
        pub port: u16,
        pub username: String,
        pub password: String,
        pub token: String,
        pub ssl: bool,
    }

    impl Settings {
        pub fn from_config(cfg: &Nats) -> Result<Settings, String> {
            let (host, port) = parse_address(&cfg.address, DEFAULT_PORT).map_err(|e| format!("nats.address: {e}"))?;
            Ok(Settings { host, port, username: cfg.username.clone(), password: cfg.password.clone(), token: cfg.token.clone(), ssl: cfg.ssl })
        }

        /// The `CONNECT` line: a user needs its password (as in jnats' `userInfo`), a token stands alone.
        pub fn connect_line(&self, tls_required: bool) -> String {
            let mut o = serde_json::json!({
                "verbose": false,
                "pedantic": false,
                "tls_required": tls_required,
                "name": "LuckPerms",
                "lang": "rust",
                "version": "5.5.0",
                "protocol": 1,
                "echo": true,
            });
            if !self.username.is_empty() && !self.password.is_empty() {
                o["user"] = self.username.clone().into();
                o["pass"] = self.password.clone().into();
            }
            if !self.token.is_empty() {
                o["auth_token"] = self.token.clone().into();
            }
            format!("CONNECT {o}\r\n")
        }
    }

    /// `PUB luckperms:update <n>` and the payload `writeUTF` made of the message.
    pub fn publish_frame(message: &str) -> Option<Vec<u8>> {
        let payload = write_utf(message)?;
        let mut frame = format!("PUB {CHANNEL} {}\r\n", payload.len()).into_bytes();
        frame.extend(payload);
        frame.extend(b"\r\n");
        Some(frame)
    }

    /// What one buffer of server bytes holds: the messages in it, the replies to send, and how much was used.
    #[derive(Debug, Default, PartialEq, Eq)]
    pub struct Parsed {
        pub messages: Vec<String>,
        pub replies: Vec<&'static str>,
        pub used: usize,
        /// A line that ends the session (`-ERR`).
        pub error: Option<String>,
    }

    fn crlf(buf: &[u8]) -> Option<usize> {
        buf.windows(2).position(|w| w == b"\r\n")
    }

    pub fn parse(buf: &[u8]) -> Result<Parsed, String> {
        let mut p = Parsed::default();
        loop {
            let rest = &buf[p.used..];
            let Some(end) = crlf(rest) else { return Ok(p) };
            let line = &rest[..end];
            if line.starts_with(b"MSG ") {
                let text = String::from_utf8_lossy(line);
                let n: usize = text.split_whitespace().last().and_then(|n| n.parse().ok()).ok_or("a MSG line without a size")?;
                if n > MAX_PAYLOAD {
                    return Err("a message is too big".into());
                }
                let need = end + 2 + n + 2;
                if rest.len() < need {
                    return Ok(p);
                }
                if let Some(m) = read_utf(&rest[end + 2..end + 2 + n]) {
                    p.messages.push(m);
                }
                p.used += need;
                continue;
            }
            if line == b"PING" {
                p.replies.push("PONG\r\n");
            } else if line.starts_with(b"-ERR") {
                let text = String::from_utf8_lossy(line);
                p.error = Some(text.chars().take(100).collect());
            }
            // INFO, +OK and PONG need nothing.
            p.used += end + 2;
            if p.error.is_some() {
                return Ok(p);
            }
        }
    }

    pub struct NatsMessenger {
        out: mpsc::SyncSender<Vec<u8>>,
        inbox: mpsc::Receiver<String>,
        stop: Arc<AtomicBool>,
        thread: Option<JoinHandle<()>>,
    }

    impl NatsMessenger {
        pub fn start(cfg: &Nats) -> Result<NatsMessenger, String> {
            let settings = Settings::from_config(cfg)?;
            let (out_tx, out_rx) = mpsc::sync_channel::<Vec<u8>>(1024);
            let (in_tx, in_rx) = mpsc::channel();
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = stop.clone();
            let thread = std::thread::Builder::new()
                .name("luckperms-nats".into())
                .spawn(move || run(settings, out_rx, in_tx, stop2))
                .map_err(|e| e.to_string())?;
            Ok(NatsMessenger { out: out_tx, inbox: in_rx, stop, thread: Some(thread) })
        }
    }

    fn nap(stop: &AtomicBool, total: Duration) {
        let step = Duration::from_millis(100);
        let mut left = total;
        while !stop.load(Ordering::Relaxed) && left > Duration::ZERO {
            let d = step.min(left);
            std::thread::sleep(d);
            left -= d;
        }
    }

    fn run(settings: Settings, out: mpsc::Receiver<Vec<u8>>, inbox: mpsc::Sender<String>, stop: Arc<AtomicBool>) {
        let mut down_reported = false;
        while !stop.load(Ordering::Relaxed) {
            match session(&settings, &out, &inbox, &stop, &mut down_reported) {
                Ok(()) => return,
                Err(e) => {
                    if !down_reported {
                        log(&format!("NATS connection lost, trying to reconnect: {e}"));
                        down_reported = true;
                    }
                }
            }
            nap(&stop, RECONNECT);
        }
    }

    fn read_line(stream: &mut Stream) -> Result<String, String> {
        let mut line = Vec::new();
        let mut b = [0u8; 1];
        while !line.ends_with(b"\r\n") {
            if line.len() > 16 * 1024 {
                return Err("the server's greeting is too long".into());
            }
            match stream.read(&mut b) {
                Ok(0) => return Err("connection closed".into()),
                Ok(_) => line.push(b[0]),
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(String::from_utf8_lossy(&line).trim_end().to_owned())
    }

    /// One connection, until it breaks (`Err`) or the messenger is stopped (`Ok`).
    fn session(s: &Settings, out: &mpsc::Receiver<Vec<u8>>, inbox: &mpsc::Sender<String>, stop: &AtomicBool, reported: &mut bool) -> Result<(), String> {
        let tcp = netio::connect_tcp(&s.host, s.port, Duration::from_secs(5))?;
        let mut stream = Stream::Plain(tcp);
        let info = read_line(&mut stream)?;
        let info_json = info.strip_prefix("INFO ").ok_or("the server did not greet with INFO")?;
        let tls_required = serde_json::from_str::<serde_json::Value>(info_json).ok().and_then(|v| v["tls_required"].as_bool()).unwrap_or(false);
        if s.ssl || tls_required {
            // NATS upgrades to TLS right after INFO, before CONNECT.
            let Stream::Plain(tcp) = stream else { return Err("unexpected stream".into()) };
            stream = netio::tls(tcp, &s.host)?;
        }
        let hello = format!("{}PING\r\nSUB {CHANNEL} 1\r\n", s.connect_line(s.ssl || tls_required));
        stream.write_all(hello.as_bytes()).and_then(|()| stream.flush()).map_err(|e| e.to_string())?;
        let _ = stream.tcp().set_read_timeout(Some(Duration::from_millis(200)));
        if *reported {
            log("NATS connection re-established");
            *reported = false;
        }
        let mut buf: Vec<u8> = Vec::new();
        let mut tmp = [0u8; 4096];
        let mut last_rx = Instant::now();
        let mut last_ping = Instant::now();
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            match stream.read(&mut tmp) {
                Ok(0) => return Err("connection closed".into()),
                Ok(n) => {
                    buf.extend_from_slice(&tmp[..n]);
                    last_rx = Instant::now();
                }
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(e) => return Err(e.to_string()),
            }
            let parsed = parse(&buf)?;
            buf.drain(..parsed.used);
            for m in parsed.messages {
                if inbox.send(m).is_err() {
                    return Ok(());
                }
            }
            for r in parsed.replies {
                stream.write_all(r.as_bytes()).map_err(|e| e.to_string())?;
            }
            if let Some(e) = parsed.error {
                return Err(e);
            }
            while let Ok(frame) = out.try_recv() {
                stream.write_all(&frame).map_err(|e| e.to_string())?;
            }
            if last_ping.elapsed() >= PING_EVERY {
                stream.write_all(b"PING\r\n").map_err(|e| e.to_string())?;
                last_ping = Instant::now();
            }
            stream.flush().map_err(|e| e.to_string())?;
            if last_rx.elapsed() >= SILENCE {
                return Err("the server stopped answering".into());
            }
        }
    }

    impl Messenger for NatsMessenger {
        fn name(&self) -> &'static str {
            "Nats"
        }

        fn send(&mut self, message: &str) -> Result<(), String> {
            let frame = publish_frame(message).ok_or("the message is too long for a NATS payload")?;
            self.out.try_send(frame).map_err(|_| "NATS: the send queue is full or the messenger is stopped".to_string())
        }

        fn poll(&mut self) -> Vec<String> {
            self.inbox.try_iter().collect()
        }

        fn close(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Drop for NatsMessenger {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn open(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
        log("Loading messaging service... [NATS]");
        Ok(Box::new(NatsMessenger::start(&cfg.nats)?))
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::io::BufRead;
        use std::net::TcpListener;
        use std::sync::Mutex;

        #[derive(Default)]
        struct FakeNats {
            connect: Mutex<Vec<String>>,
            subs: Mutex<Vec<String>>,
            published: Mutex<Vec<Vec<u8>>>,
            deliver: Mutex<Vec<Vec<u8>>>,
            greetings: std::sync::atomic::AtomicUsize,
        }

        fn serve(fake: Arc<FakeNats>) -> u16 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(stream) = stream else { continue };
                    let fake = fake.clone();
                    std::thread::spawn(move || {
                        fake.greetings.fetch_add(1, Ordering::SeqCst);
                        let mut out = stream.try_clone().unwrap();
                        out.write_all(b"INFO {\"server_id\":\"x\",\"version\":\"2.10.0\",\"max_payload\":1048576,\"tls_required\":false}\r\n").unwrap();
                        let (mut push, f2) = (out.try_clone().unwrap(), fake.clone());
                        std::thread::spawn(move || loop {
                            std::thread::sleep(Duration::from_millis(20));
                            let queued: Vec<Vec<u8>> = std::mem::take(&mut *f2.deliver.lock().unwrap());
                            for payload in queued {
                                let mut frame = format!("MSG {CHANNEL} 1 {}\r\n", payload.len()).into_bytes();
                                frame.extend(payload);
                                frame.extend(b"\r\n");
                                if push.write_all(&frame).is_err() {
                                    return;
                                }
                            }
                        });
                        let mut r = std::io::BufReader::new(stream);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            match r.read_line(&mut line) {
                                Ok(0) | Err(_) => return,
                                Ok(_) => {}
                            }
                            let l = line.trim_end().to_owned();
                            if l.starts_with("CONNECT ") {
                                fake.connect.lock().unwrap().push(l);
                            } else if l == "PING" {
                                let _ = out.write_all(b"PONG\r\n");
                            } else if l.starts_with("SUB ") {
                                fake.subs.lock().unwrap().push(l);
                            } else if let Some(rest) = l.strip_prefix("PUB ") {
                                let n: usize = rest.split_whitespace().last().unwrap().parse().unwrap();
                                let mut payload = vec![0u8; n + 2];
                                r.read_exact(&mut payload).unwrap();
                                payload.truncate(n);
                                fake.published.lock().unwrap().push(payload);
                            }
                        }
                    });
                }
            });
            port
        }

        fn wait(mut f: impl FnMut() -> bool) {
            let t = Instant::now();
            while t.elapsed() < Duration::from_secs(8) {
                if f() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            panic!("timed out");
        }

        #[test]
        fn java_utf_matches_data_output() {
            assert_eq!(write_utf("hello").unwrap(), vec![0, 5, b'h', b'e', b'l', b'l', b'o']);
            assert_eq!(write_utf("\0").unwrap(), vec![0, 2, 0xc0, 0x80]);
            assert_eq!(write_utf("\u{e9}").unwrap(), vec![0, 2, 0xc3, 0xa9]);
            assert_eq!(write_utf("\u{20ac}").unwrap(), vec![0, 3, 0xe2, 0x82, 0xac]);
            assert_eq!(write_utf("\u{1f600}").unwrap(), vec![0, 6, 0xed, 0xa0, 0xbd, 0xed, 0xb8, 0x80]);
            for s in ["", "plain", "a\0b", "caf\u{e9} \u{20ac} \u{1f600}", "{\"id\":\"x\"}"] {
                assert_eq!(read_utf(&write_utf(s).unwrap()).as_deref(), Some(s));
            }
            assert!(write_utf(&"x".repeat(65_536)).is_none());
            assert!(read_utf(&[0, 5, b'a']).is_none(), "truncated");
            assert!(read_utf(&[0, 1, 0xff]).is_none(), "not modified UTF-8");
        }

        #[test]
        fn frames_are_parsed_across_reads() {
            let payload = write_utf("{\"type\":\"update\"}").unwrap();
            let mut data = format!("INFO {{}}\r\nPING\r\n+OK\r\nMSG {CHANNEL} 1 {}\r\n", payload.len()).into_bytes();
            data.extend(&payload);
            data.extend(b"\r\n");
            let p = parse(&data).unwrap();
            assert_eq!(p.messages, vec!["{\"type\":\"update\"}"]);
            assert_eq!(p.replies, vec!["PONG\r\n"]);
            assert_eq!(p.used, data.len());
            // The same bytes cut in the middle of the payload: nothing is taken twice or lost.
            let cut = data.len() - 6;
            let first = parse(&data[..cut]).unwrap();
            assert!(first.messages.is_empty());
            assert!(first.used < cut, "the unfinished MSG stays in the buffer");
            let rest: Vec<u8> = data[first.used..].to_vec();
            assert_eq!(parse(&rest).unwrap().messages.len(), 1);
            assert_eq!(parse(b"-ERR 'Authorization Violation'\r\n").unwrap().error.as_deref(), Some("-ERR 'Authorization Violation'"));
            assert!(parse(format!("MSG {CHANNEL} 1 99999999\r\n").as_bytes()).is_err());
        }

        #[test]
        fn publish_frames_carry_write_utf() {
            let f = publish_frame("ab").unwrap();
            assert_eq!(f, b"PUB luckperms:update 4\r\n\x00\x02ab\r\n".to_vec());
        }

        #[test]
        fn connect_lines_follow_jnats_login_rules() {
            let mut s = Settings { host: "h".into(), port: 1, username: "u".into(), password: String::new(), token: String::new(), ssl: false };
            let v: serde_json::Value = serde_json::from_str(s.connect_line(false).trim_start_matches("CONNECT ").trim_end()).unwrap();
            assert!(v.get("user").is_none() && v.get("auth_token").is_none(), "a user without its password is not sent");
            s.password = "pw-not-real".into();
            s.token = "tok-not-real".into();
            let v: serde_json::Value = serde_json::from_str(s.connect_line(true).trim_start_matches("CONNECT ").trim_end()).unwrap();
            assert_eq!(v["user"], "u");
            assert_eq!(v["pass"], "pw-not-real");
            assert_eq!(v["auth_token"], "tok-not-real");
            assert_eq!(v["tls_required"], true);
            assert_eq!(v["name"], "LuckPerms");
        }

        #[test]
        fn it_publishes_subscribes_and_receives() {
            let fake = Arc::new(FakeNats::default());
            let port = serve(fake.clone());
            let mut cfg = Nats::default();
            cfg.enabled = true;
            cfg.address = format!("127.0.0.1:{port}");
            cfg.token = "tok-not-real".into();
            let mut m = NatsMessenger::start(&cfg).unwrap();
            wait(|| !fake.subs.lock().unwrap().is_empty());
            assert_eq!(fake.subs.lock().unwrap()[0], format!("SUB {CHANNEL} 1"));
            assert!(fake.connect.lock().unwrap()[0].contains("\"auth_token\":\"tok-not-real\""));
            m.send("{\"id\":\"x\"}").unwrap();
            wait(|| !fake.published.lock().unwrap().is_empty());
            assert_eq!(read_utf(&fake.published.lock().unwrap()[0]).as_deref(), Some("{\"id\":\"x\"}"));
            fake.deliver.lock().unwrap().push(write_utf("{\"from\":\"elsewhere\"}").unwrap());
            let mut got = Vec::new();
            wait(|| {
                got.extend(m.poll());
                !got.is_empty()
            });
            assert_eq!(got, vec!["{\"from\":\"elsewhere\"}".to_owned()]);
            m.close();
        }

        #[test]
        fn it_reconnects_after_the_server_drops_it() {
            let fake = Arc::new(FakeNats::default());
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            // The first connection is closed right after the greeting; the second one is served.
            let f2 = fake.clone();
            std::thread::spawn(move || {
                let (mut first, _) = listener.accept().unwrap();
                let _ = first.write_all(b"INFO {\"tls_required\":false}\r\n");
                drop(first);
                let (second, _) = listener.accept().unwrap();
                let mut out = second.try_clone().unwrap();
                let _ = out.write_all(b"INFO {\"tls_required\":false}\r\n");
                let mut r = std::io::BufReader::new(second);
                let mut line = String::new();
                while r.read_line(&mut line).unwrap_or(0) > 0 {
                    if line.starts_with("SUB ") {
                        f2.subs.lock().unwrap().push(line.trim_end().to_owned());
                    }
                    line.clear();
                }
            });
            let mut cfg = Nats::default();
            cfg.address = format!("127.0.0.1:{port}");
            let mut m = NatsMessenger::start(&cfg).unwrap();
            // jnats waits five seconds between attempts, so does this.
            let t = Instant::now();
            while fake.subs.lock().unwrap().is_empty() && t.elapsed() < Duration::from_secs(15) {
                std::thread::sleep(Duration::from_millis(50));
            }
            assert_eq!(fake.subs.lock().unwrap().len(), 1, "the second connection subscribed");
            m.close();
        }

        #[test]
        fn settings_need_an_address() {
            assert!(Settings::from_config(&Nats::default()).is_err());
            let mut n = Nats::default();
            n.address = "nats.example".into();
            let s = Settings::from_config(&n).unwrap();
            assert_eq!((s.host.as_str(), s.port), ("nats.example", 4222));
        }
    }
}

#[cfg(feature = "wire")]
fn open_nats(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
    nats_messenger::open(cfg)
}

#[cfg(not(feature = "wire"))]
fn open_nats(_cfg: &Config) -> Result<Box<dyn Messenger>, String> {
    Err("built without the wire feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_follow_hostandport() {
        assert_eq!(parse_address("localhost", 6379).unwrap(), ("localhost".into(), 6379));
        assert_eq!(parse_address(" db.example:7000 ", 6379).unwrap(), ("db.example".into(), 7000));
        assert_eq!(parse_address("[::1]", 4222).unwrap(), ("::1".into(), 4222));
        assert_eq!(parse_address("[2001:db8::1]:9", 1).unwrap(), ("2001:db8::1".into(), 9));
        for bad in ["", ":80", "::1", "a:b", "a:99999", "[::1", "[::1]x", "[::1]:"] {
            assert!(parse_address(bad, 1).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refusals_say_why() {
        assert!(refusal("rabbitmq").contains("AMQP"));
        assert!(refusal("postgresql").contains("PostgreSQL client"));
        assert!(refusal("pluginmsg").contains("proxy"));
    }

    #[test]
    fn secrets_are_scrubbed() {
        assert_eq!(scrub("login failed for hunter2-x".into(), &["hunter2-x", "ab"]), "login failed for ***");
    }

    #[test]
    fn open_picks_nothing_for_sql_and_refuses_the_unavailable() {
        let mut c = Config::default();
        assert!(open(&c).unwrap().is_none(), "auto with MariaDB is the SQL messenger, carried by the storage");
        c.rabbitmq.enabled = true;
        assert!(open(&c).err().unwrap().contains("AMQP"));
        let mut c = Config::default();
        c.messaging_service = "pluginmsg".into();
        assert!(open(&c).err().unwrap().contains("pluginmsg"));
        let mut c = Config::default();
        c.storage.method = "yaml".into();
        assert!(open(&c).unwrap().is_none());
        // A config that names Redis without an address fails with the key, not with a panic.
        let mut c = Config::default();
        c.redis.enabled = true;
        assert!(open(&c).err().unwrap().contains("redis.address"));
        let mut c = Config::default();
        c.nats.enabled = true;
        assert!(open(&c).err().unwrap().contains("nats.address"));
    }
}
