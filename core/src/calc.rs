//! Resolution: which nodes a user or group has in given contexts (inheritance graph, weights,
//! context filters), the permission check (direct, regex, wildcard, op) and the meta
//! (prefix/suffix stacks, meta values, weight, primary group).

use crate::config::{Config, StackFormat};
use crate::context::{ContextSet, GlobalFlags, SatisfyMode};
use crate::model::{Model, DEFAULT_GROUP};
use crate::node::{expand_shorthand, Kind, Node};
use std::collections::{HashMap, HashSet};

/// A permission's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tristate {
    True,
    False,
    Undefined,
}

impl Tristate {
    pub fn of(b: bool) -> Self {
        if b {
            Tristate::True
        } else {
            Tristate::False
        }
    }
    pub fn as_bool(self) -> Option<bool> {
        match self {
            Tristate::True => Some(true),
            Tristate::False => Some(false),
            Tristate::Undefined => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Tristate::True => "true",
            Tristate::False => "false",
            Tristate::Undefined => "undefined",
        }
    }
}

/// Whose data is resolved.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HolderRef {
    User(u128),
    Group(String),
}

impl HolderRef {
    fn name(&self, model: &Model) -> String {
        match self {
            HolderRef::User(u) => model.users.get(u).map(|u| u.display()).unwrap_or_else(|| crate::uuid::format(*u)),
            HolderRef::Group(g) => g.clone(),
        }
    }
}

/// Where a resolved node came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    pub value: bool,
    /// The holder's name (a group name, or the user's).
    pub holder: String,
    pub contexts: ContextSet,
    pub expiry: i64,
}

/// What a check found and which processor found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckResult {
    pub result: Tristate,
    /// `direct`, `regex`, `wildcard`, `op`, or empty when undefined.
    pub processor: &'static str,
    /// The node that decided it.
    pub node: Option<String>,
}

/// Resolved meta (`CachedMetaData`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetaData {
    pub prefix: Option<String>,
    pub suffix: Option<String>,
    /// Priority → value, highest first.
    pub prefixes: Vec<(i32, String)>,
    pub suffixes: Vec<(i32, String)>,
    /// Key → values, the first being the one that counts.
    pub meta: Vec<(String, Vec<String>)>,
    pub weight: Option<i32>,
    pub primary_group: Option<String>,
}

impl MetaData {
    pub fn meta_value(&self, key: &str) -> Option<&str> {
        let key = key.to_lowercase();
        self.meta.iter().find(|(k, _)| k.to_lowercase() == key).and_then(|(_, v)| v.first()).map(String::as_str)
    }
}

/// Everything a holder has in one set of contexts.
#[derive(Clone, Debug)]
pub struct Resolved {
    /// Lower-case key → the node that counts.
    pub permissions: HashMap<String, Origin>,
    wildcards: HashMap<String, bool>,
    root_wildcard: Option<bool>,
    regex: Vec<(regex::Regex, bool, String)>,
    pub groups: Vec<String>,
    pub meta: MetaData,
    /// The user counts as op (operator level reached and `enable-ops`).
    pub op: bool,
}

impl Resolved {
    /// The processors in Bukkit's order: direct, regex, wildcard, then the op processor.
    pub fn check(&self, permission: &str) -> CheckResult {
        let p = permission.to_lowercase();
        if let Some(o) = self.permissions.get(&p) {
            return CheckResult { result: Tristate::of(o.value), processor: "direct", node: Some(p) };
        }
        for (re, v, key) in &self.regex {
            if re.is_match(&p) {
                return CheckResult { result: Tristate::of(*v), processor: "regex", node: Some(key.clone()) };
            }
        }
        let mut node = p.as_str();
        while let Some(i) = node.rfind('.') {
            node = &node[..i];
            if node.is_empty() {
                break;
            }
            if let Some(v) = self.wildcards.get(node) {
                return CheckResult { result: Tristate::of(*v), processor: "wildcard", node: Some(format!("{node}.*")) };
            }
        }
        if let Some(v) = self.root_wildcard {
            return CheckResult { result: Tristate::of(v), processor: "wildcard", node: Some("*".into()) };
        }
        if self.op {
            return CheckResult { result: Tristate::True, processor: "op", node: None };
        }
        CheckResult { result: Tristate::Undefined, processor: "", node: None }
    }

