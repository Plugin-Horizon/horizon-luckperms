//! Users, groups and tracks, with LuckPerms' rules for setting and unsetting nodes.

use crate::context::ContextSet;
use crate::node::{Equality, Kind, Node};
use std::collections::{BTreeMap, HashMap};

pub const DEFAULT_GROUP: &str = "default";

/// What a set/unset did (`DataMutateResult`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mutate {
    Success,
    AlreadyHas,
    Lacks,
}

/// `temporary-add-behaviour` and the per-command modifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TempMerge {
    /// Adds the new duration to an existing temporary node.
    Accumulate,
    /// Keeps whichever expires later.
    Replace,
    /// Refuses when one exists.
    #[default]
    Deny,
}

impl TempMerge {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "accumulate" => Some(Self::Accumulate),
            "replace" => Some(Self::Replace),
            "deny" => Some(Self::Deny),
            _ => None,
        }
    }
}

/// The nodes of a user or group.
#[derive(Clone, Debug, Default)]
pub struct NodeSet {
    pub nodes: Vec<Node>,
}

impl NodeSet {
    pub fn has(&self, node: &Node, eq: Equality) -> Option<bool> {
        self.nodes.iter().find(|n| n.equals(node, eq)).map(|n| n.value)
    }

    /// `PermissionHolder.setNode`: fails when an equal node (ignoring the expiry time) is there.
    pub fn set(&mut self, node: Node) -> Mutate {
        if self.has(&node, Equality::IgnoreExpiryTime).is_some() {
            return Mutate::AlreadyHas;
        }
        // A node of the same key and contexts but the other value is replaced.
        self.nodes.retain(|n| !n.equals(&node, Equality::IgnoreExpiryTimeAndValue));
        self.nodes.push(node);
        Mutate::Success
    }

    /// `setNode` with a merge strategy for temporary nodes; the node it ended as.
    pub fn set_temp(&mut self, node: Node, merge: TempMerge) -> (Mutate, Node) {
        if merge != TempMerge::Deny {
            if let Some(i) = self
                .nodes
                .iter()
                .position(|n| n.has_expiry() && n.equals(&node, Equality::IgnoreExpiryTime))
            {
                let old = self.nodes[i].clone();
                let mut merged = node.clone();
                match merge {
                    TempMerge::Accumulate => {
                        let now = crate::duration::now();
                        merged.expiry = old.expiry + (node.expiry - now).max(0);
                    }
                    TempMerge::Replace => {
                        if old.expiry >= node.expiry {
                            return (Mutate::AlreadyHas, old);
                        }
                    }
                    TempMerge::Deny => unreachable!(),
                }
                self.nodes.remove(i);
                self.nodes.push(merged.clone());
                return (Mutate::Success, merged);
            }
        }
        let r = self.set(node.clone());
        (r, node)
    }

    /// `PermissionHolder.unsetNode`: removes the nodes of the same key, contexts and
    /// temporariness, whatever their value and expiry.
    pub fn unset(&mut self, node: &Node) -> Mutate {
        let before = self.nodes.len();
        self.nodes.retain(|n| !n.equals(node, Equality::IgnoreExpiryTimeAndValue));
        if self.nodes.len() == before {
            Mutate::Lacks
        } else {
            Mutate::Success
        }
    }

    /// Removes every node `f` takes; true when any went.
    pub fn remove_if(&mut self, f: impl Fn(&Node) -> bool) -> bool {
        let before = self.nodes.len();
        self.nodes.retain(|n| !f(n));
        self.nodes.len() != before
    }

    /// Drops expired temporary nodes (`auditTemporaryNodes`).
    pub fn audit_expired(&mut self, now: i64) -> bool {
        self.remove_if(|n| n.has_expired(now))
    }

    /// Nodes grouped by context set (most specific first) and, within one, in descending
    /// `NodeComparator` order: the order LuckPerms walks a holder's own nodes in.
    pub fn sorted(&self) -> Vec<&Node> {
        let mut v: Vec<&Node> = self.nodes.iter().collect();
        v.sort_by(|a, b| b.contexts.compare_specificity(&a.contexts).then_with(|| b.compare(a)));
        v
    }

