//! The messaging services that need wave 2 engine or dependency work: `pluginmsg` (plugin messages over a player's
//! connection to a relay on the proxy: branch fam-provider-bridges, `register_plugin_channel`, hook `plugin_message`,
//! `PlayerRef::send_plugin_message`), `postgresql` (LISTEN/NOTIFY) and `rabbitmq` (AMQP 0-9-1), both from the
//! clients fam-mod-deps-db-clients adds to `horizon-mod-deps`. The plugin-message buffers are plain code (tested);
//! the other two are behind the cargo feature `wave2` and not compiled yet.
//!
//! All three carry the same text as the other transports: the JSON of `messaging.rs`, here as `writeUTF` (a two byte
//! big-endian length and the UTF-8 bytes) for pluginmsg and RabbitMQ, as plain text for NOTIFY.

use crate::horizon_luckperms_core::messenger::Messenger;
use std::sync::Mutex;

/// `AbstractPluginMessageMessenger.CHANNEL`.
pub const PLUGIN_CHANNEL: &str = "luckperms:update";
/// Most outgoing frames kept while no player is online to carry them.
const MAX_PENDING: usize = 256;

static OUT: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());
static IN: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// `ByteArrayDataOutput.writeUTF` for text of up to 65535 bytes.
pub fn write_utf(s: &str) -> Option<Vec<u8>> {
    let b = s.as_bytes();
    let n = u16::try_from(b.len()).ok()?;
    let mut out = n.to_be_bytes().to_vec();
    out.extend_from_slice(b);
    Some(out)
}

/// `ByteArrayDataInput.readUTF`.
pub fn read_utf(buf: &[u8]) -> Option<String> {
    let n = u16::from_be_bytes([*buf.first()?, *buf.get(1)?]) as usize;
    String::from_utf8(buf.get(2..2 + n)?.to_vec()).ok()
}

/// The `plugin_message` hook for `luckperms:update` calls this with the payload.
pub fn plugin_message_received(data: &[u8]) {
    if let Some(s) = read_utf(data) {
        let mut q = IN.lock().unwrap_or_else(|e| e.into_inner());
        if q.len() < MAX_PENDING {
            q.push(s);
        }
    }
}

/// Frames waiting to be sent; the caller sends them through a player (`send_plugin_message`) and drops them only
/// when one was online, as `PluginMessageMessenger` sends nothing without a player.
pub fn take_outgoing() -> Vec<Vec<u8>> {
    std::mem::take(&mut *OUT.lock().unwrap_or_else(|e| e.into_inner()))
}

/// Puts frames back (no player was online).
pub fn requeue_outgoing(frames: Vec<Vec<u8>>) {
    let mut q = OUT.lock().unwrap_or_else(|e| e.into_inner());
    q.splice(0..0, frames);
    let over = q.len().saturating_sub(MAX_PENDING);
    q.drain(..over);
}

pub struct PluginMessenger;

