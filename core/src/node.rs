//! Nodes: one row of `{prefix}user_permissions` / `{prefix}group_permissions`. The key encodes
//! the kind (`group.admin`, `prefix.100.&c[Admin] `, `meta.color.red`, `weight.50`,
//! `displayname.Admins`, `R=regex`, or a plain permission), exactly as LuckPerms writes it.

use crate::context::ContextSet;
use std::cmp::Ordering;

pub const MAX_PERMISSION_LENGTH: usize = 200;
pub const MAX_GROUP_NAME_LENGTH: usize = 36;
pub const MAX_TRACK_NAME_LENGTH: usize = 36;
pub const MAX_USERNAME_LENGTH: usize = 16;

/// What a node key stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Permission,
    Regex(String),
    Inheritance(String),
    Prefix(i32, String),
    Suffix(i32, String),
    Meta(String, String),
    Weight(i32),
    DisplayName(String),
}

impl Kind {
    /// `NodeBuilders.determineMostApplicable`: the first parser that takes the key.
    pub fn parse(key: &str) -> Kind {
        let lower = key.to_lowercase();
        if let Some(g) = lower.strip_prefix("group.") {
            if valid_group_name(g) {
                return Kind::Inheritance(g.to_owned());
            }
        }
        if lower.starts_with("prefix.") {
            if let Some((p, v)) = split_chat_meta(&key["prefix.".len()..]) {
                return Kind::Prefix(p, v);
            }
        }
        if lower.starts_with("suffix.") {
            if let Some((p, v)) = split_chat_meta(&key["suffix.".len()..]) {
                return Kind::Suffix(p, v);
            }
        }
        if lower.starts_with("meta.") {
            if let Some((k, v)) = split_escaped(&key["meta.".len()..]) {
                return Kind::Meta(unescape(&k), unescape(&v));
            }
        }
        if let Some(w) = lower.strip_prefix("weight.") {
            if let Ok(w) = w.parse::<i32>() {
                return Kind::Weight(w);
            }
        }
        if lower.starts_with("displayname.") {
            return Kind::DisplayName(key["displayname.".len()..].to_owned());
        }
        if let Some(p) = key.strip_prefix("r=").or_else(|| key.strip_prefix("R=")) {
            return Kind::Regex(p.to_owned());
        }
        Kind::Permission
    }

    /// `NodeType` order (for `NodeComparator`): permission < regex < inheritance < prefix <
    /// suffix < meta < weight < display name.
    fn rank(&self) -> u8 {
        match self {
            Kind::Permission => 0,
            Kind::Regex(_) => 1,
            Kind::Inheritance(_) => 2,
            Kind::Prefix(..) => 3,
            Kind::Suffix(..) => 4,
            Kind::Meta(..) => 5,
            Kind::Weight(_) => 6,
            Kind::DisplayName(_) => 7,
        }
    }

    pub fn is_meta_or_chat_meta(&self) -> bool {
        matches!(self, Kind::Prefix(..) | Kind::Suffix(..) | Kind::Meta(..))
    }
}

fn split_chat_meta(rest: &str) -> Option<(i32, String)> {
    let (p, v) = split_escaped(rest)?;
    Some((p.parse().ok()?, unescape(&v)))
}

/// Splits at the first `.` not escaped by `\` (`Delimiters.SPLIT_BY_NODE_SEPARATOR_IN_TWO`).
fn split_escaped(s: &str) -> Option<(String, String)> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'.' && (i == 0 || b[i - 1] != b'\\') {
            return Some((s[..i].to_owned(), s[i + 1..].to_owned()));
        }
    }
    None
}

/// `Delimiters.escapeCharacters`: a `.` becomes `\.`.
pub fn escape(s: &str) -> String {
    s.replace('.', "\\.")
}

/// `Delimiters.unescapeCharacters`: `\.`, and the legacy `\/`, `\-`, `\$`, lose their `\`.
pub fn unescape(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i] == '\\' && matches!(chars[i + 1], '.' | '/' | '-' | '$') {
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

pub fn valid_group_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_GROUP_NAME_LENGTH && !s.contains(' ')
}

