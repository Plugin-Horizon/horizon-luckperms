//! `storage-method: rest`: LuckPerms' `RestStorage`, the client of the LuckPerms REST API
//! (github.com/LuckPerms/rest-api, MIT). `data.rest-url` is the API's base URL and `data.rest-auth-key` the
//! key it checks (sent as `Authorization: Bearer <key>`). Blocking HTTP (ureq) on the storage worker.
//!
//! Like the original it does not support bulk updates and answers `/lp search` only for `==` and `~~ foo%`.
//! A user's primary group comes from the API's `metadata.primaryGroup`. Messaging does not run over this storage
//! (there is no SQL messenger); use Redis or NATS.

use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::context::ContextSet;
use crate::horizon_luckperms_core::model::{Group, Track, User, DEFAULT_GROUP};
use crate::horizon_luckperms_core::node::{Kind, Node};
use crate::horizon_luckperms_core::storage::{diff, ActionFilter, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::horizon_luckperms_core::uuid;
use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

const TIMEOUT_MS: u64 = 15_000;
const MAX_BODY: u64 = 64 * 1024 * 1024;

pub struct RestStorage {
    base: String,
    key: String,
    agent: ureq::Agent,
}

/// A path segment: anything but letters, digits and `-_.~` is percent-encoded.
fn seg(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn kind_name(k: &Kind) -> &'static str {
    match k {
        Kind::Permission => "permission",
        Kind::Regex(_) => "regex_permission",
        Kind::Inheritance(_) => "inheritance",
        Kind::Prefix(..) => "prefix",
        Kind::Suffix(..) => "suffix",
        Kind::Meta(..) => "meta",
        Kind::Weight(_) => "weight",
        Kind::DisplayName(_) => "display_name",
    }
}

/// A node as the API takes it (`NewNode`).
pub fn node_json(n: &Node) -> Value {
    let ctx: Vec<Value> = n.contexts.ordered_pairs().into_iter().map(|(k, v)| json!({"key": k, "value": v})).collect();
    let mut o = json!({"key": n.key, "type": kind_name(&n.kind), "value": n.value, "context": ctx});
    if n.expiry > 0 {
        o["expiry"] = json!(n.expiry);
    }
    o
}

/// A node the API sent; None for one without a key.
pub fn node_from_json(v: &Value) -> Option<Node> {
    let key = v.get("key")?.as_str()?.to_owned();
    let value = v.get("value").and_then(Value::as_bool).unwrap_or(true);
    let expiry = v.get("expiry").and_then(Value::as_i64).unwrap_or(0).max(0);
    let mut ctx = ContextSet::new();
    for c in v.get("context").and_then(Value::as_array).into_iter().flatten() {
        if let (Some(k), Some(val)) = (c.get("key").and_then(Value::as_str), c.get("value").and_then(Value::as_str)) {
            ctx.add(k, val);
        }
    }
    Some(Node::new(key, value, expiry, ctx))
}

fn nodes_from(v: Option<&Value>) -> Vec<Node> {
    v.and_then(Value::as_array).into_iter().flatten().filter_map(node_from_json).collect()
}

fn action_json(a: &Action) -> Value {
    let mut target = json!({"name": a.target_name, "type": a.target_type.name().to_lowercase()});
    if let Some(u) = a.target_uuid {
        target["uniqueId"] = json!(uuid::format(u));
    }
    json!({
        "timestamp": a.timestamp,
        "source": {"uniqueId": uuid::format(a.source_uuid), "name": a.source_name},
        "target": target,
        "description": a.description,
    })
}

fn action_from_json(v: &Value) -> Option<Action> {
    let src = v.get("source")?;
    let tgt = v.get("target")?;
    Some(Action {
        timestamp: v.get("timestamp").and_then(Value::as_i64).unwrap_or(0),
        source_uuid: src.get("uniqueId").and_then(Value::as_str).and_then(uuid::parse).unwrap_or(0),
        source_name: src.get("name")?.as_str()?.to_owned(),
        target_type: TargetType::parse(tgt.get("type")?.as_str()?)?,
        target_uuid: tgt.get("uniqueId").and_then(Value::as_str).and_then(uuid::parse),
        target_name: tgt.get("name")?.as_str()?.to_owned(),
        description: v.get("description")?.as_str()?.to_owned(),
    })
}

impl RestStorage {
    pub fn new(url: &str, auth_key: &str) -> Result<Self> {
        let url = url.trim();
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err("data.rest-url must start with http:// or https://".into());
        }
        let base = if url.ends_with('/') { url.to_owned() } else { format!("{url}/") };
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_millis(TIMEOUT_MS)).redirects(0).build();
        Ok(RestStorage { base, key: auth_key.to_owned(), agent })
    }

    /// One request. None on 404 (nothing there), an error text (path only, never the key) on any other failure.
    fn call(&self, method: &str, path: &str, query: &[(&str, &str)], body: Option<&Value>) -> Result<Option<Value>> {
        let mut req = self.agent.request(method, &format!("{}{}", self.base, path)).set("User-Agent", "luckperms-horizon");
        for (k, v) in query {
            req = req.query(k, v);
        }
        if !self.key.is_empty() {
            req = req.set("Authorization", &format!("Bearer {}", self.key));
        }
        let result = match body {
            Some(b) => req.set("Content-Type", "application/json").send_string(&b.to_string()),
            None => req.call(),
        };
        match result {
            Ok(resp) => {
                let mut text = String::new();
                resp.into_reader().take(MAX_BODY).read_to_string(&mut text).map_err(|e| format!("rest {method} {path}: {e}"))?;
                if text.trim().is_empty() {
                    return Ok(Some(Value::Null));
                }
                serde_json::from_str(&text).map(Some).map_err(|_| format!("rest {method} {path}: the answer is not JSON"))
            }
            Err(ureq::Error::Status(404, _)) => Ok(None),
            Err(ureq::Error::Status(code, _)) => Err(format!("rest {method} {path}: HTTP {code}")),
            Err(e) => Err(format!("rest {method} {path}: {e}")),
        }
    }

    fn get_json(&self, path: &str, query: &[(&str, &str)]) -> Result<Option<Value>> {
        self.call("GET", path, query, None)
    }

    fn push_nodes(&self, holder_path: &str, before: &[Node], after: &[Node]) -> Result<()> {
        let (added, removed) = diff(before, after);
        let nodes = format!("{holder_path}/nodes");
        if !removed.is_empty() {
            self.call("DELETE", &nodes, &[], Some(&Value::Array(removed.iter().map(node_json).collect())))?;
        }
        if !added.is_empty() {
            self.call("PATCH", &nodes, &[("temporaryNodeMergeStrategy", "none")], Some(&Value::Array(added.iter().map(node_json).collect())))?;
        }
        Ok(())
    }

    fn names(&self, path: &str) -> Result<Vec<String>> {
        Ok(self.get_json(path, &[])?.as_ref().and_then(Value::as_array).into_iter().flatten().filter_map(|v| v.as_str().map(str::to_owned)).collect())
    }

    fn group_from(&self, v: &Value, name: &str) -> Group {
        let mut g = Group::new(name);
        g.data.nodes = nodes_from(v.get("nodes"));
        g
    }

    fn search_path(&self, base: &str, m: &KeyMatch) -> Result<Vec<Value>> {
        let (param, value) = match m {
            KeyMatch::Exact(k) => ("key", k.clone()),
            KeyMatch::Like(p) if p.ends_with('%') && !p[..p.len() - 1].contains(['%', '_']) => ("keyStartsWith", p[..p.len() - 1].to_owned()),
            KeyMatch::Like(p) if !p.contains(['%', '_']) => ("key", p.clone()),
            other => return Err(format!("rest: unsupported search {other:?} (only == and ~~ foo% work on the REST storage)")),
        };
        Ok(self.get_json(&format!("{base}/search"), &[(param, &value)])?.and_then(|v| v.as_array().cloned()).unwrap_or_default())
    }
}

