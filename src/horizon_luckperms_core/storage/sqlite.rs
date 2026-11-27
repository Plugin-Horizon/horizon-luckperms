//! LuckPerms' SQLite storage: the schema of `schema/sqlite.sql` and the statements of `SqlStorage`, in the file
//! `luckperms-sqlite.db` of the data directory (SQLite bundled into the mod, no system library). The same tables
//! as the MariaDB storage, so a file written by LuckPerms on Paper reads here.

use super::rows::{node_columns, read_node};
use super::{diff, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::model::{Group, Track, User, DEFAULT_GROUP};
use crate::horizon_luckperms_core::node::Node;
use crate::horizon_luckperms_core::uuid;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

pub struct SqliteStorage {
    conn: Connection,
    prefix: String,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("database: {e}")
}

/// The schema of `sqlite.sql`.
const SCHEMA: &[&str] = &[
    "CREATE TABLE `{prefix}user_permissions` (`id` INTEGER PRIMARY KEY NOT NULL, `uuid` VARCHAR(36) NOT NULL, `permission` VARCHAR(200) NOT NULL, `value` BOOL NOT NULL, `server` VARCHAR(36) NOT NULL, `world` VARCHAR(64) NOT NULL, `expiry` BIGINT NOT NULL, `contexts` VARCHAR(200) NOT NULL)",
    "CREATE INDEX `{prefix}user_permissions_uuid` ON `{prefix}user_permissions` (`uuid`)",
    "CREATE TABLE `{prefix}group_permissions` (`id` INTEGER PRIMARY KEY NOT NULL, `name` VARCHAR(36) NOT NULL, `permission` VARCHAR(200) NOT NULL, `value` BOOL NOT NULL, `server` VARCHAR(36) NOT NULL, `world` VARCHAR(64) NOT NULL, `expiry` BIGINT NOT NULL, `contexts` VARCHAR(200) NOT NULL)",
    "CREATE INDEX `{prefix}group_permissions_name` ON `{prefix}group_permissions` (`name`)",
    "CREATE TABLE `{prefix}players` (`uuid` VARCHAR(36) NOT NULL, `username` VARCHAR(16) NOT NULL, `primary_group` VARCHAR(36) NOT NULL, PRIMARY KEY (`uuid`))",
    "CREATE INDEX `{prefix}players_username` ON `{prefix}players` (`username`)",
    "CREATE TABLE `{prefix}groups` (`name` VARCHAR(36) NOT NULL, PRIMARY KEY (`name`))",
    "CREATE TABLE `{prefix}actions` (`id` INTEGER PRIMARY KEY NOT NULL, `time` BIGINT NOT NULL, `actor_uuid` VARCHAR(36) NOT NULL, `actor_name` VARCHAR(100) NOT NULL, `type` CHAR(1) NOT NULL, `acted_uuid` VARCHAR(36) NOT NULL, `acted_name` VARCHAR(36) NOT NULL, `action` VARCHAR(300) NOT NULL)",
    "CREATE TABLE `{prefix}tracks` (`name` VARCHAR(36) NOT NULL, `groups` TEXT NOT NULL, PRIMARY KEY (`name`))",
];

/// A table prefix goes into statements as text: only what a table name may hold is accepted.
fn valid_prefix(p: &str) -> bool {
    p.len() <= 40 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl SqliteStorage {
    /// Opens (and creates) the database file.
    pub fn open(file: &Path, table_prefix: &str) -> Result<Self> {
        if !valid_prefix(table_prefix) {
            return Err("data.table-prefix may only hold letters, digits and underscores".into());
        }
        let conn = Connection::open(file).map_err(err)?;
        Ok(SqliteStorage { conn, prefix: table_prefix.to_owned() })
    }

    /// A private in-memory database (tests).
    #[cfg(test)]
    pub fn in_memory(table_prefix: &str) -> Self {
        SqliteStorage { conn: Connection::open_in_memory().unwrap(), prefix: table_prefix.to_owned() }
    }

    /// `StatementProcessor`: `{prefix}` becomes the table prefix.
    fn q(&self, s: &str) -> String {
        s.replace("{prefix}", &self.prefix)
    }

    fn select_nodes(&self, table: &str, holder_col: &str, holder: &str) -> Result<Vec<Node>> {
        let q = self.q(&format!("SELECT `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{{prefix}}{table}` WHERE `{holder_col}`=?1"));
        let mut st = self.conn.prepare(&q).map_err(err)?;
        let rows = st
            .query_map(params![holder], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, bool>(2)?, r.get::<_, String>(3)?, r.get::<_, String>(4)?, r.get::<_, i64>(5)?, r.get::<_, String>(6)?))
            })
            .map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            let (id, p, v, s, w, e, x) = row.map_err(err)?;
            out.extend(read_node(id, p, v, s, w, e, x));
        }
        Ok(out)
    }

    /// `updatePermissions`: deletes by row id where known, else by every column; then inserts.
    fn update_permissions(&self, table: &str, holder_col: &str, holder: &str, added: &[Node], removed: &[Node]) -> Result<()> {
        for n in removed {
            if let Some(id) = n.row_id {
                self.conn.execute(&self.q(&format!("DELETE FROM `{{prefix}}{table}` WHERE `id`=?1")), params![id]).map_err(err)?;
            } else {
                let (p, v, s, w, e, x) = node_columns(n);
                self.conn
                    .execute(
                        &self.q(&format!("DELETE FROM `{{prefix}}{table}` WHERE `{holder_col}`=?1 AND `permission`=?2 AND `value`=?3 AND `server`=?4 AND `world`=?5 AND `expiry`=?6 AND `contexts`=?7")),
                        params![holder, p, v, s, w, e, x],
                    )
                    .map_err(err)?;
            }
        }
        for n in added {
            let (p, v, s, w, e, x) = node_columns(n);
            self.conn
                .execute(
                    &self.q(&format!("INSERT INTO `{{prefix}}{table}` (`{holder_col}`, `permission`, `value`, `server`, `world`, `expiry`, `contexts`) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)")),
                    params![holder, p, v, s, w, e, x],
                )
                .map_err(err)?;
        }
        Ok(())
    }

    fn one_string(&self, sql: &str, arg: &str) -> Result<Option<String>> {
        self.conn.query_row(&self.q(sql), params![arg], |r| r.get::<_, String>(0)).optional().map_err(err)
    }
}

