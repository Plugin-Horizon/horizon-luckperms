//! The action log: one row of `{prefix}actions` per change, and the JSON the messenger's `log`
//! messages carry (`ActionJsonSerializer`).

use crate::context::ContextSet;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetType {
    User,
    Group,
    Track,
}

impl TargetType {
    /// The `type` column: `U`, `G` or `T`.
    pub fn code(self) -> char {
        match self {
            TargetType::User => 'U',
            TargetType::Group => 'G',
            TargetType::Track => 'T',
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            TargetType::User => "USER",
            TargetType::Group => "GROUP",
            TargetType::Track => "TRACK",
        }
    }
    /// `LoggedAction.parseType`: a full name or its first letter.
    pub fn parse(s: &str) -> Option<Self> {
        match s.chars().next()?.to_ascii_uppercase() {
            'U' => Some(TargetType::User),
            'G' => Some(TargetType::Group),
            'T' => Some(TargetType::Track),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    /// Unix seconds.
    pub timestamp: i64,
    pub source_uuid: u128,
    pub source_name: String,
    pub target_type: TargetType,
    /// None for groups and tracks (stored as the text `null`).
    pub target_uuid: Option<u128>,
    pub target_name: String,
    pub description: String,
}

/// One word of a description: plain text, or contexts written as `server=x world=y k=v`.
pub enum Part<'a> {
    Text(&'a str),
    Owned(String),
    Contexts(&'a ContextSet),
}

/// `LoggedAction.Builder.description(Object...)`: the parts joined by spaces.
pub fn describe(parts: &[Part<'_>]) -> String {
    let mut out: Vec<String> = Vec::new();
    for p in parts {
        match p {
            Part::Text(t) => out.push((*t).to_owned()),
            Part::Owned(t) => out.push(t.clone()),
            Part::Contexts(c) => {
                for (k, v) in c.ordered_pairs() {
                    out.push(format!("{k}={v}"));
                }
            }
        }
    }
    out.join(" ")
}

impl Action {
    pub fn to_json(&self) -> Value {
        let mut target = json!({"type": self.target_type.name(), "name": self.target_name});
        if let Some(u) = self.target_uuid {
            target["uniqueId"] = Value::String(crate::uuid::format(u));
        }
        json!({
            "timestamp": self.timestamp,
            "source": {"uniqueId": crate::uuid::format(self.source_uuid), "name": self.source_name},
            "target": target,
            "description": self.description,
        })
    }

    /// Reads the current and the legacy layout (`actor`, `actorName`, `type`, `acted`,
    /// `actedName`, `action`).
    pub fn from_json(v: &Value) -> Option<Action> {
        let timestamp = v.get("timestamp").and_then(Value::as_i64).unwrap_or_else(crate::duration::now);
        let (source_uuid, source_name) = if let Some(s) = v.get("source") {
            (crate::uuid::parse(s.get("uniqueId")?.as_str()?)?, s.get("name")?.as_str()?.to_owned())
        } else {
            (crate::uuid::parse(v.get("actor")?.as_str()?)?, v.get("actorName")?.as_str()?.to_owned())
        };
        let (target_type, target_uuid, target_name) = if let Some(t) = v.get("target") {
            (
                TargetType::parse(t.get("type")?.as_str()?)?,
                t.get("uniqueId").and_then(Value::as_str).and_then(crate::uuid::parse),
                t.get("name")?.as_str()?.to_owned(),
            )
        } else {
            (
                TargetType::parse(v.get("type")?.as_str()?)?,
                v.get("acted").and_then(Value::as_str).and_then(crate::uuid::parse),
                v.get("actedName")?.as_str()?.to_owned(),
            )
        };
        let description = v
            .get("description")
            .or_else(|| v.get("action"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        Some(Action { timestamp, source_uuid, source_name, target_type, target_uuid, target_name, description })
    }

    /// `Message.LOG`: the two lines players with `luckperms.log.notify` get.
    pub fn notify_line(&self, _remote: bool) -> String {
        let blank = |s: &str| s.is_empty() || s == "null";
        let source = if blank(&self.source_name) { crate::uuid::format(self.source_uuid) } else { self.source_name.clone() };
        let target = if blank(&self.target_name) {
            self.target_uuid.map(crate::uuid::format).unwrap_or_else(|| self.target_name.clone())
        } else {
            self.target_name.clone()
        };
        let p = crate::text::PREFIX;
        format!(
            "{p}\u{a7}3LOG \u{a7}3\u{a7}l> \u{a7}8(\u{a7}e{source}\u{a7}8) \u{a7}8[\u{a7}a{}\u{a7}8] \u{a7}8(\u{a7}b{target}\u{a7}8)\n{p}\u{a7}3LOG \u{a7}3\u{a7}l> \u{a7}f{}",
            self.target_type.code(),
            self.description
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trip() {
        let a = Action {
            timestamp: 1_700_000_000,
            source_uuid: 0,
            source_name: "Console".into(),
            target_type: TargetType::User,
            target_uuid: Some(5),
            target_name: "alice".into(),
            description: "permission set a.b true server=x".into(),
        };
        let back = Action::from_json(&a.to_json()).unwrap();
        assert_eq!(back, a);
        let g = Action { target_type: TargetType::Group, target_uuid: None, target_name: "mod".into(), ..a.clone() };
        let j = g.to_json();
        assert!(j["target"].get("uniqueId").is_none());
        assert_eq!(Action::from_json(&j).unwrap(), g);
    }

    #[test]
    fn legacy_layout() {
        let v = serde_json::json!({
            "actor": "00000000-0000-0000-0000-000000000000", "actorName": "Console",
            "type": "G", "actedName": "mod", "action": "setweight 5"
        });
        let a = Action::from_json(&v).unwrap();
        assert_eq!(a.target_type, TargetType::Group);
        assert_eq!(a.description, "setweight 5");
    }

    #[test]
    fn description_contexts() {
        let c = ContextSet::of(&[("world", "w"), ("server", "s"), ("gamemode", "creative")]);
        let d = describe(&[Part::Text("permission"), Part::Text("set"), Part::Text("a.b"), Part::Text("true"), Part::Contexts(&c)]);
        assert_eq!(d, "permission set a.b true server=s world=w gamemode=creative");
    }
}
