//! Storage: LuckPerms' SQL schema (`sql.rs`, MariaDB/MySQL) and an in-memory stand-in with
//! the same behaviour for tests.

use crate::actionlog::{Action, TargetType};
use crate::model::{Group, Track, User, DEFAULT_GROUP};
use crate::node::{Equality, Node};
use std::collections::{BTreeMap, HashMap};

#[cfg(feature = "mysql")]
pub mod sql;

pub type Result<T> = std::result::Result<T, String>;

/// A user's row of the players table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerRow {
    pub username: String,
    pub primary_group: String,
}

/// What saving a player's name found (`PlayerSaveResult`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerSave {
    pub new_player: bool,
    pub username_changed: Option<String>,
    /// Other uuids that had this name (they lost it).
    pub other_uuids: Vec<u128>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HolderKey {
    User(u128),
    Group(String),
}

/// How `/lp search` matches node keys (LuckPerms: `==`, `!=`, `~~`, `!~`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyMatch {
    Exact(String),
    NotEqual(String),
    /// SQL `LIKE`: `%` any run, `_` one character, case-insensitive.
    Like(String),
    NotLike(String),
}

impl KeyMatch {
    pub fn matches(&self, key: &str) -> bool {
        match self {
            KeyMatch::Exact(k) => key.eq_ignore_ascii_case(k),
            KeyMatch::NotEqual(k) => !key.eq_ignore_ascii_case(k),
            KeyMatch::Like(p) => like(p, key),
            KeyMatch::NotLike(p) => !like(p, key),
        }
    }
}

/// `LIKE` as in SQL, without escapes, case-insensitive.
fn like(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.first() {
            None => t.is_empty(),
            Some('%') => (0..=t.len()).any(|i| go(&p[1..], &t[i..])),
            Some('_') => !t.is_empty() && go(&p[1..], &t[1..]),
            Some(c) => t.first() == Some(c) && go(&p[1..], &t[1..]),
        }
    }
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let t: Vec<char> = text.to_lowercase().chars().collect();
    go(&p, &t)
}

/// The added and removed nodes between two states (`Difference`).
pub fn diff(before: &[Node], after: &[Node]) -> (Vec<Node>, Vec<Node>) {
    let added = after.iter().filter(|n| !before.iter().any(|b| b.equals(n, Equality::Exact))).cloned().collect();
    let removed = before.iter().filter(|b| !after.iter().any(|n| n.equals(b, Equality::Exact))).cloned().collect();
    (added, removed)
}

/// Which actions `/lp log` lists (`ActionFilters`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionFilter {
    All,
    /// Actions done by this uuid.
    Source(u128),
    /// Actions affecting this user.
    User(u128),
    Group(String),
    Track(String),
    /// Actions whose source, target or description contains the text (case-insensitive).
    Search(String),
}

pub trait Storage: Send {
    fn name(&self) -> &'static str;
    /// Creates missing tables and the `default` group.
    fn init(&mut self) -> Result<()>;
    /// A user's nodes and players row.
    fn load_user(&mut self, uuid: u128) -> Result<(Vec<Node>, Option<PlayerRow>)>;
    /// Writes what changed since `user.saved`; LuckPerms' rule for users with nothing but the
    /// default group is kept: their rows go and their primary group becomes `default`.
    fn save_user(&mut self, user: &User) -> Result<()>;
    fn load_all_groups(&mut self) -> Result<Vec<Group>>;
    fn load_group(&mut self, name: &str) -> Result<Option<Group>>;
    fn create_group(&mut self, name: &str) -> Result<()>;
    /// Writes what changed since `before`.
    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()>;
    fn delete_group(&mut self, name: &str) -> Result<()>;
    fn load_all_tracks(&mut self) -> Result<Vec<Track>>;
    fn save_track(&mut self, track: &Track) -> Result<()>;
    fn delete_track(&mut self, name: &str) -> Result<()>;
    fn save_player_data(&mut self, uuid: u128, username: &str) -> Result<PlayerSave>;
    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>>;
    fn name_of(&mut self, uuid: u128) -> Result<Option<String>>;
    fn log_action(&mut self, action: &Action) -> Result<()>;
    /// The newest actions first, at most `limit`, optionally for one target.
    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>>;
    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>>;
    /// Every action matching `filter`, newest first (`getLogPage` without the paging).
    fn log_entries(&mut self, filter: &ActionFilter) -> Result<Vec<Action>> {
        const ALL: usize = 10_000_000;
        match filter {
            ActionFilter::All => self.actions(ALL, None),
            ActionFilter::Source(u) => Ok(self.actions(ALL, None)?.into_iter().filter(|a| a.source_uuid == *u).collect()),
            ActionFilter::User(u) => self.actions(ALL, Some((TargetType::User, &crate::uuid::format(*u)))),
            ActionFilter::Group(g) => self.actions(ALL, Some((TargetType::Group, g))),
            ActionFilter::Track(t) => self.actions(ALL, Some((TargetType::Track, t))),
            ActionFilter::Search(q) => {
                let q = q.to_lowercase();
                Ok(self
                    .actions(ALL, None)?
                    .into_iter()
                    .filter(|a| a.source_name.to_lowercase().contains(&q) || a.target_name.to_lowercase().contains(&q) || a.description.to_lowercase().contains(&q))
                    .collect())
            }
        }
    }
    /// The `{prefix}messenger` table: its newest id at start.
    fn messenger_init(&mut self) -> Result<i64>;
    fn messenger_send(&mut self, message: &str) -> Result<()>;
    /// Rows after `last_id` from the last 30 seconds.
    fn messenger_poll(&mut self, last_id: i64) -> Result<Vec<(i64, String)>>;
    /// Drops rows older than 60 seconds.
    fn messenger_housekeep(&mut self) -> Result<()>;
}