impl Storage for SqliteStorage {
    fn name(&self) -> &'static str {
        "SQLite"
    }

    fn init(&mut self) -> Result<()> {
        let first = format!("{}user_permissions", self.prefix);
        let exists = self
            .conn
            .query_row("SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1", params![first], |r| r.get::<_, i64>(0))
            .optional()
            .map_err(err)?
            .is_some();
        if !exists {
            // One transaction: a failure half way leaves no half schema behind.
            self.conn.execute_batch("BEGIN").map_err(err)?;
            for s in SCHEMA {
                if let Err(e) = self.conn.execute_batch(&self.q(s)) {
                    let _ = self.conn.execute_batch("ROLLBACK");
                    return Err(err(e));
                }
            }
            self.conn.execute_batch("COMMIT").map_err(err)?;
        }
        self.conn.execute(&self.q("INSERT OR IGNORE INTO `{prefix}groups` (`name`) VALUES(?1)"), params![DEFAULT_GROUP]).map_err(err)?;
        Ok(())
    }

    /// SQLite cannot carry the SQL messenger (LuckPerms only loads it for MariaDB and MySQL).
    fn sql_messenger(&self) -> bool {
        false
    }

    fn load_user(&mut self, id: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let key = uuid::format(id);
        let nodes = self.select_nodes("user_permissions", "uuid", &key)?;
        let row = self
            .conn
            .query_row(&self.q("SELECT `username`, `primary_group` FROM `{prefix}players` WHERE `uuid`=?1 LIMIT 1"), params![key], |r| Ok(PlayerRow { username: r.get(0)?, primary_group: r.get(1)? }))
            .optional()
            .map_err(err)?;
        Ok((nodes, row))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let id = uuid::format(user.uuid);
        if !user.is_non_default() {
            self.conn.execute(&self.q("DELETE FROM `{prefix}user_permissions` WHERE `uuid`=?1"), params![id]).map_err(err)?;
            self.conn.execute(&self.q("UPDATE `{prefix}players` SET `primary_group`=?1 WHERE `uuid`=?2"), params![DEFAULT_GROUP, id]).map_err(err)?;
            return Ok(());
        }
        let (added, removed) = diff(&user.saved, &user.data.nodes);
        self.update_permissions("user_permissions", "uuid", &id, &added, &removed)?;
        let primary = if user.primary_group.is_empty() { DEFAULT_GROUP.to_owned() } else { user.primary_group.clone() };
        match self.one_string("SELECT `primary_group` FROM `{prefix}players` WHERE `uuid`=?1 LIMIT 1", &id)? {
            Some(p) if p == primary => {}
            Some(_) => {
                self.conn.execute(&self.q("UPDATE `{prefix}players` SET `primary_group`=?1 WHERE `uuid`=?2"), params![primary, id]).map_err(err)?;
            }
            None => {
                let name = user.username.clone().unwrap_or_else(|| "null".into()).to_lowercase();
                self.conn.execute(&self.q("INSERT INTO `{prefix}players` (`uuid`, `username`, `primary_group`) VALUES(?1, ?2, ?3)"), params![id, name, primary]).map_err(err)?;
            }
        }
        Ok(())
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        let names: Vec<String> = {
            let mut st = self.conn.prepare(&self.q("SELECT `name` FROM `{prefix}groups`")).map_err(err)?;
            let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(err)?
        };
        let mut groups: std::collections::BTreeMap<String, Group> = names.iter().map(|n| (n.to_lowercase(), Group::new(n))).collect();
        let rows: Vec<(String, Node)> = {
            let mut st = self
                .conn
                .prepare(&self.q("SELECT `name`, `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{prefix}group_permissions`"))
                .map_err(err)?;
            let mapped = st
                .query_map([], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, String>(7)?,
                    ))
                })
                .map_err(err)?;
            let mut v = Vec::new();
            for row in mapped {
                let (name, id, p, val, s, w, e, x) = row.map_err(err)?;
                if let Some(n) = read_node(id, p, val, s, w, e, x) {
                    v.push((name, n));
                }
            }
            v
        };
        for (name, n) in rows {
            if let Some(g) = groups.get_mut(&name.to_lowercase()) {
                g.data.nodes.push(n);
            }
        }
        Ok(groups.into_values().collect())
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        if self.one_string("SELECT `name` FROM `{prefix}groups` WHERE `name`=?1 LIMIT 1", name)?.is_none() {
            return Ok(None);
        }
        let mut g = Group::new(name);
        g.data.nodes = self.select_nodes("group_permissions", "name", name)?;
        Ok(Some(g))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        self.conn.execute(&self.q("INSERT OR IGNORE INTO `{prefix}groups` (`name`) VALUES(?1)"), params![name]).map_err(err)?;
        Ok(())
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        let (added, removed) = diff(before, &group.data.nodes);
        self.update_permissions("group_permissions", "name", &group.name, &added, &removed)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        self.conn.execute(&self.q("DELETE FROM `{prefix}group_permissions` WHERE `name`=?1"), params![name]).map_err(err)?;
        self.conn.execute(&self.q("DELETE FROM `{prefix}groups` WHERE `name`=?1"), params![name]).map_err(err)?;
        Ok(())
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        let mut st = self.conn.prepare(&self.q("SELECT `name`, `groups` FROM `{prefix}tracks`")).map_err(err)?;
        let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(err)?;
        let mut out = Vec::new();
        for row in rows {
            let (n, g) = row.map_err(err)?;
            out.push(Track::from_json(&n, &g));
        }
        Ok(out)
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        self.conn
            .execute(&self.q("INSERT INTO `{prefix}tracks` (`name`, `groups`) VALUES(?1, ?2) ON CONFLICT(`name`) DO UPDATE SET `groups`=excluded.`groups`"), params![track.name, track.groups_json()])
            .map_err(err)?;
        Ok(())
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        self.conn.execute(&self.q("DELETE FROM `{prefix}tracks` WHERE `name`=?1"), params![name]).map_err(err)?;
        Ok(())
    }

    fn save_player_data(&mut self, id: u128, username: &str) -> Result<PlayerSave> {
        let key = uuid::format(id);
        let name = username.to_lowercase();
        let mut r = PlayerSave::default();
        match self.one_string("SELECT `username` FROM `{prefix}players` WHERE `uuid`=?1 LIMIT 1", &key)? {
            None => {
                self.conn.execute(&self.q("INSERT INTO `{prefix}players` (`uuid`, `username`, `primary_group`) VALUES(?1, ?2, ?3)"), params![key, name, DEFAULT_GROUP]).map_err(err)?;
                r.new_player = true;
            }
            Some(old) if old != name => {
                self.conn.execute(&self.q("UPDATE `{prefix}players` SET `username`=?1 WHERE `uuid`=?2"), params![name, key]).map_err(err)?;
                r.username_changed = Some(old);
            }
            _ => {}
        }
        let others: Vec<String> = {
            let mut st = self.conn.prepare(&self.q("SELECT `uuid` FROM `{prefix}players` WHERE `username`=?1 AND NOT `uuid`=?2")).map_err(err)?;
            let rows = st.query_map(params![name, key], |r| r.get::<_, String>(0)).map_err(err)?;
            rows.collect::<std::result::Result<_, _>>().map_err(err)?
        };
        if !others.is_empty() {
            self.conn.execute(&self.q("DELETE FROM `{prefix}players` WHERE `username`=?1 AND NOT `uuid`=?2"), params![name, key]).map_err(err)?;
            r.other_uuids = others.iter().filter_map(|u| uuid::parse(u)).collect();
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        Ok(self.one_string("SELECT `uuid` FROM `{prefix}players` WHERE `username`=?1 LIMIT 1", &username.to_lowercase())?.and_then(|u| uuid::parse(&u)))
    }

    fn name_of(&mut self, id: u128) -> Result<Option<String>> {
        Ok(self.one_string("SELECT `username` FROM `{prefix}players` WHERE `uuid`=?1 LIMIT 1", &uuid::format(id))?.filter(|n| n != "null"))
    }

    fn log_action(&mut self, a: &Action) -> Result<()> {
        self.conn
            .execute(
                &self.q("INSERT INTO `{prefix}actions` (`time`, `actor_uuid`, `actor_name`, `type`, `acted_uuid`, `acted_name`, `action`) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)"),
                params![
                    a.timestamp,
                    uuid::format(a.source_uuid),
                    a.source_name,
                    a.target_type.code().to_string(),
                    a.target_uuid.map(uuid::format).unwrap_or_else(|| "null".into()),
                    a.target_name,
                    a.description
                ],
            )
            .map_err(err)?;
        Ok(())
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let base = "SELECT `time`, `actor_uuid`, `actor_name`, `type`, `acted_uuid`, `acted_name`, `action` FROM `{prefix}actions`";
        let (sql, t, name) = match target {
            None => (format!("{base} ORDER BY `time` DESC, `id` DESC LIMIT ?1"), String::new(), String::new()),
            Some((t, n)) => (
                format!("{base} WHERE `type`=?2 AND (`acted_name`=?3 OR `acted_uuid`=?3) ORDER BY `time` DESC, `id` DESC LIMIT ?1"),
                t.code().to_string(),
                n.to_owned(),
            ),
        };
        let mut st = self.conn.prepare(&self.q(&sql)).map_err(err)?;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, String, String, String, String, String, String)> {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        };
        let limit = limit.min(i64::MAX as usize) as i64;
        let rows: Vec<_> = if target.is_some() {
            st.query_map(params![limit, t, name], map).map_err(err)?.collect::<std::result::Result<_, _>>().map_err(err)?
        } else {
            st.query_map(params![limit], map).map_err(err)?.collect::<std::result::Result<_, _>>().map_err(err)?
        };
        Ok(rows
            .into_iter()
            .filter_map(|(time, au, an, ty, tu, tn, d)| {
                Some(Action {
                    timestamp: time,
                    source_uuid: uuid::parse(&au)?,
                    source_name: an,
                    target_type: TargetType::parse(&ty)?,
                    target_uuid: if tu == "null" { None } else { uuid::parse(&tu) },
                    target_name: tn,
                    description: d,
                })
            })
            .collect())
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        // SQLite's LIKE is case-insensitive for ASCII, as MariaDB's.
        let (cond, arg) = match key {
            KeyMatch::Exact(k) => ("`permission` = ?1", k.clone()),
            KeyMatch::NotEqual(k) => ("`permission` <> ?1", k.clone()),
            KeyMatch::Like(p) => ("`permission` LIKE ?1", p.clone()),
            KeyMatch::NotLike(p) => ("`permission` NOT LIKE ?1", p.clone()),
        };
        let mut out = Vec::new();
        for (table, col, user) in [("user_permissions", "uuid", true), ("group_permissions", "name", false)] {
            let sql = self.q(&format!("SELECT `{col}`, `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{{prefix}}{table}` WHERE {cond}"));
            let mut st = self.conn.prepare(&sql).map_err(err)?;
            let rows = st
                .query_map(params![arg], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, i64>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, bool>(3)?,
                        r.get::<_, String>(4)?,
                        r.get::<_, String>(5)?,
                        r.get::<_, i64>(6)?,
                        r.get::<_, String>(7)?,
                    ))
                })
                .map_err(err)?;
            for row in rows {
                let (holder, id, p, v, s, w, e, x) = row.map_err(err)?;
                let Some(n) = read_node(id, p, v, s, w, e, x) else { continue };
                if user {
                    if let Some(u) = uuid::parse(&holder) {
                        out.push((HolderKey::User(u), n));
                    }
                } else {
                    out.push((HolderKey::Group(holder.to_lowercase()), n));
                }
            }
        }
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        let mut st = self
            .conn
            .prepare(&self.q("SELECT `uuid` FROM `{prefix}players` UNION SELECT `uuid` FROM `{prefix}user_permissions`"))
            .map_err(err)?;
        let rows = st.query_map([], |r| r.get::<_, String>(0)).map_err(err)?;
        let mut ids: Vec<u128> = Vec::new();
        for row in rows {
            ids.extend(uuid::parse(&row.map_err(err)?));
        }
        ids.sort_unstable();
        ids.dedup();
        Ok(ids)
    }

    fn messenger_init(&mut self) -> Result<i64> {
        Err("SQLite cannot carry the SQL messenger".into())
    }

    fn messenger_send(&mut self, _message: &str) -> Result<()> {
        Err("SQLite cannot carry the SQL messenger".into())
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
    use crate::horizon_luckperms_core::context::ContextSet;
    use crate::horizon_luckperms_core::node::key;

    const ALICE: u128 = 0xa11ce;
    const BOB: u128 = 0xb0b;

    fn db() -> SqliteStorage {
        let mut s = SqliteStorage::in_memory("luckperms_");
        s.init().unwrap();
        s
    }

    fn sorted(v: Vec<Node>) -> Vec<(String, bool, i64, String)> {
        let mut v = v;
        v.sort_by(|a, b| a.key.cmp(&b.key).then(a.expiry.cmp(&b.expiry)));
        v.into_iter().map(|n| (n.key, n.value, n.expiry, n.contexts.to_json())).collect()
    }

    fn sample() -> Vec<Node> {
        vec![
            Node::permanent("essentials.fly", true, ContextSet::new()),
            Node::permanent("essentials.home", false, ContextSet::of(&[("server", "survival"), ("world", "nether"), ("gamemode", "creative")])),
            Node::new("group.mod", true, 4_102_444_800, ContextSet::new()),
            Node::permanent(key::prefix(100, "&c[Admin] "), true, ContextSet::new()),
            Node::permanent(key::meta("color", "red"), true, ContextSet::new()),
        ]
    }

    #[test]
    fn the_schema_is_created_once_and_has_the_default_group() {
        let mut s = db();
        s.init().unwrap();
        assert_eq!(s.load_all_groups().unwrap().len(), 1);
        assert_eq!(s.load_all_groups().unwrap()[0].name, "default");
        for t in ["user_permissions", "group_permissions", "players", "groups", "actions", "tracks"] {
            let n: i64 = s.conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1", params![format!("luckperms_{t}")], |r| r.get(0)).unwrap();
            assert_eq!(n, 1, "{t}");
        }
        assert!(!s.sql_messenger());
    }

    #[test]
    fn users_are_saved_as_differences_and_read_back() {
        let mut s = db();
        let mut u = User::new(ALICE);
        u.username = Some("Alice".into());
        u.primary_group = "mod".into();
        u.data.nodes = sample();
        s.save_user(&u).unwrap();
        let (nodes, row) = s.load_user(ALICE).unwrap();
        assert_eq!(sorted(nodes.clone()), sorted(sample()));
        assert_eq!(row, Some(PlayerRow { username: "alice".into(), primary_group: "mod".into() }));
        assert!(nodes.iter().all(|n| n.row_id.is_some()), "rows keep their ids so deletes go by id");
        // A removed and an added node: only the difference is written.
        let mut u2 = User::new(ALICE);
        u2.username = Some("Alice".into());
        u2.primary_group = "mod".into();
        u2.saved = nodes.clone();
        u2.data.nodes = nodes.into_iter().filter(|n| n.key != "essentials.fly").collect();
        u2.data.nodes.push(Node::permanent("new.node", true, ContextSet::new()));
        s.save_user(&u2).unwrap();
        let keys: Vec<String> = sorted(s.load_user(ALICE).unwrap().0).into_iter().map(|n| n.0).collect();
        assert!(keys.contains(&"new.node".to_string()) && !keys.contains(&"essentials.fly".to_string()), "{keys:?}");
        assert_eq!(keys.len(), 5);
        assert_eq!(s.unique_users().unwrap(), vec![ALICE]);
        // A user with nothing but the default group loses the rows and goes back to the default group.
        let mut plain = User::new(ALICE);
        plain.give_default_if_needed();
        s.save_user(&plain).unwrap();
        let (nodes, row) = s.load_user(ALICE).unwrap();
        assert!(nodes.is_empty());
        assert_eq!(row.unwrap().primary_group, "default");
    }

    #[test]
    fn groups_tracks_and_search() {
        let mut s = db();
        s.create_group("mod").unwrap();
        s.create_group("mod").unwrap();
        let mut g = Group::new("mod");
        g.data.nodes = sample();
        s.save_group(&g, &[]).unwrap();
        assert_eq!(sorted(s.load_group("mod").unwrap().unwrap().data.nodes), sorted(sample()));
        assert!(s.load_group("nope").unwrap().is_none());
        let all = s.load_all_groups().unwrap();
        assert_eq!(all.iter().map(|g| g.name.as_str()).collect::<Vec<_>>().len(), 2);
        let mut u = User::new(BOB);
        u.data.nodes = vec![Node::permanent("essentials.fly", true, ContextSet::new())];
        s.save_user(&u).unwrap();
        let found = s.search(&KeyMatch::Exact("essentials.fly".into())).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|(h, _)| *h == HolderKey::User(BOB)));
        assert!(found.iter().any(|(h, _)| *h == HolderKey::Group("mod".into())));
        assert_eq!(s.search(&KeyMatch::Like("ESSENTIALS.%".into())).unwrap().len(), 3);
        assert_eq!(s.search(&KeyMatch::NotLike("essentials.%".into())).unwrap().len(), 3);
        s.save_track(&Track { name: "staff".into(), groups: vec!["default".into(), "mod".into()] }).unwrap();
        s.save_track(&Track { name: "staff".into(), groups: vec!["default".into()] }).unwrap();
        assert_eq!(s.load_all_tracks().unwrap(), vec![Track { name: "staff".into(), groups: vec!["default".into()] }]);
        s.delete_track("staff").unwrap();
        s.delete_group("mod").unwrap();
        assert!(s.load_all_tracks().unwrap().is_empty());
        assert!(s.load_group("mod").unwrap().is_none());
        assert_eq!(s.search(&KeyMatch::Exact("essentials.fly".into())).unwrap().len(), 1, "the group's rows went with it");
    }

    #[test]
    fn names_follow_the_players_table() {
        let mut s = db();
        assert!(s.save_player_data(ALICE, "Alice").unwrap().new_player);
        assert_eq!(s.save_player_data(ALICE, "Alicia").unwrap().username_changed.as_deref(), Some("alice"));
        let r = s.save_player_data(BOB, "alicia").unwrap();
        assert_eq!(r.other_uuids, vec![ALICE]);
        assert_eq!(s.uuid_of("ALICIA").unwrap(), Some(BOB));
        assert_eq!(s.name_of(BOB).unwrap().as_deref(), Some("alicia"));
        assert_eq!(s.name_of(ALICE).unwrap(), None);
    }

    #[test]
    fn the_action_log_is_newest_first() {
        let mut s = db();
        let mk = |t: i64, d: &str, ty: TargetType, n: &str| Action { timestamp: t, source_uuid: ALICE, source_name: "Alice".into(), target_type: ty, target_uuid: None, target_name: n.into(), description: d.into() };
        s.log_action(&mk(10, "one", TargetType::Group, "admin")).unwrap();
        s.log_action(&mk(30, "three", TargetType::Group, "mod")).unwrap();
        s.log_action(&mk(20, "two", TargetType::Track, "staff")).unwrap();
        assert_eq!(s.actions(10, None).unwrap().iter().map(|a| a.description.clone()).collect::<Vec<_>>(), vec!["three", "two", "one"]);
        assert_eq!(s.actions(1, None).unwrap().len(), 1);
        assert_eq!(s.actions(10, Some((TargetType::Group, "admin"))).unwrap().len(), 1);
        assert!(s.actions(10, Some((TargetType::User, "admin"))).unwrap().is_empty());
    }

    #[test]
    fn a_file_database_survives_a_reopen_and_a_bad_prefix_is_refused() {
        let d = super::super::file::tests::Dir::new();
        let file = d.0.join("luckperms-sqlite.db");
        let mut s = SqliteStorage::open(&file, "lp_").unwrap();
        s.init().unwrap();
        s.save_player_data(ALICE, "Alice").unwrap();
        drop(s);
        let mut s = SqliteStorage::open(&file, "lp_").unwrap();
        s.init().unwrap();
        assert_eq!(s.uuid_of("alice").unwrap(), Some(ALICE));
        assert!(SqliteStorage::open(&file, "lp`; DROP TABLE x; --").is_err());
        assert!(SqliteStorage::open(&d.0.join("no-such-dir/x.db"), "lp_").is_err());
    }
}