pub fn valid_track_name(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_TRACK_NAME_LENGTH && !s.contains(' ')
}

pub fn valid_permission(s: &str) -> bool {
    !s.is_empty() && s.chars().count() <= MAX_PERMISSION_LENGTH
}

pub fn valid_username(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_USERNAME_LENGTH && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Key builders, as LuckPerms writes them.
pub mod key {
    use super::escape;

    pub fn group(name: &str) -> String {
        format!("group.{}", name.to_lowercase())
    }
    pub fn prefix(priority: i32, value: &str) -> String {
        format!("prefix.{priority}.{}", escape(value))
    }
    pub fn suffix(priority: i32, value: &str) -> String {
        format!("suffix.{priority}.{}", escape(value))
    }
    pub fn meta(k: &str, v: &str) -> String {
        format!("meta.{}.{}", escape(k).to_lowercase(), escape(v))
    }
    pub fn weight(w: i32) -> String {
        format!("weight.{w}")
    }
    pub fn display_name(n: &str) -> String {
        format!("displayname.{n}")
    }
}

/// One node.
#[derive(Clone, Debug)]
pub struct Node {
    pub key: String,
    pub value: bool,
    /// Unix seconds; 0 when permanent.
    pub expiry: i64,
    pub contexts: ContextSet,
    /// The database row it was read from, when known (deletes go by id, as LuckPerms does).
    pub row_id: Option<i64>,
    pub kind: Kind,
}

impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.key == o.key && self.value == o.value && self.expiry == o.expiry && self.contexts == o.contexts
    }
}
impl Eq for Node {}

/// How two nodes are compared when setting and unsetting (`NodeEqualityPredicate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Equality {
    Exact,
    IgnoreValue,
    IgnoreExpiryTime,
    IgnoreExpiryTimeAndValue,
    IgnoreValueOrIfTemporary,
}

impl Node {
    pub fn new(key: impl Into<String>, value: bool, expiry: i64, contexts: ContextSet) -> Self {
        let key = key.into();
        let key = normalise_key(&key);
        let kind = Kind::parse(&key);
        Node { key, value, expiry, contexts, row_id: None, kind }
    }

    /// A node as a database row holds it. The key is rebuilt the way LuckPerms' node builders
    /// do (`group.Admin` -> `group.admin`); the row id is kept so deletes still go by id.
    pub fn from_row(key: String, value: bool, expiry: i64, contexts: ContextSet, row_id: i64) -> Self {
        let key = normalise_key(&key);
        let kind = Kind::parse(&key);
        Node { key, value, expiry, contexts, row_id: Some(row_id), kind }
    }

    pub fn permanent(key: impl Into<String>, value: bool, contexts: ContextSet) -> Self {
        Self::new(key, value, 0, contexts)
    }

    pub fn has_expiry(&self) -> bool {
        self.expiry > 0
    }

    pub fn has_expired(&self, now: i64) -> bool {
        self.has_expiry() && self.expiry <= now
    }

    pub fn is_wildcard(&self) -> bool {
        matches!(self.kind, Kind::Permission) && (self.key == "*" || self.key == "'*'" || (self.key.ends_with(".*") && self.key.len() > 2))
    }

    fn wildcard_level(&self) -> usize {
        self.key.matches('.').count()
    }

    pub fn equals(&self, o: &Node, eq: Equality) -> bool {
        if self.key != o.key {
            return false;
        }
        if self.contexts != o.contexts {
            return false;
        }
        match eq {
            Equality::Exact => self.value == o.value && self.expiry == o.expiry,
            Equality::IgnoreValue => self.expiry == o.expiry,
            Equality::IgnoreExpiryTime => self.value == o.value && self.has_expiry() == o.has_expiry(),
            Equality::IgnoreExpiryTimeAndValue => self.has_expiry() == o.has_expiry(),
            Equality::IgnoreValueOrIfTemporary => self.has_expiry() == o.has_expiry(),
        }
    }