    /// The direct state of a node (no wildcard, regex or op): what `/lp ... permission check`
    /// reports first.
    pub fn direct(&self, permission: &str) -> Option<&Origin> {
        self.permissions.get(&permission.to_lowercase())
    }
}

/// The weight a group sorts by: its highest weight node, else the config's `group-weight`
/// fallback (LuckPerms `WeightCache`), else 0.
pub fn group_weight(model: &Model, cfg: &Config, group: &str) -> i32 {
    group_weight_opt(model, cfg, group).unwrap_or(0)
}

fn group_weight_opt(model: &Model, cfg: &Config, group: &str) -> Option<i32> {
    model.groups.get(group).and_then(|g| g.weight()).or_else(|| cfg.group_weight.get(group).copied())
}

fn holder_nodes<'a>(model: &'a Model, h: &HolderRef) -> Vec<&'a Node> {
    match h {
        HolderRef::User(u) => model.users.get(u).map(|u| u.data.sorted()).unwrap_or_default(),
        HolderRef::Group(g) => model.groups.get(g).map(|g| g.data.sorted()).unwrap_or_default(),
    }
}

/// A holder's own nodes that apply in `contexts`, in LuckPerms' order (`NodeMap.copyTo` with a
/// query): normal nodes of a context set the global flags exclude are dropped, its inheritance
/// nodes kept when the flags allow.
fn own_nodes<'a>(model: &'a Model, h: &HolderRef, contexts: &ContextSet, flags: GlobalFlags, now: i64) -> Vec<&'a Node> {
    holder_nodes(model, h)
        .into_iter()
        .filter(|n| !n.has_expired(now))
        .filter(|n| n.contexts.is_satisfied_by(contexts, SatisfyMode::AtLeastOneValuePerKey))
        .filter(|n| {
            if flags.excludes_normal(&n.contexts) {
                matches!(n.kind, Kind::Inheritance(_)) && flags.includes_inheritance(&n.contexts)
            } else {
                true
            }
        })
        .collect()
}

/// The groups a holder inherits directly in `contexts`, sorted by weight (highest first), the
/// user's stored primary group first among equals (`InheritanceGraph.successors`).
fn successors(model: &Model, cfg: &Config, h: &HolderRef, contexts: &ContextSet, now: i64) -> Vec<HolderRef> {
    let flags = cfg.global_flags();
    let mut seen = HashSet::new();
    let mut groups: Vec<String> = Vec::new();
    for n in holder_nodes(model, h) {
        let Kind::Inheritance(g) = &n.kind else { continue };
        if !n.value || n.has_expired(now) || !n.contexts.is_satisfied_by(contexts, SatisfyMode::AtLeastOneValuePerKey) {
            continue;
        }
        if !flags.includes_inheritance(&n.contexts) {
            continue;
        }
        if model.groups.contains_key(g) && seen.insert(g.clone()) {
            groups.push(g.clone());
        }
    }
    let primary = match h {
        HolderRef::User(u) => model.users.get(u).map(|u| u.primary_group.clone()),
        HolderRef::Group(_) => None,
    };
    // A stable sort: weight descending, then the primary group.
    groups.sort_by(|a, b| {
        let wa = group_weight(model, cfg, a);
        let wb = group_weight(model, cfg, b);
        wb.cmp(&wa).then_with(|| {
            let pa = primary.as_deref() == Some(a.as_str());
            let pb = primary.as_deref() == Some(b.as_str());
            pb.cmp(&pa)
        })
    });
    groups.into_iter().map(HolderRef::Group).collect()
}

