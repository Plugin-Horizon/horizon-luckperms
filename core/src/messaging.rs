//! The messaging service: the JSON messages LuckPerms instances exchange (`update`,
//! `userupdate`, `log`, `custom`) and the bookkeeping around them (ids already seen, the
//! buffered update push). The SQL transport (`{prefix}messenger`) lives in the storage.

use crate::actionlog::Action;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    /// Everything changed: reload groups, tracks and online users.
    Update { id: u128 },
    /// One user changed.
    UserUpdate { id: u128, user: u128 },
    /// An action was logged elsewhere.
    Log { id: u128, action: Action },
    /// A plugin's own payload (kept, not interpreted).
    Custom { id: u128, channel: String, payload: String },
}

impl Message {
    pub fn id(&self) -> u128 {
        match self {
            Message::Update { id } | Message::UserUpdate { id, .. } | Message::Log { id, .. } | Message::Custom { id, .. } => *id,
        }
    }

    /// `LuckPermsMessagingService.encodeMessageAsString`: `{"id":..,"type":..,"content":..}`.
    pub fn encode(&self) -> String {
        let (ty, content) = match self {
            Message::Update { .. } => ("update", None),
            Message::UserUpdate { user, .. } => ("userupdate", Some(json!({"userUuid": crate::uuid::format(*user)}))),
            Message::Log { action, .. } => ("log", Some(action.to_json())),
            Message::Custom { channel, payload, .. } => ("custom", Some(json!({"channelId": channel, "payload": payload}))),
        };
        let mut obj = json!({"id": crate::uuid::format(self.id()), "type": ty});
        if let Some(c) = content {
            obj["content"] = c;
        }
        obj.to_string()
    }

    /// None for a message this version does not know or cannot read (LuckPerms ignores those
    /// too).
    pub fn decode(s: &str) -> Option<Message> {
        let v: Value = serde_json::from_str(s).ok()?;
        let id = crate::uuid::parse(v.get("id")?.as_str()?)?;
        let content = v.get("content");
        match v.get("type")?.as_str()? {
            "update" => Some(Message::Update { id }),
            "userupdate" => {
                let user = crate::uuid::parse(content?.get("userUuid")?.as_str()?)?;
                Some(Message::UserUpdate { id, user })
            }
            "log" => Some(Message::Log { id, action: Action::from_json(content?)? }),
            "custom" => {
                let c = content?;
                Some(Message::Custom {
                    id,
                    channel: c.get("channelId")?.as_str()?.to_owned(),
                    payload: c.get("payload")?.as_str()?.to_owned(),
                })
            }
            _ => None,
        }
    }
}

/// Message ids seen in the last five minutes, ours included, so nothing is handled twice
/// (`receivedMessages`, an expiring set).
#[derive(Debug, Default)]
pub struct SeenIds {
    ids: HashMap<u128, Instant>,
}

impl SeenIds {
    const KEEP: Duration = Duration::from_secs(300);

    /// True when the id is new (and remembers it).
    pub fn add(&mut self, id: u128) -> bool {
        let now = Instant::now();
        self.ids.retain(|_, t| now.duration_since(*t) < Self::KEEP);
        self.ids.insert(id, now).is_none()
    }
}

/// `PushUpdateBuffer`: several changes within two seconds become one `update` ping.
#[derive(Debug, Default)]
pub struct UpdateBuffer {
    due: Option<Instant>,
}

impl UpdateBuffer {
    const DELAY: Duration = Duration::from_secs(2);

    pub fn request(&mut self) {
        self.due = Some(Instant::now() + Self::DELAY);
    }

    /// True once the delay has run out since the last request.
    pub fn take_due(&mut self) -> bool {
        match self.due {
            Some(t) if Instant::now() >= t => {
                self.due = None;
                true
            }
            _ => false,
        }
    }

    pub fn pending(&self) -> bool {
        self.due.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actionlog::TargetType;

    #[test]
    fn encodes_like_luckperms() {
        let id = crate::uuid::parse("d3a1f0e2-0000-4000-8000-000000000001").unwrap();
        assert_eq!(Message::Update { id }.encode(), r#"{"id":"d3a1f0e2-0000-4000-8000-000000000001","type":"update"}"#);
        let user = crate::uuid::parse("069a79f4-44e9-4726-a5be-fca90e38aaf5").unwrap();
        let m = Message::UserUpdate { id, user };
        // Key order differs from Gson's (it does not matter to a JSON reader); compare values.
        let want: serde_json::Value = serde_json::from_str(
            r#"{"id":"d3a1f0e2-0000-4000-8000-000000000001","type":"userupdate","content":{"userUuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5"}}"#,
        )
        .unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&m.encode()).unwrap(), want);
        assert_eq!(Message::decode(&m.encode()), Some(m));
    }

    #[test]
    fn decodes_gson_order_and_unknown() {
        let s = r#"{"id":"d3a1f0e2-0000-4000-8000-000000000001","type":"userupdate","content":{"userUuid":"069a79f4-44e9-4726-a5be-fca90e38aaf5"}}"#;
        assert!(matches!(Message::decode(s), Some(Message::UserUpdate { .. })));
        assert_eq!(Message::decode(r#"{"id":"d3a1f0e2-0000-4000-8000-000000000001","type":"future"}"#), None);
        assert_eq!(Message::decode("not json"), None);
    }

    #[test]
    fn log_and_custom() {
        let a = Action {
            timestamp: 1,
            source_uuid: 0,
            source_name: "Console".into(),
            target_type: TargetType::Group,
            target_uuid: None,
            target_name: "mod".into(),
            description: "setweight 10".into(),
        };
        let m = Message::Log { id: 9, action: a };
        assert_eq!(Message::decode(&m.encode()), Some(m));
        let c = Message::Custom { id: 3, channel: "x".into(), payload: "y".into() };
        assert_eq!(Message::decode(&c.encode()), Some(c));
    }

    #[test]
    fn seen_ids() {
        let mut s = SeenIds::default();
        assert!(s.add(1));
        assert!(!s.add(1));
        assert!(s.add(2));
    }
}