    pub fn inheritance(&self) -> impl Iterator<Item = (&Node, &str)> {
        self.nodes.iter().filter_map(|n| match &n.kind {
            Kind::Inheritance(g) => Some((n, g.as_str())),
            _ => None,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Group {
    pub name: String,
    pub data: NodeSet,
}

impl Group {
    pub fn new(name: &str) -> Self {
        Group { name: name.to_lowercase(), data: NodeSet::default() }
    }

    /// The highest own weight node (`Group.getWeight`); None without one.
    pub fn weight(&self) -> Option<i32> {
        self.data
            .nodes
            .iter()
            .filter_map(|n| if let Kind::Weight(w) = n.kind { Some(w) } else { None })
            .max()
    }

    /// The own display name node, if any, preferring global ones.
    pub fn display_name(&self) -> Option<String> {
        self.data
            .sorted()
            .into_iter()
            .rev()
            .find_map(|n| if let Kind::DisplayName(d) = &n.kind { Some(d.clone()) } else { None })
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub uuid: u128,
    pub username: Option<String>,
    /// `primary_group` of the players table.
    pub primary_group: String,
    pub data: NodeSet,
    /// The nodes as last read from or written to storage, to save only the difference.
    pub saved: Vec<Node>,
}

impl User {
    pub fn new(uuid: u128) -> Self {
        User { uuid, username: None, primary_group: DEFAULT_GROUP.into(), data: NodeSet::default(), saved: Vec::new() }
    }

    /// `UserManager.giveDefaultIfNeeded`: a user without any group node in the global context
    /// inherits `default`. True when it added it.
    pub fn give_default_if_needed(&mut self) -> bool {
        let has_global_group = self.data.inheritance().any(|(n, _)| n.contexts.is_empty());
        if has_global_group {
            return false;
        }
        self.data.nodes.push(Node::permanent(crate::node::key::group(DEFAULT_GROUP), true, ContextSet::new()));
        self.primary_group = DEFAULT_GROUP.into();
        true
    }

    /// With `primary-group-calculation: stored`: a stored primary group the user is no longer
    /// in (globally) moves to the first global group the user is in, as LuckPerms does on load.
    /// True when it changed.
    pub fn repair_stored_primary_group(&mut self) -> bool {
        let global: Vec<&str> =
            self.data.inheritance().filter(|(n, _)| n.value && n.contexts.is_empty()).map(|(_, g)| g).collect();
        if global.is_empty() || global.iter().any(|g| g.eq_ignore_ascii_case(&self.primary_group)) {
            return false;
        }
        self.primary_group = global[0].to_lowercase();
        true
    }

    /// `UserManager.isNonDefaultUser`: anything but the single global `group.default` node.
    pub fn is_non_default(&self) -> bool {
        match self.data.nodes.as_slice() {
            [] => true,
            [only] => {
                let default_node =
                    only.key == "group.default" && only.value && !only.has_expiry() && only.contexts.is_empty();
                !default_node || !self.primary_group.eq_ignore_ascii_case(DEFAULT_GROUP)
            }
            _ => true,
        }
    }

    pub fn display(&self) -> String {
        self.username.clone().unwrap_or_else(|| crate::uuid::format(self.uuid))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub name: String,
    pub groups: Vec<String>,
}

impl Track {
    /// The `groups` column: a JSON array of names (`["default","mod","admin"]`).
    pub fn groups_json(&self) -> String {
        serde_json::to_string(&self.groups).unwrap_or_else(|_| "[]".into())
    }

    pub fn from_json(name: &str, json: &str) -> Self {
        let groups: Vec<String> = serde_json::from_str(json).unwrap_or_default();
        Track { name: name.to_lowercase(), groups: groups.into_iter().map(|g| g.to_lowercase()).collect() }
    }

    pub fn next(&self, group: &str) -> Option<&str> {
        let i = self.groups.iter().position(|g| g == group)?;
        self.groups.get(i + 1).map(String::as_str)
    }

    pub fn previous(&self, group: &str) -> Option<&str> {
        let i = self.groups.iter().position(|g| g == group)?;
        i.checked_sub(1).and_then(|j| self.groups.get(j)).map(String::as_str)
    }
}

/// Everything this server knows: all groups and tracks (LuckPerms keeps them all loaded) and
/// the users that are on or were looked up.
#[derive(Clone, Debug, Default)]
pub struct Model {
    pub groups: BTreeMap<String, Group>,
    pub tracks: BTreeMap<String, Track>,
    pub users: HashMap<u128, User>,
    /// Bumped on every change; caches compare against it.
    pub version: u64,
}

impl Model {
    pub fn touch(&mut self) {
        self.version = self.version.wrapping_add(1);
    }

    pub fn ensure_default_group(&mut self) -> bool {
        if self.groups.contains_key(DEFAULT_GROUP) {
            return false;
        }
        self.groups.insert(DEFAULT_GROUP.into(), Group::new(DEFAULT_GROUP));
        true
    }

    /// A group by name or by display name (`GroupManager.getByDisplayName`).
    pub fn group_by_name_or_display(&self, name: &str) -> Option<&Group> {
        let lower = name.to_lowercase();
        self.groups
            .get(&lower)
            .or_else(|| self.groups.values().find(|g| g.display_name().is_some_and(|d| d.eq_ignore_ascii_case(name))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(key: &str, v: bool) -> Node {
        Node::permanent(key, v, ContextSet::new())
    }

    #[test]
    fn set_unset() {
        let mut s = NodeSet::default();
        assert_eq!(s.set(n("a.b", true)), Mutate::Success);
        assert_eq!(s.set(n("a.b", true)), Mutate::AlreadyHas);
        assert_eq!(s.set(n("a.b", false)), Mutate::Success);
        assert_eq!(s.nodes.len(), 1);
        assert!(!s.nodes[0].value);
        assert_eq!(s.unset(&n("a.b", true)), Mutate::Success);
        assert_eq!(s.unset(&n("a.b", true)), Mutate::Lacks);
    }

    #[test]
    fn temp_merge() {
        let now = crate::duration::now();
        let mut s = NodeSet::default();
        let t = Node::new("x", true, now + 100, ContextSet::new());
        assert_eq!(s.set_temp(t.clone(), TempMerge::Deny).0, Mutate::Success);
        assert_eq!(s.set_temp(t.clone(), TempMerge::Deny).0, Mutate::AlreadyHas);
        let (r, merged) = s.set_temp(Node::new("x", true, now + 50, ContextSet::new()), TempMerge::Accumulate);
        assert_eq!(r, Mutate::Success);
        assert!(merged.expiry >= now + 149);
        let (r, _) = s.set_temp(Node::new("x", true, now + 10, ContextSet::new()), TempMerge::Replace);
        assert_eq!(r, Mutate::AlreadyHas);
        // A permanent node is not touched by unsetting the temporary one.
        s.set(n("x", true));
        let mut temp = n("x", true);
        temp.expiry = 1;
        assert_eq!(s.unset(&temp), Mutate::Success);
        assert_eq!(s.nodes.len(), 1);
        assert!(!s.nodes[0].has_expiry());
    }

    #[test]
    fn default_user() {
        let mut u = User::new(1);
        assert!(u.give_default_if_needed());
        assert!(!u.is_non_default());
        u.data.set(n("some.perm", true));
        assert!(u.is_non_default());
        assert!(!u.give_default_if_needed());
    }

    #[test]
    fn non_default_primary_group_counts() {
        let mut u = User::new(1);
        assert!(u.is_non_default(), "no nodes at all is non-default, as in LuckPerms");
        u.give_default_if_needed();
        assert!(!u.is_non_default());
        u.primary_group = "vip".into();
        assert!(u.is_non_default(), "a stored primary group other than default must be kept");
    }

    #[test]
    fn give_default_resets_primary_group() {
        let mut u = User::new(1);
        u.primary_group = "admin".into();
        assert!(u.give_default_if_needed());
        assert_eq!(u.primary_group, "default");
    }

    #[test]
    fn stored_primary_group_repair() {
        let mut u = User::new(1);
        u.primary_group = "admin".into();
        u.data.set(n("group.mod", true));
        assert!(u.repair_stored_primary_group());
        assert_eq!(u.primary_group, "mod");
        assert!(!u.repair_stored_primary_group());
    }

    #[test]
    fn key_case_is_significant() {
        let mut s = NodeSet::default();
        assert_eq!(s.set(n("Essentials.Fly", true)), Mutate::Success);
        assert_eq!(s.set(n("essentials.fly", true)), Mutate::Success);
        assert_eq!(s.nodes.len(), 2);
        assert_eq!(s.unset(&n("essentials.fly", true)), Mutate::Success);
        assert_eq!(s.nodes.len(), 1);
        assert_eq!(s.nodes[0].key, "Essentials.Fly");
    }

    #[test]
    fn weight_false_still_counts() {
        let mut g = Group::new("staff");
        g.data.set(n("weight.50", false));
        assert_eq!(g.weight(), Some(50));
    }

    #[test]
    fn tracks() {
        let t = Track::from_json("Staff", r#"["default","Mod","admin"]"#);
        assert_eq!(t.name, "staff");
        assert_eq!(t.next("mod"), Some("admin"));
        assert_eq!(t.previous("mod"), Some("default"));
        assert_eq!(t.next("admin"), None);
        assert_eq!(t.groups_json(), r#"["default","mod","admin"]"#);
    }
}