/// The holders to walk, the start first (`InheritanceGraph.traverse`).
pub fn traverse(model: &Model, cfg: &Config, start: &HolderRef, contexts: &ContextSet, now: i64) -> Vec<HolderRef> {
    let mut out: Vec<HolderRef> = Vec::new();
    let mut visited: HashSet<HolderRef> = HashSet::new();
    match cfg.inheritance_traversal_algorithm.as_str() {
        "breadth-first" => {
            let mut queue = std::collections::VecDeque::new();
            queue.push_back(start.clone());
            visited.insert(start.clone());
            while let Some(h) = queue.pop_front() {
                for s in successors(model, cfg, &h, contexts, now) {
                    if visited.insert(s.clone()) {
                        queue.push_back(s);
                    }
                }
                out.push(h);
            }
        }
        "depth-first-post-order" => {
            fn post(model: &Model, cfg: &Config, h: &HolderRef, c: &ContextSet, now: i64, v: &mut HashSet<HolderRef>, out: &mut Vec<HolderRef>) {
                for s in successors(model, cfg, h, c, now) {
                    if v.insert(s.clone()) {
                        post(model, cfg, &s, c, now, v, out);
                    }
                }
                out.push(h.clone());
            }
            visited.insert(start.clone());
            post(model, cfg, start, contexts, now, &mut visited, &mut out);
        }
        _ => {
            // depth-first-pre-order (the default), with an explicit stack.
            let mut stack = vec![start.clone()];
            while let Some(h) = stack.pop() {
                if !visited.insert(h.clone()) {
                    continue;
                }
                let succ = successors(model, cfg, &h, contexts, now);
                for s in succ.into_iter().rev() {
                    if !visited.contains(&s) {
                        stack.push(s);
                    }
                }
                out.push(h);
            }
        }
    }
    if cfg.post_traversal_inheritance_sort {
        let primary = match start {
            HolderRef::User(u) => model.users.get(u).map(|u| u.primary_group.clone()),
            _ => None,
        };
        out.sort_by(|a, b| match (a, b) {
            (HolderRef::User(_), HolderRef::User(_)) => std::cmp::Ordering::Equal,
            (HolderRef::User(_), _) => std::cmp::Ordering::Less,
            (_, HolderRef::User(_)) => std::cmp::Ordering::Greater,
            (HolderRef::Group(x), HolderRef::Group(y)) => group_weight(model, cfg, y)
                .cmp(&group_weight(model, cfg, x))
                .then_with(|| (primary.as_deref() == Some(y.as_str())).cmp(&(primary.as_deref() == Some(x.as_str())))),
        });
    }
    out
}

/// The primary group (`primary-group-calculation`).
pub fn primary_group(model: &Model, cfg: &Config, uuid: u128, contexts: &ContextSet, now: i64) -> String {
    let stored = model.users.get(&uuid).map(|u| u.primary_group.clone()).unwrap_or_else(|| DEFAULT_GROUP.into());
    let start = HolderRef::User(uuid);
    match cfg.primary_group_calculation.as_str() {
        "stored" => stored,
        "all-parents-by-weight" => {
            let mut c = cfg.clone();
            c.post_traversal_inheritance_sort = true;
            traverse(model, &c, &start, contexts, now)
                .into_iter()
                .find_map(|h| if let HolderRef::Group(g) = h { Some(g) } else { None })
                .unwrap_or(stored)
        }
        _ => {
            let mut best: Option<(String, i32)> = None;
            let mut seen = HashSet::new();
            for n in holder_nodes(model, &start) {
                let Kind::Inheritance(g) = &n.kind else { continue };
                if !n.value || n.has_expired(now) || !n.contexts.is_satisfied_by(contexts, SatisfyMode::AtLeastOneValuePerKey) {
                    continue;
                }
                if !model.groups.contains_key(g) || !seen.insert(g.clone()) {
                    continue;
                }
                let w = group_weight(model, cfg, g);
                if best.as_ref().is_none_or(|(_, bw)| w > *bw) {
                    best = Some((g.clone(), w));
                }
            }
            best.map(|(g, _)| g).unwrap_or(stored)
        }
    }
}

