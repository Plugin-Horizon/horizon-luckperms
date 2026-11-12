//! Context sets: `key=value` pairs a node applies in (server, world, gamemode, dimension-type,
//! anything a config adds), with LuckPerms' matching, ordering and JSON forms.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

pub const SERVER: &str = "server";
pub const WORLD: &str = "world";
pub const GAMEMODE: &str = "gamemode";
pub const DIMENSION_TYPE: &str = "dimension-type";

/// How a node's contexts must be met by a player's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatisfyMode {
    /// Each key of the node: at least one of its values among the player's (the default).
    AtLeastOneValuePerKey,
    /// Every pair of the node among the player's.
    AllValuesPerKey,
}

/// A set of contexts; keys and values are lower case, `server=global` and `world=global`
/// mean "no context" and are never stored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ContextSet {
    map: BTreeMap<String, BTreeSet<String>>,
}

impl ContextSet {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn of(pairs: &[(&str, &str)]) -> Self {
        let mut s = Self::new();
        for (k, v) in pairs {
            s.add(k, v);
        }
        s
    }

    /// Adds a pair; a blank key or value, or a global server/world, is ignored. Like LuckPerms
    /// the pair is only lower-cased, not trimmed.
    pub fn add(&mut self, key: &str, value: &str) {
        let k = key.to_lowercase();
        let v = value.to_lowercase();
        if k.trim().is_empty() || v.trim().is_empty() || is_global_entry(&k, &v) {
            return;
        }
        self.map.entry(k).or_default().insert(v);
    }

    pub fn add_all(&mut self, other: &ContextSet) {
        for (k, v) in other.pairs() {
            self.add(k, v);
        }
    }

    pub fn remove_key(&mut self, key: &str) {
        self.map.remove(key);
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// The number of pairs.
    pub fn len(&self) -> usize {
        self.map.values().map(BTreeSet::len).sum()
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.map.contains_key(key)
    }

    pub fn contains(&self, key: &str, value: &str) -> bool {
        self.map.get(key).is_some_and(|s| s.contains(value))
    }

    pub fn values(&self, key: &str) -> Vec<&str> {
        self.map.get(key).map(|s| s.iter().map(String::as_str).collect()).unwrap_or_default()
    }

    /// Pairs in key order, then value order (`ContextComparator`).
    pub fn pairs(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().flat_map(|(k, vs)| vs.iter().map(move |v| (k.as_str(), v.as_str())))
    }

    /// True when `other` (a player's contexts) meets this set (a node's).
    pub fn is_satisfied_by(&self, other: &ContextSet, mode: SatisfyMode) -> bool {
        if self.is_empty() {
            return true;
        }
        if other.is_empty() {
            return false;
        }
        match mode {
            SatisfyMode::AllValuesPerKey => self.pairs().all(|(k, v)| other.contains(k, v)),
            SatisfyMode::AtLeastOneValuePerKey => self
                .map
                .iter()
                .all(|(k, vs)| vs.iter().any(|v| other.contains(k, v))),
        }
    }

    /// The JSON the `contexts` column holds: keys sorted, one value as a string, several as a
    /// sorted array (`ContextSetJsonSerializer`).
    pub fn to_json(&self) -> String {
        let mut obj = serde_json::Map::new();
        for (k, vs) in &self.map {
            let v = if vs.len() == 1 {
                serde_json::Value::String(vs.iter().next().cloned().unwrap_or_default())
            } else {
                serde_json::Value::Array(vs.iter().cloned().map(serde_json::Value::String).collect())
            };
            obj.insert(k.clone(), v);
        }
        serde_json::Value::Object(obj).to_string()
    }

    /// Reads the `contexts` column; anything unreadable is the empty set.
    pub fn from_json(s: &str) -> Self {
        let mut set = Self::new();
        let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(s) else {
            return set;
        };
        for (k, v) in obj {
            match v {
                serde_json::Value::Array(a) => {
                    for e in a {
                        if let Some(s) = json_str(&e) {
                            set.add(&k, &s);
                        }
                    }
                }
                other => {
                    if let Some(s) = json_str(&other) {
                        set.add(&k, &s);
                    }
                }
            }
        }
        set
    }

    /// As LuckPerms prints contexts in chat: `server=x, world=y` (server and world first), or
    /// `global`.
    pub fn display(&self) -> String {
        if self.is_empty() {
            return "global".into();
        }
        self.ordered_pairs().iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(", ")
    }

    /// Server values, then world values, then the rest: the order of the action log's
    /// description (`LoggedAction.Builder.description`).
    pub fn ordered_pairs(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for v in self.values(SERVER) {
            out.push((SERVER.to_owned(), v.to_owned()));
        }
        for v in self.values(WORLD) {
            out.push((WORLD.to_owned(), v.to_owned()));
        }
        for (k, v) in self.pairs() {
            if k != SERVER && k != WORLD {
                out.push((k.to_owned(), v.to_owned()));
            }
        }
        out
    }

    /// `ContextSetComparator` ascending: an empty set is least, then sets with a server, then
    /// with a world, then by size, then by content.
    pub fn compare_specificity(&self, other: &ContextSet) -> Ordering {
        if self == other {
            return Ordering::Equal;
        }
        (!self.is_empty())
            .cmp(&!other.is_empty())
            .then(self.contains_key(SERVER).cmp(&other.contains_key(SERVER)))
            .then(self.contains_key(WORLD).cmp(&other.contains_key(WORLD)))
            .then(self.len().cmp(&other.len()))
            .then_with(|| {
                let a: Vec<_> = self.pairs().collect();
                let b: Vec<_> = other.pairs().collect();
                a.cmp(&b)
            })
    }
}

fn json_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

pub fn is_global_entry(key: &str, value: &str) -> bool {
    (key == SERVER || key == WORLD) && value == "global"
}

/// The flags of a query that decide whether nodes without a server/world context count
/// (`include-global`, `include-global-world`, `apply-global-groups`, `apply-global-world-groups`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlobalFlags {
    pub include_global: bool,
    pub include_global_world: bool,
    pub apply_global_groups: bool,
    pub apply_global_world_groups: bool,
}