impl Messenger for PluginMessenger {
    fn name(&self) -> &'static str {
        "PluginMessage"
    }
    fn send(&mut self, message: &str) -> Result<(), String> {
        let frame = write_utf(message).ok_or("the message is longer than a plugin message can hold")?;
        requeue_outgoing(vec![frame]);
        Ok(())
    }
    fn poll(&mut self) -> Vec<String> {
        std::mem::take(&mut *IN.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(feature = "wave2")]
pub use external::{open_postgres, open_rabbitmq};

#[cfg(feature = "wave2")]
mod external {
    use super::*;
    use crate::horizon_luckperms_core::config::Config;
    use crate::horizon_luckperms_core::messenger::{parse_address, CHANNEL};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    // ---- PostgreSQL LISTEN/NOTIFY ---------------------------------------------------------------------------

    struct PgMessenger {
        sender: postgres::Client,
        inbox: mpsc::Receiver<String>,
        open: Arc<AtomicBool>,
    }

    pub fn open_postgres(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
        use postgres::fallible_iterator::FallibleIterator;
        let sender = crate::horizon_luckperms_core::storage::postgres::connect(&cfg.storage)?;
        let (tx, rx) = mpsc::channel();
        let open = Arc::new(AtomicBool::new(true));
        let (flag, storage) = (open.clone(), cfg.storage.clone());
        std::thread::spawn(move || {
            while flag.load(Ordering::Relaxed) {
                // (re)connect every five seconds, as the original's connection check does
                let Ok(mut c) = crate::horizon_luckperms_core::storage::postgres::connect(&storage) else {
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                };
                if c.batch_execute(&format!("LISTEN \"{CHANNEL}\"")).is_err() {
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
                while flag.load(Ordering::Relaxed) {
                    let mut it = c.notifications();
                    let mut got = it.timeout_iter(Duration::from_secs(1));
                    match got.next() {
                        Ok(Some(n)) => {
                            let _ = tx.send(n.payload().to_owned());
                        }
                        Ok(None) => {}
                        Err(_) => break,
                    }
                }
            }
        });
        Ok(Box::new(PgMessenger { sender, inbox: rx, open }))
    }

    impl Messenger for PgMessenger {
        fn name(&self) -> &'static str {
            "PostgreSQL"
        }
        fn send(&mut self, message: &str) -> Result<(), String> {
            self.sender.execute("SELECT pg_notify($1, $2)", &[&CHANNEL, &message]).map(|_| ()).map_err(|e| format!("postgres notify: {e}"))
        }
        fn poll(&mut self) -> Vec<String> {
            self.inbox.try_iter().collect()
        }
        fn close(&mut self) {
            self.open.store(false, Ordering::Relaxed);
        }
    }

    // ---- RabbitMQ -------------------------------------------------------------------------------------------

    const EXCHANGE: &str = "luckperms";

    struct AmqpMessenger {
        out: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
        inbox: mpsc::Receiver<String>,
        open: Arc<AtomicBool>,
    }

    fn enc(s: &str) -> String {
        s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
    }

    pub fn open_rabbitmq(cfg: &Config) -> Result<Box<dyn Messenger>, String> {
        let r = &cfg.rabbitmq;
        let (host, port) = parse_address(&r.address, 5672)?;
        let host = if host.contains(':') { format!("[{host}]") } else { host };
        let uri = format!("amqp://{}:{}@{host}:{port}/{}", enc(&r.username), enc(&r.password), enc(&r.vhost));
        let (otx, mut orx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
        let (itx, irx) = mpsc::channel();
        let open = Arc::new(AtomicBool::new(true));
        let flag = open.clone();
        std::thread::spawn(move || {
            let Ok(rt) = tokio::runtime::Builder::new_current_thread().enable_all().build() else { return };
            rt.block_on(async move {
                use lapin::options::{BasicGetOptions, BasicPublishOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions};
                use lapin::types::FieldTable;
                while flag.load(Ordering::Relaxed) {
                    let attempt = async {
                        let conn = lapin::Connection::connect(&uri, lapin::ConnectionProperties::default()).await?;
                        let ch = conn.create_channel().await?;
                        let q = ch.queue_declare("".into(), QueueDeclareOptions { durable: false, exclusive: true, auto_delete: true, ..Default::default() }, FieldTable::default()).await?;
                        ch.exchange_declare(EXCHANGE.into(), lapin::ExchangeKind::Topic, ExchangeDeclareOptions { durable: false, auto_delete: true, ..Default::default() }, FieldTable::default()).await?;
                        ch.queue_bind(q.name().clone(), EXCHANGE.into(), CHANNEL.into(), QueueBindOptions::default(), FieldTable::default()).await?;
                        while flag.load(Ordering::Relaxed) {
                            while let Ok(frame) = orx.try_recv() {
                                ch.basic_publish(EXCHANGE.into(), CHANNEL.into(), BasicPublishOptions::default(), &frame, lapin::BasicProperties::default()).await?;
                            }
                            while let Some(m) = ch.basic_get(q.name().clone(), BasicGetOptions { no_ack: true }).await? {
                                if let Some(s) = read_utf(&m.data) {
                                    let _ = itx.send(s);
                                }
                            }
                            tokio::time::sleep(Duration::from_millis(200)).await;
                        }
                        Ok::<(), lapin::Error>(())
                    };
                    if attempt.await.is_err() {
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            });
        });
        Ok(Box::new(AmqpMessenger { out: otx, inbox: irx, open }))
    }

    impl Messenger for AmqpMessenger {
        fn name(&self) -> &'static str {
            "RabbitMQ"
        }
        fn send(&mut self, message: &str) -> Result<(), String> {
            let frame = write_utf(message).ok_or("the message is longer than 65535 bytes")?;
            self.out.send(frame).map_err(|_| "the RabbitMQ thread has stopped".to_string())
        }
        fn poll(&mut self) -> Vec<String> {
            self.inbox.try_iter().collect()
        }
        fn close(&mut self) {
            self.open.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_utf_matches_java() {
        assert_eq!(write_utf("hi").unwrap(), vec![0, 2, b'h', b'i']);
        assert_eq!(read_utf(&[0, 2, b'h', b'i']).as_deref(), Some("hi"));
        assert_eq!(read_utf(&[0, 9, b'h']), None);
        assert!(write_utf(&"x".repeat(70_000)).is_none());
    }

    #[test]
    fn the_plugin_messenger_buffers_both_ways() {
        let mut m = PluginMessenger;
        m.send("{\"a\":1}").unwrap();
        let out = take_outgoing();
        assert_eq!(out.len(), 1);
        plugin_message_received(&out[0]);
        assert_eq!(m.poll(), vec!["{\"a\":1}".to_string()]);
        assert!(m.poll().is_empty());
        requeue_outgoing(vec![vec![1]]);
        requeue_outgoing(vec![vec![0]]);
        assert_eq!(take_outgoing(), vec![vec![0], vec![1]]);
    }
}