impl Storage for RestStorage {
    fn name(&self) -> &'static str {
        "rest"
    }

    fn init(&mut self) -> Result<()> {
        match self.get_json("health", &[])? {
            Some(h) if h.get("health").and_then(Value::as_bool) == Some(true) => {}
            _ => eprintln!("[LuckPerms] REST storage endpoint is unhealthy"),
        }
        if self.get_json(&format!("group/{}", seg(DEFAULT_GROUP)), &[])?.is_none() {
            self.call("POST", "group", &[], Some(&json!({"name": DEFAULT_GROUP})))?;
        }
        Ok(())
    }

    fn load_user(&mut self, id: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let Some(v) = self.get_json(&format!("user/{}", uuid::format(id)), &[])? else { return Ok((Vec::new(), None)) };
        let nodes = nodes_from(v.get("nodes"));
        let row = v.get("username").and_then(Value::as_str).map(|n| PlayerRow {
            username: n.to_lowercase(),
            primary_group: v.pointer("/metadata/primaryGroup").and_then(Value::as_str).unwrap_or(DEFAULT_GROUP).to_owned(),
        });
        Ok((nodes, row))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let path = format!("user/{}", uuid::format(user.uuid));
        // A user with nothing but the default group is not exported (its default node is implied).
        let after: &[Node] = if user.is_non_default() { &user.data.nodes } else { &[] };
        if !user.is_non_default() && user.saved.is_empty() {
            return Ok(());
        }
        if let Some(name) = &user.username {
            self.call("PATCH", &path, &[], Some(&json!({"username": name})))?;
        }
        self.push_nodes(&path, &user.saved, after)
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        let mut out = Vec::new();
        for name in self.names("group")? {
            if let Some(v) = self.get_json(&format!("group/{}", seg(&name)), &[])? {
                out.push(self.group_from(&v, &name));
            }
        }
        Ok(out)
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        Ok(self.get_json(&format!("group/{}", seg(name)), &[])?.map(|v| self.group_from(&v, name)))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        self.call("POST", "group", &[], Some(&json!({"name": name.to_lowercase()}))).map(|_| ())
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        self.push_nodes(&format!("group/{}", seg(&group.name)), before, &group.data.nodes)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.call("DELETE", &format!("group/{}", seg(name)), &[], None).map(|_| ())
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        let mut out = Vec::new();
        for name in self.names("track")? {
            if let Some(v) = self.get_json(&format!("track/{}", seg(&name)), &[])? {
                let groups = v.get("groups").and_then(Value::as_array).into_iter().flatten().filter_map(|g| g.as_str().map(str::to_owned)).collect();
                out.push(Track { name, groups });
            }
        }
        Ok(out)
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        let path = format!("track/{}", seg(&track.name));
        let body = json!({"groups": track.groups});
        if self.call("PATCH", &path, &[], Some(&body))?.is_none() {
            self.call("POST", "track", &[], Some(&json!({"name": track.name})))?;
            self.call("PATCH", &path, &[], Some(&body))?;
        }
        Ok(())
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.call("DELETE", &format!("track/{}", seg(name)), &[], None).map(|_| ())
    }

    fn save_player_data(&mut self, id: u128, username: &str) -> Result<PlayerSave> {
        let v = self
            .call("POST", "user", &[], Some(&json!({"uniqueId": uuid::format(id), "username": username})))?
            .ok_or("rest POST user: not found")?;
        let outcomes: Vec<&str> = v.get("outcomes").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let mut r = PlayerSave { new_player: outcomes.contains(&"clean_insert"), ..PlayerSave::default() };
        if outcomes.contains(&"username_updated") {
            r.username_changed = v.get("previousUsername").and_then(Value::as_str).map(str::to_lowercase);
        }
        if outcomes.contains(&"other_unique_ids_present_for_username") {
            r.other_uuids = v.get("otherUniqueIds").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter_map(uuid::parse).collect();
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        Ok(self.get_json("user/lookup", &[("username", username)])?.and_then(|v| v.get("uniqueId").and_then(Value::as_str).and_then(uuid::parse)))
    }

    fn name_of(&mut self, id: u128) -> Result<Option<String>> {
        Ok(self
            .get_json("user/lookup", &[("uniqueId", &uuid::format(id))])?
            .and_then(|v| v.get("username").and_then(Value::as_str).map(str::to_lowercase)))
    }

    fn log_action(&mut self, action: &Action) -> Result<()> {
        self.call("POST", "action", &[], Some(&action_json(action))).map(|_| ())
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let query: Vec<(&str, String)> = match target {
            None => vec![],
            Some((TargetType::User, n)) => vec![("user", n.to_owned())],
            Some((TargetType::Group, n)) => vec![("group", n.to_owned())],
            Some((TargetType::Track, n)) => vec![("track", n.to_owned())],
        };
        let q: Vec<(&str, &str)> = query.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let v = self.get_json("action", &q)?.unwrap_or(Value::Null);
        let mut all: Vec<Action> = v.get("entries").and_then(Value::as_array).into_iter().flatten().filter_map(action_from_json).collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
        all.truncate(limit);
        Ok(all)
    }

    fn log_entries(&mut self, filter: &ActionFilter) -> Result<Vec<Action>> {
        const ALL: usize = usize::MAX;
        match filter {
            ActionFilter::Source(u) => {
                let id = uuid::format(*u);
                let v = self.get_json("action", &[("source", &id)])?.unwrap_or(Value::Null);
                let mut all: Vec<Action> = v.get("entries").and_then(Value::as_array).into_iter().flatten().filter_map(action_from_json).collect();
                all.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
                Ok(all)
            }
            ActionFilter::Search(q) => {
                let v = self.get_json("action", &[("search", q)])?.unwrap_or(Value::Null);
                let mut all: Vec<Action> = v.get("entries").and_then(Value::as_array).into_iter().flatten().filter_map(action_from_json).collect();
                all.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
                Ok(all)
            }
            ActionFilter::All => self.actions(ALL, None),
            ActionFilter::User(u) => self.actions(ALL, Some((TargetType::User, &uuid::format(*u)))),
            ActionFilter::Group(g) => self.actions(ALL, Some((TargetType::Group, g))),
            ActionFilter::Track(t) => self.actions(ALL, Some((TargetType::Track, t))),
        }
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        let mut out = Vec::new();
        for r in self.search_path("user", key)? {
            let Some(id) = r.get("uniqueId").and_then(Value::as_str).and_then(uuid::parse) else { continue };
            out.extend(nodes_from(r.get("results")).into_iter().map(|n| (HolderKey::User(id), n)));
        }
        for r in self.search_path("group", key)? {
            let Some(name) = r.get("name").and_then(Value::as_str) else { continue };
            out.extend(nodes_from(r.get("results")).into_iter().map(|n| (HolderKey::Group(name.to_owned()), n)));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        let mut ids: Vec<u128> = self.names("user")?.iter().filter_map(|s| uuid::parse(s)).collect();
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    fn sql_messenger(&self) -> bool {
        false
    }
    fn messenger_init(&mut self) -> Result<i64> {
        Ok(0)
    }
    fn messenger_send(&mut self, _message: &str) -> Result<()> {
        Ok(())
    }
    fn messenger_poll(&mut self, _last_id: i64) -> Result<Vec<(i64, String)>> {
        Ok(Vec::new())
    }
    fn messenger_housekeep(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    type Seen = Arc<Mutex<Vec<(String, String, String)>>>;

    /// A one-thread HTTP server: answers every request from `route(method, path_and_query, body)` as
    /// `(status, json)`, and records the requests (and the Authorization header on the first line of `body`).
    fn serve(route: impl Fn(&str, &str, &str) -> (u16, String) + Send + 'static) -> (String, Seen) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/", l.local_addr().unwrap());
        let seen: Seen = Arc::default();
        let log = seen.clone();
        std::thread::spawn(move || {
            for stream in l.incoming() {
                let Ok(mut s) = stream else { break };
                let mut r = BufReader::new(s.try_clone().unwrap());
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                let mut parts = line.split_whitespace();
                let (method, path) = (parts.next().unwrap_or("").to_owned(), parts.next().unwrap_or("").to_owned());
                let (mut len, mut auth) = (0usize, String::new());
                loop {
                    let mut h = String::new();
                    r.read_line(&mut h).unwrap();
                    if h.trim().is_empty() {
                        break;
                    }
                    let low = h.to_lowercase();
                    if let Some(v) = low.strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap_or(0);
                    }
                    if low.starts_with("authorization:") {
                        auth = h.trim().to_owned();
                    }
                }
                let mut body = vec![0u8; len];
                r.read_exact(&mut body).unwrap();
                let body = String::from_utf8(body).unwrap();
                log.lock().unwrap().push((method.clone(), path.clone(), format!("{auth}|{body}")));
                let (code, out) = route(&method, &path, &body);
                let _ = write!(s, "HTTP/1.1 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{out}", out.len());
            }
        });
        (url, seen)
    }

    #[test]
    fn nodes_round_trip_through_the_api_json() {
        let n = Node::new("prefix.100.&c[Admin]", true, 0, ContextSet::of(&[("server", "lobby"), ("world", "nether")]));
        let j = node_json(&n);
        assert_eq!(j["type"], "prefix");
        assert_eq!(j["context"][0], json!({"key": "server", "value": "lobby"}));
        assert!(j.get("expiry").is_none());
        assert_eq!(node_from_json(&j).unwrap(), n);
        let t = Node::new("temp.node", false, 1_999_999_999, ContextSet::new());
        let back = node_from_json(&node_json(&t)).unwrap();
        assert_eq!((back.value, back.expiry), (false, 1_999_999_999));
    }

    #[test]
    fn a_url_that_is_no_http_url_is_refused() {
        assert!(RestStorage::new("ftp://x/", "").is_err());
    }

    #[test]
    fn the_storage_talks_to_the_api() {
        let alice = "00000000-0000-4000-8000-0000000000a1";
        let (url, seen) = serve(move |m, p, _| match (m, p) {
            ("GET", "/health") => (200, r#"{"health":true,"details":{}}"#.into()),
            ("GET", "/group/default") => (200, r#"{"name":"default","nodes":[],"metadata":{"meta":{}}}"#.into()),
            ("GET", "/group") => (200, r#"["default","admin"]"#.into()),
            ("GET", "/group/admin") => (200, r#"{"name":"admin","nodes":[{"key":"a.b","type":"permission","value":true,"context":[]}],"metadata":{"meta":{}}}"#.into()),
            ("GET", "/user/00000000-0000-4000-8000-0000000000a1") => (
                200,
                r#"{"uniqueId":"00000000-0000-4000-8000-0000000000a1","username":"Alice","nodes":[{"key":"group.admin","type":"inheritance","value":true,"context":[]}],"metadata":{"meta":{},"primaryGroup":"admin"}}"#.into(),
            ),
            ("GET", "/user/00000000-0000-4000-8000-0000000000b2") => (404, String::new()),
            ("POST", "/user") => (201, r#"{"outcomes":["username_updated","other_unique_ids_present_for_username"],"previousUsername":"OldAlice","otherUniqueIds":["00000000-0000-4000-8000-0000000000c3"]}"#.into()),
            ("GET", "/user/lookup?username=alice") => (200, format!(r#"{{"uniqueId":"{alice}","username":"Alice"}}"#)),
            ("GET", "/user/lookup?username=nobody") => (404, String::new()),
            ("GET", "/user/search?keyStartsWith=a.") => (200, format!(r#"[{{"uniqueId":"{alice}","results":[{{"key":"a.b","type":"permission","value":true,"context":[]}}]}}]"#)),
            ("GET", "/group/search?keyStartsWith=a.") => (200, r#"[{"name":"admin","results":[{"key":"a.b","type":"permission","value":true,"context":[]}]}]"#.into()),
            ("GET", "/track") => (200, r#"["staff"]"#.into()),
            ("GET", "/track/staff") => (200, r#"{"name":"staff","groups":["default","admin"]}"#.into()),
            ("GET", "/action?user=00000000-0000-4000-8000-0000000000a1") => (
                200,
                format!(r#"{{"entries":[{{"timestamp":10,"source":{{"uniqueId":"{alice}","name":"Alice"}},"target":{{"uniqueId":"{alice}","name":"Alice","type":"user"}},"description":"old"}},{{"timestamp":20,"source":{{"uniqueId":"{alice}","name":"Alice"}},"target":{{"name":"admin","type":"group"}},"description":"new"}}],"overallSize":2}}"#),
            ),
            ("PATCH", _) | ("DELETE", _) | ("POST", _) => (200, "[]".into()),
            _ => (500, String::new()),
        });
        let mut s = RestStorage::new(&url, "sekret").unwrap();
        s.init().unwrap();
        assert_eq!(s.load_all_groups().unwrap().iter().map(|g| (g.name.as_str(), g.data.nodes.len())).collect::<Vec<_>>(), [("default", 0), ("admin", 1)]);
        let id = uuid::parse(alice).unwrap();
        let (nodes, row) = s.load_user(id).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(row, Some(PlayerRow { username: "alice".into(), primary_group: "admin".into() }));
        assert_eq!(s.load_user(uuid::parse("00000000-0000-4000-8000-0000000000b2").unwrap()).unwrap(), (vec![], None));
        let r = s.save_player_data(id, "Alice").unwrap();
        assert_eq!((r.username_changed.as_deref(), r.other_uuids.len(), r.new_player), (Some("oldalice"), 1, false));
        assert_eq!(s.uuid_of("alice").unwrap(), Some(id));
        assert_eq!(s.uuid_of("nobody").unwrap(), None);
        let found = s.search(&KeyMatch::Like("a.%".into())).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|(h, _)| *h == HolderKey::User(id)) && found.iter().any(|(h, _)| *h == HolderKey::Group("admin".into())));
        assert!(s.search(&KeyMatch::NotEqual("x".into())).is_err());
        assert_eq!(s.load_all_tracks().unwrap(), vec![Track { name: "staff".into(), groups: vec!["default".into(), "admin".into()] }]);
        let acts = s.actions(10, Some((TargetType::User, alice))).unwrap();
        assert_eq!(acts.iter().map(|a| a.description.as_str()).collect::<Vec<_>>(), ["new", "old"], "newest first");
        assert_eq!(acts[0].target_type, TargetType::Group);

        // Writes: a user's change goes out as DELETE and PATCH of /nodes with the Bearer key.
        let mut u = User::new(id);
        u.username = Some("Alice".into());
        u.data.nodes = vec![Node::permanent("group.admin", true, ContextSet::new())];
        u.primary_group = "admin".into();
        u.saved = vec![Node::permanent("old.node", true, ContextSet::new())];
        s.save_user(&u).unwrap();
        s.log_action(&acts[0]).unwrap();
        let log = seen.lock().unwrap().clone();
        let find = |m: &str, p: &str| log.iter().find(|(lm, lp, _)| lm == m && lp.starts_with(p)).cloned().unwrap_or_else(|| panic!("no {m} {p}"));
        let del = find("DELETE", &format!("/user/{alice}/nodes"));
        assert!(del.2.starts_with("authorization: Bearer sekret") || del.2.starts_with("Authorization: Bearer sekret"), "{}", del.2);
        assert!(del.2.contains("old.node"));
        assert!(find("PATCH", &format!("/user/{alice}/nodes?temporaryNodeMergeStrategy=none")).2.contains("group.admin"));
        assert!(find("PATCH", &format!("/user/{alice}")).2.contains("Alice"));
        let posted = find("POST", "/action").2;
        assert!(posted.contains("\"type\":\"group\"") && posted.contains("\"description\":\"new\""));
    }

    #[test]
    fn a_default_only_user_is_not_exported() {
        let (url, seen) = serve(|_, _, _| (200, "[]".into()));
        let mut s = RestStorage::new(&url, "").unwrap();
        let mut u = User::new(7);
        u.give_default_if_needed();
        s.save_user(&u).unwrap();
        assert!(seen.lock().unwrap().is_empty(), "nothing to say about a user with nothing but default");
    }
}