    /// `NodeComparator` ascending; LuckPerms keeps nodes of one context set in descending
    /// order, so the "greatest" node is looked at first.
    pub fn compare(&self, o: &Node) -> Ordering {
        if self == o {
            return Ordering::Equal;
        }
        // Special kinds before plain permissions (reversed type order).
        o.kind
            .rank()
            .cmp(&self.kind.rank())
            .then(self.has_expiry().cmp(&o.has_expiry()))
            .then(o.is_wildcard().cmp(&self.is_wildcard()))
            .then_with(|| if self.has_expiry() { o.expiry.cmp(&self.expiry) } else { Ordering::Equal })
            .then_with(|| {
                if self.is_wildcard() {
                    self.wildcard_level().cmp(&o.wildcard_level())
                } else {
                    Ordering::Equal
                }
            })
            .then_with(|| o.key.cmp(&self.key))
            .then(self.value.cmp(&o.value))
    }
}

/// Keys of the special kinds are stored lower case where LuckPerms lowers them (group names,
/// meta keys, the markers); a plain permission keeps its case.
fn normalise_key(key: &str) -> String {
    match Kind::parse(key) {
        Kind::Inheritance(g) => key::group(&g),
        Kind::Weight(w) => key::weight(w),
        Kind::Prefix(p, v) => key::prefix(p, &v),
        Kind::Suffix(p, v) => key::suffix(p, &v),
        Kind::Meta(k, v) => key::meta(&k, &v),
        Kind::DisplayName(n) => key::display_name(&n),
        _ => key.to_owned(),
    }
}

/// `ShorthandParser.expandShorthand`: `essentials.(fly|god)`, `kit.{1-3}`, `x.(a-c)`; the
/// expansions only (not the input itself). At most 1000.
pub fn expand_shorthand(input: &str) -> Vec<String> {
    let mut results: Vec<String> = vec![input.to_owned()];
    loop {
        let mut work = false;
        let mut next = Vec::new();
        for s in &results {
            if let Some(expanded) = match_group(s) {
                work = true;
                next.extend(expanded);
            } else {
                next.push(s.clone());
            }
            if next.len() > 1000 {
                return Vec::new();
            }
        }
        next.sort();
        next.dedup();
        results = next;
        if !work {
            break;
        }
    }
    results.retain(|s| s != input);
    results
}

fn index_of_either(s: &str, a: char, b: char) -> Option<usize> {
    s.find(a).or_else(|| s.find(b))
}

fn match_group(input: &str) -> Option<Vec<String>> {
    let open = index_of_either(input, '{', '(')?;
    let close = index_of_either(input, '}', ')')?;
    if close < open {
        return None;
    }
    let before = &input[..open];
    let after = &input[close + 1..];
    let between = &input[open + 1..close];
    let parts: Vec<String> = if let Some(r) = numeric_range(between) {
        r
    } else if let Some(r) = char_range(between) {
        r
    } else if between.contains(',') || between.contains('|') {
        between.split([',', '|']).filter(|p| !p.is_empty()).map(str::to_owned).collect()
    } else {
        vec![between.to_owned()]
    };
    Some(parts.into_iter().map(|p| format!("{before}{p}{after}")).collect())
}

fn numeric_range(s: &str) -> Option<Vec<String>> {
    let i = s.find('-')?;
    if i == 0 || i == s.len() - 1 {
        return None;
    }
    let a: i64 = s[..i].parse().ok()?;
    let b: i64 = s[i + 1..].parse().ok()?;
    let (lo, hi) = (a.min(b), a.max(b));
    if hi - lo > 250 {
        return None;
    }
    Some((lo..=hi).map(|n| n.to_string()).collect())
}

