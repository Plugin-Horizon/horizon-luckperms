//! `storage-method: mongodb`: LuckPerms' `MongoStorage` document layout (collections `<prefix>users`, `groups`,
//! `tracks`, `uuid` and `action`; ids are legacy-Java UUID binaries), on the official driver's blocking API.
//! Uses the `mongodb` 3 and `bson` 3 crates that
//! `horizon-mod-deps` lists.

use super::{diff, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::config;
use crate::horizon_luckperms_core::context::ContextSet;
use crate::horizon_luckperms_core::model::{Group, Track, User, DEFAULT_GROUP};
use crate::horizon_luckperms_core::node::Node;
use crate::horizon_luckperms_core::uuid;
use mongodb::bson::spec::BinarySubtype;
use mongodb::bson::{doc, Binary, Bson, Document};
use mongodb::sync::{Client, Database};
use mongodb::options::ReplaceOptions;

pub struct MongoStorage {
    _client: Client,
    db: Database,
    prefix: String,
}

fn err(e: impl std::fmt::Display) -> String {
    // Connection errors name the host, never the credentials.
    format!("database: {e}")
}

/// A UUID as `UuidRepresentation.JAVA_LEGACY` stores it: subtype 3, each half of the bytes reversed.
pub fn uuid_bson(id: u128) -> Bson {
    let b = id.to_be_bytes();
    let mut out = [0u8; 16];
    for i in 0..8 {
        out[i] = b[7 - i];
        out[8 + i] = b[15 - i];
    }
    Bson::Binary(Binary { subtype: BinarySubtype::UuidOld, bytes: out.to_vec() })
}

/// A document id: legacy binary, standard binary, or the text form.
pub fn uuid_from(v: &Bson) -> Option<u128> {
    match v {
        Bson::String(s) => uuid::parse(s),
        Bson::Binary(b) if b.bytes.len() == 16 => {
            let mut x = [0u8; 16];
            if b.subtype == BinarySubtype::UuidOld {
                for i in 0..8 {
                    x[i] = b.bytes[7 - i];
                    x[8 + i] = b.bytes[15 - i];
                }
            } else {
                x.copy_from_slice(&b.bytes);
            }
            Some(u128::from_be_bytes(x))
        }
        _ => None,
    }
}

pub fn node_doc(n: &Node) -> Document {
    let mut d = doc! {"key": &n.key, "value": n.value};
    if n.has_expiry() {
        d.insert("expiry", n.expiry);
    }
    if !n.contexts.is_empty() {
        let ctx: Vec<Document> = n.contexts.ordered_pairs().into_iter().map(|(k, v)| doc! {"key": k, "value": v}).collect();
        d.insert("context", ctx);
    }
    d
}

pub fn node_from(d: &Document) -> Option<Node> {
    let key = d.get_str("permission").or_else(|_| d.get_str("key")).ok().filter(|k| !k.is_empty())?;
    let mut ctx = ContextSet::new();
    if let Ok(s) = d.get_str("server") {
        ctx.add("server", s);
    }
    if let Ok(w) = d.get_str("world") {
        ctx.add("world", w);
    }
    for c in d.get_array("context").into_iter().flatten() {
        if let Bson::Document(c) = c {
            if let (Ok(k), Ok(v)) = (c.get_str("key"), c.get_str("value")) {
                ctx.add(k, v);
            }
        }
    }
    let expiry = d.get("expiry").and_then(|e| e.as_i64().or_else(|| e.as_i32().map(i64::from))).unwrap_or(0);
    Some(Node::new(key, d.get_bool("value").unwrap_or(true), expiry, ctx))
}

fn nodes_of(d: &Document) -> Vec<Node> {
    d.get_array("permissions").into_iter().flatten().filter_map(|b| if let Bson::Document(x) = b { node_from(x) } else { None }).collect()
}

impl MongoStorage {
    pub fn new(cfg: &config::Storage) -> Result<Self> {
        let client;
        let db_name;
        if !cfg.mongodb_connection_uri.is_empty() {
            client = Client::with_uri_str(&cfg.mongodb_connection_uri).map_err(err)?;
            db_name = cfg.database.clone();
        } else {
            let addr = if cfg.address.contains(':') { cfg.address.clone() } else { format!("{}:27017", cfg.address) };
            let auth = if cfg.username.is_empty() { String::new() } else { format!("{}:{}@", cfg.username, cfg.password) };
            client = Client::with_uri_str(format!("mongodb://{auth}{addr}/?authSource={}", cfg.database)).map_err(err)?;
            db_name = cfg.database.clone();
        }
        let db = client.database(&db_name);
        let m = MongoStorage { _client: client, db, prefix: cfg.mongodb_collection_prefix.clone() };
        Ok(m)
    }

    fn col(&self, name: &str) -> mongodb::sync::Collection<Document> {
        self.db.collection(&format!("{}{name}", self.prefix))
    }

    fn all(&self, name: &str, filter: Document) -> Result<Vec<Document>> {
        self.col(name).find(filter).run().map_err(err)?.collect::<std::result::Result<Vec<_>, _>>().map_err(err)
    }

    fn push_changes(&self, coll: &str, id: Bson, added: &[Node], removed: &[Node], extra: Option<Document>) -> Result<()> {
        let c = self.col(coll);
        let f = doc! {"_id": id};
        if !removed.is_empty() {
            c.update_one(f.clone(), doc! {"$pullAll": {"permissions": removed.iter().map(node_doc).collect::<Vec<_>>()}}).upsert(true).run().map_err(err)?;
        }
        if !added.is_empty() {
            c.update_one(f.clone(), doc! {"$push": {"permissions": {"$each": added.iter().map(node_doc).collect::<Vec<_>>()}}}).upsert(true).run().map_err(err)?;
        }
        if let Some(set) = extra {
            c.update_one(f, doc! {"$set": set}).upsert(true).run().map_err(err)?;
        }
        Ok(())
    }
}

impl Storage for MongoStorage {
    fn name(&self) -> &'static str {
        "MongoDB"
    }

    fn init(&mut self) -> Result<()> {
        use mongodb::IndexModel;
        // `ensureIndexes`: failures are ignored, as in the original.
        let _ = self.col("uuid").create_index(IndexModel::builder().keys(doc! {"name": 1}).build()).run();
        let _ = self.col("users").create_index(IndexModel::builder().keys(doc! {"permissions.key": 1}).build()).run();
        let _ = self.col("groups").create_index(IndexModel::builder().keys(doc! {"permissions.key": 1}).build()).run();
        if self.col("groups").find_one(doc! {"_id": DEFAULT_GROUP}).run().map_err(err)?.is_none() {
            self.col("groups").insert_one(doc! {"_id": DEFAULT_GROUP}).run().map_err(err)?;
        }
        Ok(())
    }

    fn load_user(&mut self, id: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let Some(d) = self.col("users").find_one(doc! {"_id": uuid_bson(id)}).run().map_err(err)? else { return Ok((Vec::new(), None)) };
        let row = PlayerRow {
            username: d.get_str("name").unwrap_or("null").to_owned(),
            primary_group: d.get_str("primaryGroup").unwrap_or(DEFAULT_GROUP).to_owned(),
        };
        Ok((nodes_of(&d), Some(row)))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let id = uuid_bson(user.uuid);
        if !user.is_non_default() {
            self.col("users").delete_one(doc! {"_id": id}).run().map_err(err)?;
            return Ok(());
        }
        let (added, removed) = diff(&user.saved, &user.data.nodes);
        let set = doc! {
            "name": user.username.clone().unwrap_or_else(|| "null".into()),
            "primaryGroup": if user.primary_group.is_empty() { DEFAULT_GROUP } else { user.primary_group.as_str() },
        };
        self.push_changes("users", id, &added, &removed, Some(set))
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        Ok(self
            .all("groups", doc! {})?
            .iter()
            .filter_map(|d| {
                let mut g = Group::new(d.get_str("_id").ok()?);
                g.data.nodes = nodes_of(d);
                Some(g)
            })
            .collect())
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        Ok(self.col("groups").find_one(doc! {"_id": name}).run().map_err(err)?.map(|d| {
            let mut g = Group::new(name);
            g.data.nodes = nodes_of(&d);
            g
        }))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        if self.col("groups").find_one(doc! {"_id": name}).run().map_err(err)?.is_none() {
            self.col("groups").insert_one(doc! {"_id": name}).run().map_err(err)?;
        }
        Ok(())
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        let (added, removed) = diff(before, &group.data.nodes);
        self.push_changes("groups", Bson::String(group.name.clone()), &added, &removed, None)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.col("groups").delete_one(doc! {"_id": name}).run().map(|_| ()).map_err(err)
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        Ok(self
            .all("tracks", doc! {})?
            .iter()
            .filter_map(|d| {
                let groups = d.get_array("groups").into_iter().flatten().filter_map(|g| g.as_str().map(str::to_owned)).collect();
                Some(Track { name: d.get_str("_id").ok()?.to_owned(), groups })
            })
            .collect())
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        let d = doc! {"_id": &track.name, "groups": &track.groups};
        self.col("tracks").replace_one(doc! {"_id": &track.name}, d).with_options(ReplaceOptions::builder().upsert(true).build()).run().map(|_| ()).map_err(err)
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.col("tracks").delete_one(doc! {"_id": name}).run().map(|_| ()).map_err(err)
    }

    fn save_player_data(&mut self, id: u128, username: &str) -> Result<PlayerSave> {
        let name = username.to_lowercase();
        let old = self.name_of(id)?;
        let mut r = PlayerSave::default();
        if old.as_deref() != Some(name.as_str()) {
            let d = doc! {"_id": uuid_bson(id), "name": &name};
            self.col("uuid").replace_one(doc! {"_id": uuid_bson(id)}, d).with_options(ReplaceOptions::builder().upsert(true).build()).run().map_err(err)?;
        }
        match old {
            None => r.new_player = true,
            Some(o) if o != name => r.username_changed = Some(o),
            _ => {}
        }
        let others: Vec<u128> = self.all("uuid", doc! {"name": &name})?.iter().filter_map(|d| uuid_from(d.get("_id")?)).filter(|u| *u != id).collect();
        if !others.is_empty() {
            self.col("uuid").delete_many(doc! {"_id": {"$in": others.iter().map(|u| uuid_bson(*u)).collect::<Vec<_>>()}}).run().map_err(err)?;
            r.other_uuids = others;
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        Ok(self.col("uuid").find_one(doc! {"name": username.to_lowercase()}).run().map_err(err)?.and_then(|d| uuid_from(d.get("_id")?)))
    }

    fn name_of(&mut self, id: u128) -> Result<Option<String>> {
        Ok(self.col("uuid").find_one(doc! {"_id": uuid_bson(id)}).run().map_err(err)?.and_then(|d| d.get_str("name").ok().map(str::to_owned)).filter(|n| n != "null"))
    }

    fn log_action(&mut self, a: &Action) -> Result<()> {
        let mut target = doc! {"type": a.target_type.name(), "name": &a.target_name};
        if let Some(u) = a.target_uuid {
            target.insert("uniqueId", uuid_bson(u));
        }
        let d = doc! {"timestamp": a.timestamp, "source": {"uniqueId": uuid_bson(a.source_uuid), "name": &a.source_name}, "target": target, "description": &a.description};
        self.col("action").insert_one(d).run().map(|_| ()).map_err(err)
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let filter = match target {
            None => doc! {},
            Some((t, n)) => {
                let mut by = vec![doc! {"target.name": n}];
                if let Some(u) = uuid::parse(n) {
                    by.push(doc! {"target.uniqueId": uuid_bson(u)});
                }
                doc! {"target.type": t.name(), "$or": by}
            }
        };
        let mut all: Vec<Action> = self
            .all("action", filter)?
            .iter()
            .filter_map(|d| {
                let (s, t) = (d.get_document("source").ok()?, d.get_document("target").ok()?);
                Some(Action {
                    timestamp: d.get("timestamp").and_then(|v| v.as_i64().or_else(|| v.as_i32().map(i64::from)))?,
                    source_uuid: uuid_from(s.get("uniqueId")?)?,
                    source_name: s.get_str("name").ok()?.to_owned(),
                    target_type: TargetType::parse(t.get_str("type").ok()?)?,
                    target_uuid: t.get("uniqueId").and_then(uuid_from),
                    target_name: t.get_str("name").ok()?.to_owned(),
                    description: d.get_str("description").ok()?.to_owned(),
                })
            })
            .collect();
        all.sort_by_key(|a| std::cmp::Reverse(a.timestamp));
        all.truncate(limit);
        Ok(all)
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        let mut out = Vec::new();
        for d in self.all("users", doc! {})? {
            if let Some(u) = d.get("_id").and_then(uuid_from) {
                out.extend(nodes_of(&d).into_iter().filter(|n| key.matches(&n.key)).map(|n| (HolderKey::User(u), n)));
            }
        }
        for d in self.all("groups", doc! {})? {
            if let Ok(g) = d.get_str("_id") {
                out.extend(nodes_of(&d).into_iter().filter(|n| key.matches(&n.key)).map(|n| (HolderKey::Group(g.to_lowercase()), n)));
            }
        }
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        let mut ids: Vec<u128> = self.all("users", doc! {})?.iter().filter_map(|d| uuid_from(d.get("_id")?)).collect();
        ids.sort_unstable();
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

    #[test]
    fn legacy_uuid_binaries_round_trip() {
        let id = uuid::parse("00112233-4455-6677-8899-aabbccddeeff").unwrap();
        let b = uuid_bson(id);
        if let Bson::Binary(bin) = &b {
            assert_eq!(bin.bytes[0], 0x77, "the first half is reversed");
        }
        assert_eq!(uuid_from(&b), Some(id));
        assert_eq!(uuid_from(&Bson::String("00112233-4455-6677-8899-aabbccddeeff".into())), Some(id));
    }

    #[test]
    fn nodes_round_trip_through_documents() {
        let n = Node::new("prefix.5.&e[X]", true, 1_999_999_999, ContextSet::of(&[("server", "a"), ("world", "b")]));
        assert_eq!(node_from(&node_doc(&n)).unwrap(), n);
        assert!(node_doc(&Node::permanent("a.b", true, ContextSet::new())).get("context").is_none());
    }
}