impl Default for GlobalFlags {
    fn default() -> Self {
        Self { include_global: true, include_global_world: true, apply_global_groups: true, apply_global_world_groups: true }
    }
}

impl GlobalFlags {
    /// Normal (non-inheritance) nodes of a set are left out (`normalNodesExcludeTest`).
    pub fn excludes_normal(&self, set: &ContextSet) -> bool {
        (!self.include_global && !set.contains_key(SERVER)) || (!self.include_global_world && !set.contains_key(WORLD))
    }

    /// Inheritance nodes of a set still count (`inheritanceNodesIncludeTest`).
    pub fn includes_inheritance(&self, set: &ContextSet) -> bool {
        !((!self.apply_global_groups && !set.contains_key(SERVER)) || (!self.apply_global_world_groups && !set.contains_key(WORLD)))
    }
}

/// The contexts of a command's trailing arguments (`ArgumentParser.parseContext`):
/// `key=value` pairs; a bare word is the server, a second bare word the world.
pub fn parse_args(args: &[&str]) -> ContextSet {
    let mut set = ContextSet::new();
    let mut bare = 0;
    for a in args {
        if let Some((k, v)) = a.split_once('=') {
            set.add(k, v);
        } else if !a.is_empty() {
            match bare {
                0 => set.add(SERVER, a),
                1 => set.add(WORLD, a),
                _ => {}
            }
            bare += 1;
        }
    }
    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_not_trimmed() {
        let s = ContextSet::of(&[("world", " Nether ")]);
        assert!(s.contains("world", " nether "));
        assert!(!s.contains("world", "nether"));
        assert!(ContextSet::of(&[("world", "   ")]).is_empty());
    }

    #[test]
    fn global_is_dropped_and_lowercased() {
        let s = ContextSet::of(&[("server", "global"), ("World", "Nether"), ("world", "global")]);
        assert_eq!(s.len(), 1);
        assert!(s.contains("world", "nether"));
    }

    #[test]
    fn json_round_trip_matches_gson() {
        let s = ContextSet::of(&[("gamemode", "creative"), ("foo", "b"), ("foo", "a")]);
        assert_eq!(s.to_json(), r#"{"foo":["a","b"],"gamemode":"creative"}"#);
        assert_eq!(ContextSet::from_json(&s.to_json()), s);
        assert_eq!(ContextSet::new().to_json(), "{}");
        assert!(ContextSet::from_json("{}").is_empty());
        assert!(ContextSet::from_json("garbage").is_empty());
    }

    #[test]
    fn satisfy_modes() {
        let node = ContextSet::of(&[("world", "a"), ("world", "b")]);
        let player = ContextSet::of(&[("world", "a"), ("server", "x")]);
        assert!(node.is_satisfied_by(&player, SatisfyMode::AtLeastOneValuePerKey));
        assert!(!node.is_satisfied_by(&player, SatisfyMode::AllValuesPerKey));
        assert!(ContextSet::new().is_satisfied_by(&player, SatisfyMode::AllValuesPerKey));
        assert!(!node.is_satisfied_by(&ContextSet::new(), SatisfyMode::AtLeastOneValuePerKey));
        let other = ContextSet::of(&[("server", "y")]);
        assert!(!ContextSet::of(&[("server", "x")]).is_satisfied_by(&other, SatisfyMode::AtLeastOneValuePerKey));
    }

    #[test]
    fn specificity_order() {
        let empty = ContextSet::new();
        let world = ContextSet::of(&[("world", "w")]);
        let server = ContextSet::of(&[("server", "s")]);
        let both = ContextSet::of(&[("server", "s"), ("world", "w")]);
        assert_eq!(empty.compare_specificity(&world), Ordering::Less);
        assert_eq!(world.compare_specificity(&server), Ordering::Less);
        assert_eq!(server.compare_specificity(&both), Ordering::Less);
    }

    #[test]
    fn command_contexts() {
        let s = parse_args(&["survival", "world_nether", "gamemode=creative"]);
        assert!(s.contains("server", "survival"));
        assert!(s.contains("world", "world_nether"));
        assert!(s.contains("gamemode", "creative"));
        assert_eq!(parse_args(&["server=global"]).len(), 0);
    }
}