/// Resolves a holder in `contexts`. `op` says whether the player is op (always false for a
/// group).
pub fn resolve(model: &Model, cfg: &Config, holder: &HolderRef, contexts: &ContextSet, op: bool, now: i64) -> Resolved {
    let flags = cfg.global_flags();
    let order = traverse(model, cfg, holder, contexts, now);
    let mut permissions: HashMap<String, Origin> = HashMap::new();
    let mut all: Vec<(&Node, String)> = Vec::new();
    for h in &order {
        let name = h.name(model);
        for n in own_nodes(model, h, contexts, flags, now) {
            all.push((n, name.clone()));
        }
    }
    for (n, holder_name) in &all {
        permissions.entry(n.key.to_lowercase()).or_insert_with(|| Origin {
            value: n.value,
            holder: holder_name.clone(),
            contexts: n.contexts.clone(),
            expiry: n.expiry,
        });
    }
    if cfg.apply_shorthand {
        for (n, holder_name) in &all {
            if !matches!(n.kind, Kind::Permission) || !(n.key.contains('(') || n.key.contains('{')) {
                continue;
            }
            for s in expand_shorthand(&n.key.to_lowercase()) {
                permissions.entry(s).or_insert_with(|| Origin {
                    value: n.value,
                    holder: holder_name.clone(),
                    contexts: n.contexts.clone(),
                    expiry: n.expiry,
                });
            }
        }
    }
    let mut wildcards = HashMap::new();
    let mut root_wildcard = None;
    let mut regex = Vec::new();
    for (k, o) in &permissions {
        if cfg.apply_wildcards {
            if k == "*" || k == "'*'" {
                if root_wildcard.is_none() || k == "*" {
                    root_wildcard = Some(o.value);
                }
            } else if k.len() > 2 && k.ends_with(".*") {
                wildcards.insert(k[..k.len() - 2].to_owned(), o.value);
            }
        }
        if cfg.apply_regex && (k.starts_with("r=")) {
            if let Ok(re) = regex::Regex::new(&format!("^(?:{})$", &k[2..])) {
                regex.push((re, o.value, k.clone()));
            }
        }
    }
    regex.sort_by(|a, b| a.2.cmp(&b.2));
    let groups = order
        .iter()
        .filter_map(|h| if let HolderRef::Group(g) = h { Some(g.clone()) } else { None })
        .collect();
    let meta = accumulate_meta(model, cfg, holder, &order, contexts, now);
    Resolved { permissions, wildcards, root_wildcard, regex, groups, meta, op: op && cfg.horizon.enable_ops }
}

/// One element of a meta stack (`highest`, `lowest_own`, `highest_on_track_staff`, ...).
#[derive(Clone, Debug)]
struct StackElement {
    highest: bool,
    filter: StackFilter,
}

#[derive(Clone, Debug)]
enum StackFilter {
    Any,
    Own,
    Inherited,
    OnTrack(String),
    NotOnTrack(String),
    FromGroup(String),
    NotFromGroup(String),
}

fn parse_element(s: &str) -> Option<StackElement> {
    let s = s.to_lowercase();
    let (highest, rest) = if let Some(r) = s.strip_prefix("highest") {
        (true, r)
    } else if let Some(r) = s.strip_prefix("lowest") {
        (false, r)
    } else {
        return None;
    };
    let filter = match rest {
        "" => StackFilter::Any,
        "_own" => StackFilter::Own,
        "_inherited" => StackFilter::Inherited,
        r => {
            if let Some(t) = r.strip_prefix("_on_track_") {
                StackFilter::OnTrack(t.into())
            } else if let Some(t) = r.strip_prefix("_not_on_track_") {
                StackFilter::NotOnTrack(t.into())
            } else if let Some(g) = r.strip_prefix("_from_group_") {
                StackFilter::FromGroup(g.into())
            } else if let Some(g) = r.strip_prefix("_not_from_group_") {
                StackFilter::NotFromGroup(g.into())
            } else {
                return None;
            }
        }
    };
    Some(StackElement { highest, filter })
}

