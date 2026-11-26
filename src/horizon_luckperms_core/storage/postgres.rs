//! `storage-method: postgresql`: LuckPerms' PostgreSQL schema (`schema/postgresql.sql`) and statements, so this
//! server shares tables with LuckPerms on Paper and Velocity. Uses the blocking `postgres` client that
//! `horizon-mod-deps` lists (no TLS).

use super::rows::{node_columns, read_node};
use super::{diff, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::config;
use crate::horizon_luckperms_core::model::{Group, Track, User, DEFAULT_GROUP};
use crate::horizon_luckperms_core::node::Node;
use crate::horizon_luckperms_core::uuid;
use postgres::types::ToSql;
use postgres::{Client, NoTls, Row};

pub struct PostgresStorage {
    client: Client,
    prefix: String,
}

fn err(e: impl std::fmt::Display) -> String {
    // The driver's errors never carry the password.
    format!("database: {e}")
}

type P<'a> = &'a [&'a (dyn ToSql + Sync)];

const SCHEMA: &[&str] = &[
    "CREATE TABLE \"{prefix}user_permissions\" (\"id\" SERIAL PRIMARY KEY, \"uuid\" VARCHAR(36) NOT NULL, \"permission\" VARCHAR(200) NOT NULL, \"value\" BOOL NOT NULL, \"server\" VARCHAR(36) NOT NULL, \"world\" VARCHAR(64) NOT NULL, \"expiry\" BIGINT NOT NULL, \"contexts\" VARCHAR(200) NOT NULL)",
    "CREATE INDEX \"{prefix}user_permissions_uuid\" ON \"{prefix}user_permissions\" (\"uuid\")",
    "CREATE TABLE \"{prefix}group_permissions\" (\"id\" SERIAL PRIMARY KEY, \"name\" VARCHAR(36) NOT NULL, \"permission\" VARCHAR(200) NOT NULL, \"value\" BOOL NOT NULL, \"server\" VARCHAR(36) NOT NULL, \"world\" VARCHAR(64) NOT NULL, \"expiry\" BIGINT NOT NULL, \"contexts\" VARCHAR(200) NOT NULL)",
    "CREATE INDEX \"{prefix}group_permissions_name\" ON \"{prefix}group_permissions\" (\"name\")",
    "CREATE TABLE \"{prefix}players\" (\"uuid\" VARCHAR(36) NOT NULL PRIMARY KEY, \"username\" VARCHAR(16) NOT NULL, \"primary_group\" VARCHAR(36) NOT NULL)",
    "CREATE INDEX \"{prefix}players_username\" ON \"{prefix}players\" (\"username\")",
    "CREATE TABLE \"{prefix}groups\" (\"name\" VARCHAR(36) NOT NULL PRIMARY KEY)",
    "CREATE TABLE \"{prefix}actions\" (\"id\" SERIAL PRIMARY KEY, \"time\" BIGINT NOT NULL, \"actor_uuid\" VARCHAR(36) NOT NULL, \"actor_name\" VARCHAR(100) NOT NULL, \"type\" CHAR(1) NOT NULL, \"acted_uuid\" VARCHAR(36) NOT NULL, \"acted_name\" VARCHAR(36) NOT NULL, \"action\" VARCHAR(300) NOT NULL)",
    "CREATE TABLE \"{prefix}tracks\" (\"name\" VARCHAR(36) NOT NULL PRIMARY KEY, \"groups\" TEXT NOT NULL)",
];

impl PostgresStorage {
    /// Connects now (the driver has no lazy pool); `data.address` is `host[:port]` (default 5432).
    pub fn new(cfg: &config::Storage) -> Result<Self> {
        let (host, port) = match cfg.address.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), p.parse::<u16>().map_err(|_| "storage.address: bad port".to_string())?),
            None => (cfg.address.clone(), 5432),
        };
        let mut c = Client::configure();
        c.host(&host).port(port).user(&cfg.username).dbname(&cfg.database).connect_timeout(std::time::Duration::from_millis(cfg.connection_timeout_ms.max(1000)));
        if !cfg.password.is_empty() {
            c.password(&cfg.password);
        }
        Ok(PostgresStorage { client: c.connect(NoTls).map_err(err)?, prefix: cfg.table_prefix.clone() })
    }

    fn q(&self, s: &str) -> String {
        s.replace("{prefix}", &self.prefix)
    }

    fn exec(&mut self, sql: &str, p: P) -> Result<u64> {
        let q = self.q(sql);
        self.client.execute(q.as_str(), p).map_err(err)
    }

    fn query(&mut self, sql: &str, p: P) -> Result<Vec<Row>> {
        let q = self.q(sql);
        self.client.query(q.as_str(), p).map_err(err)
    }

    fn node_of(r: &Row, o: usize) -> Option<Node> {
        read_node(r.get::<_, i32>(o) as i64, r.get(o + 1), r.get(o + 2), r.get(o + 3), r.get(o + 4), r.get(o + 5), r.get(o + 6))
    }

    fn update_permissions(&mut self, table: &str, col: &str, holder: &str, added: &[Node], removed: &[Node]) -> Result<()> {
        for n in removed {
            if let Some(id) = n.row_id {
                self.exec(&format!("DELETE FROM \"{{prefix}}{table}\" WHERE \"id\"=$1"), &[&(id as i32)])?;
            } else {
                let (p, v, s, w, e, x) = node_columns(n);
                self.exec(
                    &format!("DELETE FROM \"{{prefix}}{table}\" WHERE \"{col}\"=$1 AND \"permission\"=$2 AND \"value\"=$3 AND \"server\"=$4 AND \"world\"=$5 AND \"expiry\"=$6 AND \"contexts\"=$7"),
                    &[&holder, &p, &v, &s, &w, &e, &x],
                )?;
            }
        }
        for n in added {
            let (p, v, s, w, e, x) = node_columns(n);
            self.exec(
                &format!("INSERT INTO \"{{prefix}}{table}\" (\"{col}\", \"permission\", \"value\", \"server\", \"world\", \"expiry\", \"contexts\") VALUES($1, $2, $3, $4, $5, $6, $7)"),
                &[&holder, &p, &v, &s, &w, &e, &x],
            )?;
        }
        Ok(())
    }

    fn select_nodes(&mut self, table: &str, col: &str, holder: &str) -> Result<Vec<Node>> {
        let rows = self.query(&format!("SELECT \"id\", \"permission\", \"value\", \"server\", \"world\", \"expiry\", \"contexts\" FROM \"{{prefix}}{table}\" WHERE \"{col}\"=$1"), &[&holder])?;
        Ok(rows.iter().filter_map(|r| Self::node_of(r, 0)).collect())
    }
}