fn char_range(s: &str) -> Option<Vec<String>> {
    let c: Vec<char> = s.chars().collect();
    if c.len() != 3 || c[1] != '-' {
        return None;
    }
    let (lo, hi) = ((c[0] as u32).min(c[2] as u32), (c[0] as u32).max(c[2] as u32));
    if hi - lo > 250 {
        return None;
    }
    Some((lo..=hi).filter_map(char::from_u32).map(|ch| ch.to_string()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_keys_are_rebuilt_and_keep_row_id() {
        let n = Node::from_row("group.Admin".into(), true, 0, ContextSet::new(), 7);
        assert_eq!(n.key, "group.admin");
        assert_eq!(n.row_id, Some(7));
        let p = Node::from_row("Essentials.Fly".into(), true, 0, ContextSet::new(), 8);
        assert_eq!(p.key, "Essentials.Fly");
        let m = Node::from_row("meta.Color.x".into(), true, 0, ContextSet::new(), 9);
        assert_eq!(m.key, "meta.color.x");
    }

    #[test]
    fn key_equality_is_exact() {
        let a = Node::permanent("Essentials.Fly", true, ContextSet::new());
        let b = Node::permanent("essentials.fly", true, ContextSet::new());
        assert!(!a.equals(&b, Equality::Exact));
        let g1 = Node::permanent("group.Admin", true, ContextSet::new());
        let g2 = Node::permanent("group.admin", true, ContextSet::new());
        assert!(g1.equals(&g2, Equality::Exact));
    }

    #[test]
    fn kinds() {
        assert_eq!(Kind::parse("group.Admin"), Kind::Inheritance("admin".into()));
        assert_eq!(Kind::parse("prefix.100.&c[Admin] "), Kind::Prefix(100, "&c[Admin] ".into()));
        assert_eq!(Kind::parse("suffix.5.x\\.y"), Kind::Suffix(5, "x.y".into()));
        assert_eq!(Kind::parse("meta.color.red"), Kind::Meta("color".into(), "red".into()));
        assert_eq!(Kind::parse("meta.a\\.b.c.d"), Kind::Meta("a.b".into(), "c.d".into()));
        assert_eq!(Kind::parse("weight.50"), Kind::Weight(50));
        assert_eq!(Kind::parse("weight.x"), Kind::Permission);
        assert_eq!(Kind::parse("displayname.Admins"), Kind::DisplayName("Admins".into()));
        assert_eq!(Kind::parse("R=essentials\\..*"), Kind::Regex("essentials\\..*".into()));
        assert_eq!(Kind::parse("essentials.fly"), Kind::Permission);
        assert_eq!(Kind::parse("prefix.notanumber.x"), Kind::Permission);
    }

    #[test]
    fn keys_escape() {
        assert_eq!(key::prefix(10, "&7[Mr. X]"), "prefix.10.&7[Mr\\. X]");
        assert_eq!(key::meta("Some.Key", "v.1"), "meta.some\\.key.v\\.1");
        assert_eq!(unescape("a\\.b\\-c\\$d\\/e\\x"), "a.b-c$d/e\\x");
        let n = Node::permanent("GROUP.Mod", true, ContextSet::new());
        assert_eq!(n.key, "group.mod");
        let p = Node::permanent("Essentials.Fly", true, ContextSet::new());
        assert_eq!(p.key, "Essentials.Fly");
    }

    #[test]
    fn wildcards() {
        assert!(Node::permanent("*", true, ContextSet::new()).is_wildcard());
        assert!(Node::permanent("a.*", true, ContextSet::new()).is_wildcard());
        assert!(!Node::permanent(".*", true, ContextSet::new()).is_wildcard());
        assert!(!Node::permanent("a.b", true, ContextSet::new()).is_wildcard());
    }

    #[test]
    fn ordering_temporary_first() {
        let perm = Node::permanent("a.b", true, ContextSet::new());
        let temp = Node::new("a.b", true, 100, ContextSet::new());
        let mut v = [perm.clone(), temp.clone()];
        v.sort_by(|a, b| b.compare(a));
        assert_eq!(v[0], temp);
    }

    #[test]
    fn shorthand() {
        let mut r = expand_shorthand("essentials.(fly|god)");
        r.sort();
        assert_eq!(r, vec!["essentials.fly", "essentials.god"]);
        assert_eq!(expand_shorthand("kit.{1-3}").len(), 3);
        assert_eq!(expand_shorthand("x.(a-c).y"), vec!["x.a.y", "x.b.y", "x.c.y"]);
        assert!(expand_shorthand("plain.node").is_empty());
    }
}