fn element_accepts(model: &Model, el: &StackElement, origin: &HolderRef, from: &HolderRef) -> bool {
    let group = if let HolderRef::Group(g) = from { Some(g.as_str()) } else { None };
    match &el.filter {
        StackFilter::Any => true,
        StackFilter::Own => from == origin,
        StackFilter::Inherited => from != origin,
        StackFilter::OnTrack(t) => group.is_some_and(|g| model.tracks.get(t).is_some_and(|t| t.groups.iter().any(|x| x == g))),
        StackFilter::NotOnTrack(t) => group.is_some_and(|g| !model.tracks.get(t).is_some_and(|t| t.groups.iter().any(|x| x == g))),
        StackFilter::FromGroup(x) => group == Some(x.as_str()),
        StackFilter::NotFromGroup(x) => group.is_some_and(|g| g != x),
    }
}

/// Builds a stacked prefix or suffix from the candidates in inheritance order.
fn stack(model: &Model, fmt: &StackFormat, origin: &HolderRef, candidates: &[(i32, String, HolderRef)]) -> Option<String> {
    let mut picked: Vec<String> = Vec::new();
    for e in fmt.format.iter().filter_map(|e| parse_element(e)) {
        let mut best: Option<(i32, &str)> = None;
        for (p, v, from) in candidates {
            if !element_accepts(model, &e, origin, from) {
                continue;
            }
            let better = match best {
                None => true,
                Some((bp, _)) => {
                    if e.highest {
                        *p > bp
                    } else {
                        *p < bp
                    }
                }
            };
            if better {
                best = Some((*p, v));
            }
        }
        if let Some((_, v)) = best {
            picked.push(v.to_owned());
        }
    }
    match fmt.duplicates.as_str() {
        "retain-all" => {}
        "last-only" => {
            let mut out: Vec<String> = Vec::new();
            for v in picked.iter().rev() {
                if !out.contains(v) {
                    out.insert(0, v.clone());
                }
            }
            picked = out;
        }
        _ => {
            let mut out: Vec<String> = Vec::new();
            for v in &picked {
                if !out.contains(v) {
                    out.push(v.clone());
                }
            }
            picked = out;
        }
    }
    if picked.is_empty() {
        return None;
    }
    Some(format!("{}{}{}", fmt.start_spacer, picked.join(&fmt.middle_spacer), fmt.end_spacer))
}

