//! The `/lp` command tree the client shows: every main command, `user|group <x>` child and sub command of
//! `CommandManager` and the command classes (the tables in `tables.rs`), one usage each, so the chat box lists
//! the sub commands and their arguments. The arguments of a leaf after its command words are one greedy tail
//! (`node_value_context`): the original reads them by position, a tail keeps the typed line whole and its
//! completion is the port's own (`complete`, tab completion by position), which is what the tail's argument type asks.
//!
//! A player's typed line never goes through this tree (`player_command` takes it whole); the console and command
//! blocks do, and `rebuild` turns the parsed invocation back into the line `run` takes.

use super::specs::spec;
use super::tables::{holder_children, parent_subs, Auth, Child, Sub, LOG_CHILDREN, MAINS, TRACK_CHILDREN};

/// One usage of the tree (root word `lp`; the other roots are aliases of it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeUsage {
    pub usage: String,
    /// The `luckperms.*` permission node that runs it; `None` for the traversal prefixes (`lp user <user>`).
    pub node: Option<String>,
}

/// A completion type of the tree: the words the completion code sees in front of the typed text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TailDef {
    /// The name the type is registered under (`user_permission_set`).
    pub id: String,
    /// The command words before it (`user x permission set`), a placeholder for the holder's name.
    pub words: Vec<String>,
    /// A tail takes the rest of the line; the holder names (`user`, `group`, `track`) one word.
    pub greedy: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub usages: Vec<TreeUsage>,
    pub tails: Vec<TailDef>,
}

/// The kinds of the three holder names, as the registered ids (`luckperms:user`).
pub const KIND_USER: &str = "user";
pub const KIND_GROUP: &str = "group";
pub const KIND_TRACK: &str = "track";

/// The name of the placeholder the completion code gets for a holder's name.
const HOLDER: &str = "x";