impl Storage for PostgresStorage {
    fn name(&self) -> &'static str {
        "PostgreSQL"
    }

    fn init(&mut self) -> Result<()> {
        let first = format!("{}user_permissions", self.prefix);
        let exists = !self
            .client
            .query("SELECT 1 FROM information_schema.tables WHERE table_schema = current_schema() AND table_name = $1", &[&first])
            .map_err(err)?
            .is_empty();
        if !exists {
            for s in SCHEMA {
                let q = self.q(s);
                self.client.batch_execute(&q).map_err(err)?;
            }
        }
        self.exec("INSERT INTO \"{prefix}groups\" (\"name\") VALUES($1) ON CONFLICT DO NOTHING", &[&DEFAULT_GROUP])?;
        Ok(())
    }

    fn load_user(&mut self, id: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let id = uuid::format(id);
        let nodes = self.select_nodes("user_permissions", "uuid", &id)?;
        let rows = self.query("SELECT \"username\", \"primary_group\" FROM \"{prefix}players\" WHERE \"uuid\"=$1 LIMIT 1", &[&id])?;
        Ok((nodes, rows.first().map(|r| PlayerRow { username: r.get(0), primary_group: r.get(1) })))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let id = uuid::format(user.uuid);
        if !user.is_non_default() {
            self.exec("DELETE FROM \"{prefix}user_permissions\" WHERE \"uuid\"=$1", &[&id])?;
            self.exec("UPDATE \"{prefix}players\" SET \"primary_group\"=$1 WHERE \"uuid\"=$2", &[&DEFAULT_GROUP, &id])?;
            return Ok(());
        }
        let (added, removed) = diff(&user.saved, &user.data.nodes);
        self.update_permissions("user_permissions", "uuid", &id, &added, &removed)?;
        let primary = if user.primary_group.is_empty() { DEFAULT_GROUP.to_owned() } else { user.primary_group.clone() };
        let existing = self.query("SELECT \"primary_group\" FROM \"{prefix}players\" WHERE \"uuid\"=$1 LIMIT 1", &[&id])?;
        match existing.first().map(|r| r.get::<_, String>(0)) {
            Some(p) if p == primary => {}
            Some(_) => {
                self.exec("UPDATE \"{prefix}players\" SET \"primary_group\"=$1 WHERE \"uuid\"=$2", &[&primary, &id])?;
            }
            None => {
                let name = user.username.clone().unwrap_or_else(|| "null".into()).to_lowercase();
                self.exec("INSERT INTO \"{prefix}players\" (\"uuid\", \"username\", \"primary_group\") VALUES($1, $2, $3)", &[&id, &name, &primary])?;
            }
        }
        Ok(())
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        let names = self.query("SELECT \"name\" FROM \"{prefix}groups\"", &[])?;
        let mut groups: std::collections::BTreeMap<String, Group> = names.iter().map(|r| r.get::<_, String>(0)).map(|n| (n.to_lowercase(), Group::new(&n))).collect();
        let rows = self.query("SELECT \"name\", \"id\", \"permission\", \"value\", \"server\", \"world\", \"expiry\", \"contexts\" FROM \"{prefix}group_permissions\"", &[])?;
        for r in &rows {
            let name: String = r.get(0);
            if let (Some(g), Some(n)) = (groups.get_mut(&name.to_lowercase()), Self::node_of(r, 1)) {
                g.data.nodes.push(n);
            }
        }
        Ok(groups.into_values().collect())
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        if self.query("SELECT \"name\" FROM \"{prefix}groups\" WHERE \"name\"=$1 LIMIT 1", &[&name])?.is_empty() {
            return Ok(None);
        }
        let mut g = Group::new(name);
        g.data.nodes = self.select_nodes("group_permissions", "name", name)?;
        Ok(Some(g))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        self.exec("INSERT INTO \"{prefix}groups\" (\"name\") VALUES($1) ON CONFLICT DO NOTHING", &[&name]).map(|_| ())
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        let (added, removed) = diff(before, &group.data.nodes);
        self.update_permissions("group_permissions", "name", &group.name, &added, &removed)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.exec("DELETE FROM \"{prefix}group_permissions\" WHERE \"name\"=$1", &[&name])?;
        self.exec("DELETE FROM \"{prefix}groups\" WHERE \"name\"=$1", &[&name]).map(|_| ())
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        Ok(self.query("SELECT \"name\", \"groups\" FROM \"{prefix}tracks\"", &[])?.iter().map(|r| Track::from_json(&r.get::<_, String>(0), &r.get::<_, String>(1))).collect())
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        let g = track.groups_json();
        self.exec("INSERT INTO \"{prefix}tracks\" (\"name\", \"groups\") VALUES($1, $2) ON CONFLICT (\"name\") DO UPDATE SET \"groups\"=EXCLUDED.\"groups\"", &[&track.name, &g]).map(|_| ())
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.exec("DELETE FROM \"{prefix}tracks\" WHERE \"name\"=$1", &[&name]).map(|_| ())
    }

    fn save_player_data(&mut self, id: u128, username: &str) -> Result<PlayerSave> {
        let id = uuid::format(id);
        let name = username.to_lowercase();
        let mut r = PlayerSave::default();
        let current = self.query("SELECT \"username\" FROM \"{prefix}players\" WHERE \"uuid\"=$1 LIMIT 1", &[&id])?.first().map(|x| x.get::<_, String>(0));
        match current {
            None => {
                self.exec("INSERT INTO \"{prefix}players\" (\"uuid\", \"username\", \"primary_group\") VALUES($1, $2, $3)", &[&id, &name, &DEFAULT_GROUP])?;
                r.new_player = true;
            }
            Some(old) if old != name => {
                self.exec("UPDATE \"{prefix}players\" SET \"username\"=$1 WHERE \"uuid\"=$2", &[&name, &id])?;
                r.username_changed = Some(old);
            }
            _ => {}
        }
        let others: Vec<String> = self.query("SELECT \"uuid\" FROM \"{prefix}players\" WHERE \"username\"=$1 AND NOT \"uuid\"=$2", &[&name, &id])?.iter().map(|x| x.get(0)).collect();
        if !others.is_empty() {
            self.exec("DELETE FROM \"{prefix}players\" WHERE \"username\"=$1 AND NOT \"uuid\"=$2", &[&name, &id])?;
            r.other_uuids = others.iter().filter_map(|u| uuid::parse(u)).collect();
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        let n = username.to_lowercase();
        Ok(self.query("SELECT \"uuid\" FROM \"{prefix}players\" WHERE \"username\"=$1 LIMIT 1", &[&n])?.first().and_then(|r| uuid::parse(&r.get::<_, String>(0))))
    }

    fn name_of(&mut self, id: u128) -> Result<Option<String>> {
        let id = uuid::format(id);
        Ok(self.query("SELECT \"username\" FROM \"{prefix}players\" WHERE \"uuid\"=$1 LIMIT 1", &[&id])?.first().map(|r| r.get::<_, String>(0)).filter(|n| n != "null"))
    }

    fn log_action(&mut self, a: &Action) -> Result<()> {
        let (src, tgt) = (uuid::format(a.source_uuid), a.target_uuid.map(uuid::format).unwrap_or_else(|| "null".into()));
        let ty = a.target_type.code().to_string();
        self.exec(
            "INSERT INTO \"{prefix}actions\" (\"time\", \"actor_uuid\", \"actor_name\", \"type\", \"acted_uuid\", \"acted_name\", \"action\") VALUES($1, $2, $3, $4, $5, $6, $7)",
            &[&a.timestamp, &src, &a.source_name, &ty, &tgt, &a.target_name, &a.description],
        )
        .map(|_| ())
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let base = "SELECT \"time\", \"actor_uuid\", \"actor_name\", \"type\", \"acted_uuid\", \"acted_name\", \"action\" FROM \"{prefix}actions\"";
        let lim = limit.min(i64::MAX as usize) as i64;
        let rows = match target {
            None => self.query(&format!("{base} ORDER BY \"time\" DESC, \"id\" DESC LIMIT $1"), &[&lim])?,
            Some((t, name)) => {
                let code = t.code().to_string();
                self.query(&format!("{base} WHERE \"type\"=$1 AND (\"acted_name\"=$2 OR \"acted_uuid\"=$2) ORDER BY \"time\" DESC, \"id\" DESC LIMIT $3"), &[&code, &name, &lim])?
            }
        };
        Ok(rows
            .iter()
            .filter_map(|r| {
                let (tu, ty): (String, String) = (r.get(4), r.get(3));
                Some(Action {
                    timestamp: r.get(0),
                    source_uuid: uuid::parse(&r.get::<_, String>(1))?,
                    source_name: r.get(2),
                    target_type: TargetType::parse(&ty)?,
                    target_uuid: if tu == "null" { None } else { uuid::parse(&tu) },
                    target_name: r.get(5),
                    description: r.get(6),
                })
            })
            .collect())
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        let (cond, arg) = match key {
            KeyMatch::Exact(k) => ("\"permission\" = $1", k.clone()),
            KeyMatch::NotEqual(k) => ("\"permission\" <> $1", k.clone()),
            KeyMatch::Like(p) => ("\"permission\" LIKE $1", p.clone()),
            KeyMatch::NotLike(p) => ("\"permission\" NOT LIKE $1", p.clone()),
        };
        let mut out = Vec::new();
        for r in &self.query(&format!("SELECT \"uuid\", \"id\", \"permission\", \"value\", \"server\", \"world\", \"expiry\", \"contexts\" FROM \"{{prefix}}user_permissions\" WHERE {cond}"), &[&arg])? {
            if let (Some(u), Some(n)) = (uuid::parse(&r.get::<_, String>(0)), Self::node_of(r, 1)) {
                out.push((HolderKey::User(u), n));
            }
        }
        for r in &self.query(&format!("SELECT \"name\", \"id\", \"permission\", \"value\", \"server\", \"world\", \"expiry\", \"contexts\" FROM \"{{prefix}}group_permissions\" WHERE {cond}"), &[&arg])? {
            if let Some(n) = Self::node_of(r, 1) {
                out.push((HolderKey::Group(r.get::<_, String>(0).to_lowercase()), n));
            }
        }
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        let rows = self.query("SELECT \"uuid\" FROM \"{prefix}players\" UNION SELECT \"uuid\" FROM \"{prefix}user_permissions\"", &[])?;
        let mut ids: Vec<u128> = rows.iter().filter_map(|r| uuid::parse(&r.get::<_, String>(0))).collect();
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

/// A fresh connection for the LISTEN/NOTIFY messenger (it holds its own).
pub fn connect(cfg: &config::Storage) -> Result<Client> {
    Ok(PostgresStorage::new(cfg)?.client)
}