fn accumulate_meta(model: &Model, cfg: &Config, origin: &HolderRef, order: &[HolderRef], contexts: &ContextSet, now: i64) -> MetaData {
    let flags = cfg.global_flags();
    let mut seen = HashSet::new();
    let mut meta: Vec<(String, Vec<String>)> = Vec::new();
    let mut prefixes: Vec<(i32, String)> = Vec::new();
    let mut suffixes: Vec<(i32, String)> = Vec::new();
    let mut prefix_c: Vec<(i32, String, HolderRef)> = Vec::new();
    let mut suffix_c: Vec<(i32, String, HolderRef)> = Vec::new();
    let mut weight: Option<i32> = None;
    for h in order {
        for n in own_nodes(model, h, contexts, flags, now) {
            if !n.kind.is_meta_or_chat_meta() || !seen.insert(n.key.clone()) || !n.value {
                continue;
            }
            match &n.kind {
                Kind::Meta(k, v) => match meta.iter_mut().find(|(mk, _)| mk == k) {
                    Some((_, vs)) => vs.push(v.clone()),
                    None => meta.push((k.clone(), vec![v.clone()])),
                },
                Kind::Prefix(p, v) => {
                    if !prefixes.iter().any(|(pp, _)| pp == p) {
                        prefixes.push((*p, v.clone()));
                    }
                    prefix_c.push((*p, v.clone(), h.clone()));
                }
                Kind::Suffix(p, v) => {
                    if !suffixes.iter().any(|(pp, _)| pp == p) {
                        suffixes.push((*p, v.clone()));
                    }
                    suffix_c.push((*p, v.clone(), h.clone()));
                }
                _ => {}
            }
        }
        if let HolderRef::Group(g) = h {
            let w = group_weight_opt(model, cfg, g);
            if let Some(w) = w {
                if weight.is_none_or(|cur| w > cur) {
                    weight = Some(w);
                }
            }
        }
    }
    prefixes.sort_by(|a, b| b.0.cmp(&a.0));
    suffixes.sort_by(|a, b| b.0.cmp(&a.0));
    let primary_group = if let HolderRef::User(u) = origin { Some(primary_group(model, cfg, *u, contexts, now)) } else { None };
    if let Some(w) = weight {
        if !meta.iter().any(|(k, _)| k == "weight") {
            meta.push(("weight".into(), vec![w.to_string()]));
        }
    }
    if let Some(pg) = &primary_group {
        if !meta.iter().any(|(k, _)| k == "primarygroup") {
            meta.push(("primarygroup".into(), vec![pg.clone()]));
        }
    }
    MetaData {
        prefix: stack(model, &cfg.meta_formatting.prefix, origin, &prefix_c),
        suffix: stack(model, &cfg.meta_formatting.suffix, origin, &suffix_c),
        prefixes,
        suffixes,
        meta,
        weight,
        primary_group,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Group, User};
    use crate::node::key;

    fn g(model: &mut Model, name: &str, nodes: &[(&str, bool, &[(&str, &str)])]) {
        let mut gr = Group::new(name);
        for (k, v, c) in nodes {
            gr.data.set(Node::permanent(*k, *v, ContextSet::of(c)));
        }
        model.groups.insert(name.into(), gr);
    }

    fn setup() -> (Model, Config, u128) {
        let mut m = Model::default();
        g(&mut m, "default", &[("essentials.spawn", true, &[]), ("prefix.1.&7", true, &[])]);
        g(&mut m, "mod", &[("weight.50", true, &[]), ("group.default", true, &[]), ("essentials.*", true, &[]), ("essentials.nuke", false, &[]), ("prefix.50.&9[Mod] ", true, &[])]);
        g(&mut m, "admin", &[("weight.100", true, &[]), ("group.mod", true, &[]), ("*", true, &[]), ("prefix.100.&c[Admin] ", true, &[])]);
        g(&mut m, "builder", &[("weight.10", true, &[]), ("worldedit.*", true, &[("world", "buildfab")]), ("meta.color.gold", true, &[])]);
        let mut u = User::new(7);
        u.username = Some("alice".into());
        u.data.set(Node::permanent(key::group("mod"), true, ContextSet::new()));
        u.data.set(Node::permanent(key::group("builder"), true, ContextSet::new()));
        u.data.set(Node::permanent("essentials.nuke", true, ContextSet::of(&[("server", "lobby")])));
        m.users.insert(7, u);
        (m, Config::default(), 7)
    }

    #[test]
    fn group_weight_config_is_only_a_fallback() {
        let (m, mut c, _) = setup();
        c.group_weight.insert("mod".into(), 500);
        c.group_weight.insert("default".into(), 3);
        assert_eq!(group_weight(&m, &c, "mod"), 50, "the weight node wins");
        assert_eq!(group_weight(&m, &c, "default"), 3, "config used without a weight node");
        assert_eq!(group_weight(&m, &c, "nobody"), 0);
    }

    #[test]
    fn inheritance_wildcards_and_negation() {
        let (m, c, u) = setup();
        let ctx = ContextSet::of(&[("server", "survival"), ("world", "world")]);
        let r = resolve(&m, &c, &HolderRef::User(u), &ctx, false, 0);
        assert_eq!(r.check("essentials.fly").result, Tristate::True);
        assert_eq!(r.check("essentials.fly").processor, "wildcard");
        assert_eq!(r.check("essentials.nuke").result, Tristate::False);
        assert_eq!(r.check("essentials.spawn").result, Tristate::True);
        assert_eq!(r.check("worldedit.wand").result, Tristate::Undefined);
        assert_eq!(r.check("Some.Other").result, Tristate::Undefined);
        assert_eq!(r.groups, vec!["mod", "default", "builder"]);
    }

    #[test]
    fn contexts_apply() {
        let (m, c, u) = setup();
        let lobby = ContextSet::of(&[("server", "lobby"), ("world", "buildfab")]);
        let r = resolve(&m, &c, &HolderRef::User(u), &lobby, false, 0);
        assert_eq!(r.check("essentials.nuke").result, Tristate::True);
        assert_eq!(r.check("worldedit.wand").result, Tristate::True);
        assert_eq!(r.direct("essentials.nuke").unwrap().holder, "alice");
    }

    #[test]
    fn op_fills_undefined_only() {
        let (m, c, u) = setup();
        let r = resolve(&m, &c, &HolderRef::User(u), &ContextSet::new(), true, 0);
        assert_eq!(r.check("anything.at.all").result, Tristate::True);
        assert_eq!(r.check("anything.at.all").processor, "op");
        assert_eq!(r.check("essentials.nuke").result, Tristate::False);
    }

    #[test]
    fn meta_and_primary() {
        let (m, c, u) = setup();
        let r = resolve(&m, &c, &HolderRef::User(u), &ContextSet::new(), false, 0);
        assert_eq!(r.meta.prefix.as_deref(), Some("&9[Mod] "));
        assert_eq!(r.meta.primary_group.as_deref(), Some("mod"));
        assert_eq!(r.meta.weight, Some(50));
        assert_eq!(r.meta.meta_value("color"), Some("gold"));
        assert_eq!(r.meta.meta_value("primarygroup"), Some("mod"));
        assert_eq!(r.meta.prefixes.first().map(|p| p.0), Some(50));
    }

    #[test]
    fn root_wildcard_and_group_check() {
        let (m, c, _) = setup();
        let r = resolve(&m, &c, &HolderRef::Group("admin".into()), &ContextSet::new(), false, 0);
        assert_eq!(r.check("x.y").result, Tristate::True);
        // admin inherits mod's negation but its own root wildcard does not beat a direct false.
        assert_eq!(r.check("essentials.nuke").result, Tristate::False);
        assert_eq!(r.meta.prefix.as_deref(), Some("&c[Admin] "));
    }

    #[test]
    fn regex_and_shorthand() {
        let mut m = Model::default();
        g(&mut m, "default", &[("R=plots\\.claim\\.[0-9]+", true, &[]), ("kits.(daily|weekly)", true, &[])]);
        let mut u = User::new(1);
        u.give_default_if_needed();
        m.users.insert(1, u);
        let r = resolve(&m, &Config::default(), &HolderRef::User(1), &ContextSet::new(), false, 0);
        assert_eq!(r.check("plots.claim.3").result, Tristate::True);
        assert_eq!(r.check("plots.claim.x").result, Tristate::Undefined);
        assert_eq!(r.check("kits.weekly").result, Tristate::True);
    }

    #[test]
    fn expired_nodes_do_not_count() {
        let mut m = Model::default();
        g(&mut m, "default", &[]);
        let mut u = User::new(2);
        u.data.set(Node::new("temp.perm", true, 50, ContextSet::new()));
        m.users.insert(2, u);
        let c = Config::default();
        assert_eq!(resolve(&m, &c, &HolderRef::User(2), &ContextSet::new(), false, 10).check("temp.perm").result, Tristate::True);
        assert_eq!(resolve(&m, &c, &HolderRef::User(2), &ContextSet::new(), false, 60).check("temp.perm").result, Tristate::Undefined);
    }

    #[test]
    fn stack_formats() {
        let (m, mut c, u) = setup();
        c.meta_formatting.prefix.format = vec!["highest_inherited".into(), "lowest".into()];
        let r = resolve(&m, &c, &HolderRef::User(u), &ContextSet::new(), false, 0);
        assert_eq!(r.meta.prefix.as_deref(), Some("&9[Mod]  &7"));
    }

    #[test]
    fn global_flags_exclude() {
        let (m, mut c, u) = setup();
        c.include_global = false;
        let r = resolve(&m, &c, &HolderRef::User(u), &ContextSet::of(&[("server", "lobby")]), false, 0);
        // Only the server-specific node remains; inheritance still applies but groups' global nodes do not.
        assert_eq!(r.check("essentials.nuke").result, Tristate::True);
        assert_eq!(r.check("essentials.fly").result, Tristate::Undefined);
    }
}