fn name_of(args: &[super::specs::SpecArg]) -> String {
    let parts: Vec<String> = args
        .iter()
        .map(|a| a.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' }).collect::<String>())
        .collect();
    parts.join("_")
}

struct Builder {
    tree: Tree,
}

impl Builder {
    fn prefix(&mut self, usage: String) {
        self.tree.usages.push(TreeUsage { usage, node: None });
    }

    /// A runnable usage with its spec's arguments as one tail.
    fn leaf(&mut self, head: &str, words: Vec<String>, spec_key: &str, node: String) {
        let s = spec(spec_key);
        let mut usage = head.to_owned();
        if !s.args.is_empty() {
            let id = words.iter().map(|w| if w == HOLDER { "name" } else { w.as_str() }).collect::<Vec<_>>().join("_");
            usage.push_str(&format!(" [{}:luckperms:{id}]", name_of(s.args)));
            self.tree.tails.push(TailDef { id, words, greedy: true });
        }
        self.tree.usages.push(TreeUsage { usage, node: Some(node) });
    }

    fn holder(&mut self, user: bool) {
        let tag = if user { "user" } else { "group" };
        let kind = if user { KIND_USER } else { KIND_GROUP };
        self.prefix(format!("lp {tag} [{tag}:luckperms:{kind}]"));
        for ch in holder_children(user) {
            self.child(tag, kind, user, &ch);
        }
    }

    fn child(&mut self, tag: &str, kind: &str, user: bool, ch: &Child) {
        let head = format!("lp {tag} <{tag}:luckperms:{kind}> {}", ch.name);
        let base = vec![tag.to_owned(), HOLDER.to_owned(), ch.name.to_owned()];
        if ch.subs.is_empty() {
            self.leaf(&head, base, ch.spec, format!("luckperms.{tag}.{}", ch.node));
            return;
        }
        self.prefix(head.clone());
        let subs: Vec<Sub> = parent_subs(ch, user);
        for s in subs {
            let mut words = base.clone();
            words.push(s.name.to_owned());
            self.leaf(&format!("{head} {}", s.name), words, s.spec, format!("luckperms.{tag}.{}", s.node));
        }
    }

    fn children(&mut self, tag: &str, kind: &str, list: &[Child]) {
        if tag == "track" {
            self.prefix(format!("lp track [track:luckperms:{kind}]"));
        } else {
            self.prefix("lp log".to_owned());
        }
        for ch in list {
            let (head, words) = if tag == "track" {
                (format!("lp track <track:luckperms:{kind}> {}", ch.name), vec!["track".to_owned(), HOLDER.to_owned(), ch.name.to_owned()])
            } else {
                (format!("lp log {}", ch.name), vec!["log".to_owned(), ch.name.to_owned()])
            };
            self.leaf(&head, words, ch.spec, format!("luckperms.{tag}.{}", ch.node));
        }
    }
}

/// Every usage of the tree, with `lp` as the root word.
pub fn build() -> Tree {
    let mut b = Builder { tree: Tree::default() };
    b.prefix("lp".to_owned());
    for m in MAINS.iter().filter(|m| m.shown) {
        match m.auth {
            Auth::User => b.holder(true),
            Auth::Group => b.holder(false),
            Auth::Track => b.children("track", KIND_TRACK, TRACK_CHILDREN),
            Auth::Log => b.children("log", "", LOG_CHILDREN),
            Auth::Node(node) => b.leaf(&format!("lp {}", m.name), vec![m.name.to_owned()], m.spec, format!("luckperms.{node}")),
        }
    }
    let mut tree = b.tree;
    for (kind, word) in [(KIND_USER, "user"), (KIND_GROUP, "group"), (KIND_TRACK, "track")] {
        tree.tails.push(TailDef { id: kind.to_owned(), words: vec![word.to_owned()], greedy: false });
    }
    tree
}

/// The line (without the root word) a parsed invocation stands for: the usage with the same command words is walked
/// and each of its arguments replaced by the value the invocation holds (an omitted optional one is skipped).
/// `words` are the invocation's literals, `values` its arguments by name.
pub fn rebuild(usages: &[TreeUsage], words: &[String], values: &[(String, String)]) -> String {
    let literal_count = |u: &str| u.split_whitespace().filter(|t| !t.starts_with('<') && !t.starts_with('[')).count();
    let template = usages.iter().find(|u| {
        let tokens: Vec<&str> = u.usage.split_whitespace().collect();
        let lits: Vec<&&str> = tokens.iter().filter(|t| !t.starts_with('<') && !t.starts_with('[')).collect();
        literal_count(&u.usage) == words.len()
            && lits.iter().skip(1).zip(words.iter().skip(1)).all(|(a, b)| a.eq_ignore_ascii_case(b))
            && values.iter().all(|(n, _)| {
                tokens.iter().any(|t| t[1..].split(':').next() == Some(n.as_str()) && (t.starts_with('<') || t.starts_with('[')))
            })
    });
    let mut out: Vec<String> = Vec::new();
    let mut lit = 0;
    match template {
        Some(u) => {
            for token in u.usage.split_whitespace() {
                if token.starts_with('<') || token.starts_with('[') {
                    let name = token[1..].split(':').next().unwrap_or("");
                    if let Some((_, v)) = values.iter().find(|(n, _)| n == name) {
                        out.push(v.clone());
                    }
                } else {
                    out.push(words.get(lit).cloned().unwrap_or_else(|| token.to_owned()));
                    lit += 1;
                }
            }
        }
        None => {
            out.extend(words.iter().cloned());
            out.extend(values.iter().map(|(_, v)| v.clone()));
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usages() -> Vec<String> {
        build().usages.into_iter().map(|u| u.usage).collect()
    }

    fn has(prefix: &str) -> bool {
        usages().iter().any(|u| u == prefix || u.starts_with(&format!("{prefix} ")))
    }

    #[test]
    fn the_main_commands_of_the_original_are_in_the_tree() {
        for main in [
            "user", "group", "track", "log", "sync", "info", "editor", "verbose", "tree", "search", "networksync", "import", "export",
            "reloadconfig", "bulkupdate", "translations", "creategroup", "deletegroup", "listgroups", "createtrack", "deletetrack",
            "listtracks",
        ] {
            assert!(has(&format!("lp {main}")), "lp {main}");
        }
    }

    #[test]
    fn the_holder_commands_of_the_original_are_in_the_tree() {
        for (tag, subs) in [
            ("user", &["info", "editor", "promote", "demote", "showtracks", "clear", "clone"][..]),
            ("group", &["info", "editor", "listmembers", "setweight", "setdisplayname", "showtracks", "clear", "rename", "clone"][..]),
        ] {
            for s in subs {
                let want = format!("lp {tag} <{tag}:luckperms:{tag}> {s}");
                assert!(has(&want), "{want}");
            }
        }
        let u = usages();
        for want in [
            "lp user <user:luckperms:user> permission set",
            "lp user <user:luckperms:user> permission unset",
            "lp user <user:luckperms:user> permission settemp",
            "lp user <user:luckperms:user> permission unsettemp",
            "lp user <user:luckperms:user> permission check",
            "lp user <user:luckperms:user> permission clear",
            "lp user <user:luckperms:user> permission info",
            "lp user <user:luckperms:user> parent add",
            "lp user <user:luckperms:user> parent set",
            "lp user <user:luckperms:user> parent remove",
            "lp user <user:luckperms:user> parent settrack",
            "lp user <user:luckperms:user> parent addtemp",
            "lp user <user:luckperms:user> parent removetemp",
            "lp user <user:luckperms:user> parent cleartrack",
            "lp user <user:luckperms:user> parent switchprimarygroup",
            "lp user <user:luckperms:user> meta set",
            "lp user <user:luckperms:user> meta addprefix",
            "lp user <user:luckperms:user> meta setsuffix",
            "lp user <user:luckperms:user> meta addtempprefix",
            "lp user <user:luckperms:user> meta removetempsuffix",
            "lp group <group:luckperms:group> permission set",
            "lp group <group:luckperms:group> parent add",
            "lp group <group:luckperms:group> meta clear",
        ] {
            assert!(u.iter().any(|x| x == want || x.starts_with(&format!("{want} "))), "{want}");
        }
        // The group has no `switchprimarygroup`, the user no `listmembers`.
        assert!(!u.iter().any(|x| x.starts_with("lp group <group:luckperms:group> parent switchprimarygroup")));
        assert!(!u.iter().any(|x| x.contains("lp user <user:luckperms:user> listmembers")));
    }

    #[test]
    fn the_track_and_log_commands_of_the_original_are_in_the_tree() {
        for sub in ["info", "editor", "append", "insert", "remove", "clear", "rename", "clone"] {
            assert!(has(&format!("lp track <track:luckperms:track> {sub}")), "{sub}");
        }
        for sub in ["recent", "search", "notify", "userhistory", "grouphistory", "trackhistory"] {
            assert!(has(&format!("lp log {sub}")), "{sub}");
        }
    }

    #[test]
    fn a_leaf_with_arguments_has_a_tail_and_a_permission_node() {
        let t = build();
        let set = t.usages.iter().find(|u| u.usage.starts_with("lp user <user:luckperms:user> permission set")).unwrap();
        assert_eq!(set.node.as_deref(), Some("luckperms.user.permission.set"));
        assert!(set.usage.contains("[node_value_context:luckperms:user_name_permission_set]"), "{}", set.usage);
        let tail = t.tails.iter().find(|x| x.id == "user_name_permission_set").unwrap();
        assert_eq!(tail.words, ["user", "x", "permission", "set"]);
        let prefix = t.usages.iter().find(|u| u.usage == "lp user [user:luckperms:user]").unwrap();
        assert_eq!(prefix.node, None);
    }

    #[test]
    fn every_usage_is_a_valid_usage_with_registered_kinds() {
        let t = build();
        let tails: Vec<&str> = t.tails.iter().map(|x| x.id.as_str()).collect();
        for u in &t.usages {
            let mut greedy = false;
            for token in u.usage.split_whitespace() {
                assert!(!greedy, "an argument after the tail in `{}`", u.usage);
                if let Some(inner) = token.strip_prefix('<').and_then(|t| t.strip_suffix('>')).or_else(|| token.strip_prefix('[').and_then(|t| t.strip_suffix(']'))) {
                    let kind = inner.split_once(":luckperms:").map(|x| x.1).unwrap_or_else(|| panic!("`{token}` in `{}`", u.usage));
                    assert!(tails.contains(&kind), "kind {kind} of `{}`", u.usage);
                    greedy = t.tails.iter().any(|x| x.id == kind && x.greedy);
                } else {
                    assert!(token.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'), "`{token}`");
                }
            }
        }
    }

    #[test]
    fn an_invocation_is_rebuilt_into_the_typed_line() {
        let t = build();
        let line = rebuild(
            &t.usages,
            &["lp".into(), "user".into(), "permission".into(), "set".into()],
            &[("user".into(), "Alex".into()), ("node_value_context".into(), "essentials.* true server=lobby".into())],
        );
        assert_eq!(line, "lp user Alex permission set essentials.* true server=lobby");
        let line = rebuild(&t.usages, &["luckperms".into(), "listgroups".into()], &[]);
        assert_eq!(line, "luckperms listgroups");
        let line = rebuild(&t.usages, &["lp".into(), "group".into()], &[("group".into(), "admin".into())]);
        assert_eq!(line, "lp group admin");
    }
}