/// Everything in RAM, shared between "servers" of a test through `Clone` + a mutex.
#[derive(Clone, Default)]
pub struct MemoryStorage {
    pub inner: std::sync::Arc<std::sync::Mutex<MemoryData>>,
}

#[derive(Default, Debug)]
pub struct MemoryData {
    pub user_nodes: HashMap<u128, Vec<Node>>,
    pub players: HashMap<u128, PlayerRow>,
    pub groups: BTreeMap<String, Vec<Node>>,
    pub tracks: BTreeMap<String, Vec<String>>,
    pub actions: Vec<Action>,
    pub messages: Vec<(i64, std::time::Instant, String)>,
    pub next_message: i64,
}

impl MemoryStorage {
    fn with<T>(&self, f: impl FnOnce(&mut MemoryData) -> T) -> T {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }
}

fn apply_diff(rows: &mut Vec<Node>, before: &[Node], after: &[Node]) {
    let (added, removed) = diff(before, after);
    rows.retain(|r| !removed.iter().any(|n| n.equals(r, Equality::Exact)));
    for n in added {
        let mut n = n;
        n.row_id = None;
        rows.push(n);
    }
}

impl Storage for MemoryStorage {
    fn name(&self) -> &'static str {
        "memory"
    }
    fn init(&mut self) -> Result<()> {
        self.with(|d| {
            d.groups.entry(DEFAULT_GROUP.into()).or_default();
        });
        Ok(())
    }
    fn load_user(&mut self, uuid: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        Ok(self.with(|d| (d.user_nodes.get(&uuid).cloned().unwrap_or_default(), d.players.get(&uuid).cloned())))
    }
    fn save_user(&mut self, user: &User) -> Result<()> {
        self.with(|d| {
            if !user.is_non_default() {
                d.user_nodes.remove(&user.uuid);
                if let Some(p) = d.players.get_mut(&user.uuid) {
                    p.primary_group = DEFAULT_GROUP.into();
                }
                return;
            }
            let rows = d.user_nodes.entry(user.uuid).or_default();
            apply_diff(rows, &user.saved, &user.data.nodes);
            let name = user.username.clone().unwrap_or_else(|| "null".into()).to_lowercase();
            d.players
                .entry(user.uuid)
                .and_modify(|p| p.primary_group = user.primary_group.clone())
                .or_insert(PlayerRow { username: name, primary_group: user.primary_group.clone() });
        });
        Ok(())
    }
    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        Ok(self.with(|d| {
            d.groups
                .iter()
                .map(|(n, nodes)| {
                    let mut g = Group::new(n);
                    g.data.nodes = nodes.clone();
                    g
                })
                .collect()
        }))
    }
    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        Ok(self.with(|d| {
            d.groups.get(name).map(|nodes| {
                let mut g = Group::new(name);
                g.data.nodes = nodes.clone();
                g
            })
        }))
    }
    fn create_group(&mut self, name: &str) -> Result<()> {
        self.with(|d| {
            d.groups.entry(name.to_lowercase()).or_default();
        });
        Ok(())
    }
    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        self.with(|d| apply_diff(d.groups.entry(group.name.clone()).or_default(), before, &group.data.nodes));
        Ok(())
    }
    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.with(|d| d.groups.remove(name));
        Ok(())
    }
    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        Ok(self.with(|d| d.tracks.iter().map(|(n, g)| Track { name: n.clone(), groups: g.clone() }).collect()))
    }
    fn save_track(&mut self, track: &Track) -> Result<()> {
        self.with(|d| d.tracks.insert(track.name.clone(), track.groups.clone()));
        Ok(())
    }
    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.with(|d| d.tracks.remove(name));
        Ok(())
    }
    fn save_player_data(&mut self, uuid: u128, username: &str) -> Result<PlayerSave> {
        let name = username.to_lowercase();
        Ok(self.with(|d| {
            let mut r = PlayerSave::default();
            match d.players.get_mut(&uuid) {
                None => {
                    d.players.insert(uuid, PlayerRow { username: name.clone(), primary_group: DEFAULT_GROUP.into() });
                    r.new_player = true;
                }
                Some(p) if p.username != name => {
                    r.username_changed = Some(std::mem::replace(&mut p.username, name.clone()));
                }
                _ => {}
            }
            let others: Vec<u128> = d.players.iter().filter(|(u, p)| **u != uuid && p.username == name).map(|(u, _)| *u).collect();
            for o in &others {
                d.players.remove(o);
            }
            r.other_uuids = others;
            r
        }))
    }
    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        let name = username.to_lowercase();
        Ok(self.with(|d| d.players.iter().find(|(_, p)| p.username == name).map(|(u, _)| *u)))
    }
    fn name_of(&mut self, uuid: u128) -> Result<Option<String>> {
        Ok(self.with(|d| d.players.get(&uuid).map(|p| p.username.clone()).filter(|n| n != "null")))
    }
    fn log_action(&mut self, action: &Action) -> Result<()> {
        self.with(|d| d.actions.push(action.clone()));
        Ok(())
    }
    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        Ok(self.with(|d| {
            d.actions
                .iter()
                .rev()
                .filter(|a| target.is_none_or(|(t, n)| a.target_type == t && (a.target_name.eq_ignore_ascii_case(n) || a.target_uuid.map(crate::uuid::format).as_deref() == Some(n))))
                .take(limit)
                .cloned()
                .collect()
        }))
    }
    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        Ok(self.with(|d| {
            let mut out = Vec::new();
            for (u, nodes) in &d.user_nodes {
                for n in nodes.iter().filter(|n| key.matches(&n.key)) {
                    out.push((HolderKey::User(*u), n.clone()));
                }
            }
            for (g, nodes) in &d.groups {
                for n in nodes.iter().filter(|n| key.matches(&n.key)) {
                    out.push((HolderKey::Group(g.clone()), n.clone()));
                }
            }
            out.sort_by(|a, b| a.0.cmp(&b.0));
            out
        }))
    }
    fn messenger_init(&mut self) -> Result<i64> {
        Ok(self.with(|d| d.next_message))
    }
    fn messenger_send(&mut self, message: &str) -> Result<()> {
        self.with(|d| {
            d.next_message += 1;
            let id = d.next_message;
            d.messages.push((id, std::time::Instant::now(), message.to_owned()));
        });
        Ok(())
    }
    fn messenger_poll(&mut self, last_id: i64) -> Result<Vec<(i64, String)>> {
        Ok(self.with(|d| {
            d.messages
                .iter()
                .filter(|(id, t, _)| *id > last_id && t.elapsed().as_secs() < 30)
                .map(|(id, _, m)| (*id, m.clone()))
                .collect()
        }))
    }
    fn messenger_housekeep(&mut self) -> Result<()> {
        self.with(|d| d.messages.retain(|(_, t, _)| t.elapsed().as_secs() < 60));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::ContextSet;

    #[test]
    fn diff_works() {
        let a = Node::permanent("a", true, ContextSet::new());
        let b = Node::permanent("b", true, ContextSet::new());
        let c = Node::permanent("c", true, ContextSet::new());
        let (added, removed) = diff(&[a.clone(), b.clone()], &[b, c.clone()]);
        assert_eq!(added, vec![c]);
        assert_eq!(removed, vec![a]);
    }

    #[test]
    fn default_only_user_is_deleted() {
        let mut s = MemoryStorage::default();
        let mut u = User::new(1);
        u.username = Some("Bob".into());
        u.data.set(Node::permanent("x.y", true, ContextSet::new()));
        s.save_user(&u).unwrap();
        assert_eq!(s.load_user(1).unwrap().0.len(), 1);
        assert_eq!(s.name_of(1).unwrap().as_deref(), Some("bob"));
        let mut u2 = u.clone();
        u2.saved = u.data.nodes.clone();
        u2.data.nodes.clear();
        u2.give_default_if_needed();
        s.save_user(&u2).unwrap();
        assert!(s.load_user(1).unwrap().0.is_empty());
    }

    #[test]
    fn player_names() {
        let mut s = MemoryStorage::default();
        assert!(s.save_player_data(1, "Alice").unwrap().new_player);
        assert_eq!(s.save_player_data(1, "Alicia").unwrap().username_changed.as_deref(), Some("alice"));
        let r = s.save_player_data(2, "alicia").unwrap();
        assert_eq!(r.other_uuids, vec![1]);
        assert_eq!(s.uuid_of("ALICIA").unwrap(), Some(2));
    }
}
