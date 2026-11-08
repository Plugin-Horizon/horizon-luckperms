//! LuckPerms' MariaDB/MySQL schema (`schema/mariadb.sql`) and statements (`SqlStorage`), so
//! this server reads and writes the same tables as LuckPerms on Paper and Velocity.

use super::{diff, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::actionlog::{Action, TargetType};
use crate::config;
use crate::context::{ContextSet, SERVER, WORLD};
use crate::model::{Group, Track, User, DEFAULT_GROUP};
use crate::node::Node;
use mysql::prelude::*;
use mysql::{OptsBuilder, Pool, PoolConstraints, PoolOpts, PooledConn, SslOpts};
use std::time::Duration;

pub struct SqlStorage {
    pool: Pool,
    prefix: String,
}

type Row7 = (i64, String, bool, String, String, i64, String);
type Row8 = (String, i64, String, bool, String, String, i64, String);

fn err(e: impl std::fmt::Display) -> String {
    // The mysql crate's errors never carry the password.
    format!("database: {e}")
}

impl SqlStorage {
    /// Connects lazily: the pool opens its connections on first use.
    pub fn new(cfg: &config::Storage) -> Result<Self> {
        let (host, port) = match cfg.address.rsplit_once(':') {
            Some((h, p)) => (h.to_owned(), p.parse::<u16>().map_err(|_| "storage.address: bad port".to_string())?),
            None => (cfg.address.clone(), 3306),
        };
        let max = cfg.maximum_pool_size.max(1);
        let min = cfg.minimum_idle.min(max);
        let constraints = PoolConstraints::new(min, max).ok_or("storage: minimum-idle above maximum-pool-size")?;
        let mut opts = OptsBuilder::new()
            .ip_or_hostname(Some(host))
            .tcp_port(port)
            .user(Some(cfg.username.clone()))
            .pass(Some(cfg.password.clone()))
            .db_name(Some(cfg.database.clone()))
            .tcp_connect_timeout(Some(Duration::from_millis(cfg.connection_timeout_ms)))
            .pool_opts(PoolOpts::default().with_constraints(constraints));
        if cfg.use_ssl {
            opts = opts.ssl_opts(Some(SslOpts::default()));
        }
        let pool = Pool::new(opts).map_err(err)?;
        Ok(SqlStorage { pool, prefix: cfg.table_prefix.clone() })
    }

    fn conn(&self) -> Result<PooledConn> {
        self.pool.get_conn().map_err(err)
    }

    /// `StatementProcessor`: `{prefix}` → the table prefix.
    fn q(&self, s: &str) -> String {
        s.replace("{prefix}", &self.prefix)
    }

    fn table_exists(&self, c: &mut PooledConn, table: &str) -> Result<bool> {
        let r: Option<i64> = c
            .exec_first(
                "SELECT 1 FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? LIMIT 1",
                (table,),
            )
            .map_err(err)?;
        Ok(r.is_some())
    }
}

/// The schema LuckPerms creates on MariaDB (its `mariadb.sql`).
const SCHEMA: &[&str] = &[
    "CREATE TABLE `{prefix}user_permissions` (`id` INT AUTO_INCREMENT NOT NULL, `uuid` VARCHAR(36) NOT NULL, `permission` VARCHAR(200) NOT NULL, `value` BOOL NOT NULL, `server` VARCHAR(36) NOT NULL, `world` VARCHAR(64) NOT NULL, `expiry` BIGINT NOT NULL, `contexts` VARCHAR(200) NOT NULL, PRIMARY KEY (`id`)) DEFAULT CHARSET = utf8mb4",
    "CREATE INDEX `{prefix}user_permissions_uuid` ON `{prefix}user_permissions` (`uuid`)",
    "CREATE TABLE `{prefix}group_permissions` (`id` INT AUTO_INCREMENT NOT NULL, `name` VARCHAR(36) NOT NULL, `permission` VARCHAR(200) NOT NULL, `value` BOOL NOT NULL, `server` VARCHAR(36) NOT NULL, `world` VARCHAR(64) NOT NULL, `expiry` BIGINT NOT NULL, `contexts` VARCHAR(200) NOT NULL, PRIMARY KEY (`id`)) DEFAULT CHARSET = utf8mb4",
    "CREATE INDEX `{prefix}group_permissions_name` ON `{prefix}group_permissions` (`name`)",
    "CREATE TABLE `{prefix}players` (`uuid` VARCHAR(36) NOT NULL, `username` VARCHAR(16) NOT NULL, `primary_group` VARCHAR(36) NOT NULL, PRIMARY KEY (`uuid`)) DEFAULT CHARSET = utf8mb4",
    "CREATE INDEX `{prefix}players_username` ON `{prefix}players` (`username`)",
    "CREATE TABLE `{prefix}groups` (`name` VARCHAR(36) NOT NULL, PRIMARY KEY (`name`)) DEFAULT CHARSET = utf8mb4",
    "CREATE TABLE `{prefix}actions` (`id` INT AUTO_INCREMENT NOT NULL, `time` BIGINT NOT NULL, `actor_uuid` VARCHAR(36) NOT NULL, `actor_name` VARCHAR(100) NOT NULL, `type` CHAR(1) NOT NULL, `acted_uuid` VARCHAR(36) NOT NULL, `acted_name` VARCHAR(36) NOT NULL, `action` VARCHAR(300) NOT NULL, PRIMARY KEY (`id`)) DEFAULT CHARSET = utf8mb4",
    "CREATE TABLE `{prefix}tracks` (`name` VARCHAR(36) NOT NULL, `groups` TEXT NOT NULL, PRIMARY KEY (`name`)) DEFAULT CHARSET = utf8mb4",
];

/// `SqlStorage.readNode`: server and world columns become contexts (`global` is none).
fn read_node(id: i64, permission: String, value: bool, server: String, world: String, expiry: i64, contexts: String) -> Option<Node> {
    if permission.is_empty() {
        return None;
    }
    let mut set = ContextSet::new();
    set.add(SERVER, if server.is_empty() { "global" } else { &server });
    set.add(WORLD, if world.is_empty() { "global" } else { &world });
    set.add_all(&ContextSet::from_json(&contexts));
    Some(Node::from_row(permission, value, expiry, set, id))
}

/// `SqlStorage.writeNode`'s columns: (permission, value, server, world, expiry, contexts). The
/// first server and world value (sorted) go into their columns, the rest into the JSON.
fn node_columns(n: &Node) -> (String, bool, String, String, i64, String) {
    let mut rest = n.contexts.clone();
    let server = n.contexts.values(SERVER).first().map(|s| s.to_string());
    let world = n.contexts.values(WORLD).first().map(|s| s.to_string());
    rest.remove_key(SERVER);
    rest.remove_key(WORLD);
    for v in n.contexts.values(SERVER).iter().skip(1) {
        rest.add(SERVER, v);
    }
    for v in n.contexts.values(WORLD).iter().skip(1) {
        rest.add(WORLD, v);
    }
    let expiry = if n.has_expiry() { n.expiry } else { 0 };
    (
        n.key.clone(),
        n.value,
        server.unwrap_or_else(|| "global".into()),
        world.unwrap_or_else(|| "global".into()),
        expiry,
        rest.to_json(),
    )
}

impl SqlStorage {
    /// `updatePermissions`: deletes by row id where known, else by every column; then inserts.
    fn update_permissions(&self, c: &mut PooledConn, table: &str, holder_col: &str, holder: &str, added: &[Node], removed: &[Node]) -> Result<()> {
        let by_id: Vec<i64> = removed.iter().filter_map(|n| n.row_id).collect();
        if !by_id.is_empty() {
            let q = self.q(&format!("DELETE FROM `{{prefix}}{table}` WHERE `id`=?"));
            c.exec_batch(q, by_id.iter().map(|id| (*id,))).map_err(err)?;
        }
        let by_props: Vec<&Node> = removed.iter().filter(|n| n.row_id.is_none()).collect();
        if !by_props.is_empty() {
            let q = self.q(&format!(
                "DELETE FROM `{{prefix}}{table}` WHERE `{holder_col}`=? AND `permission`=? AND `value`=? AND `server`=? AND `world`=? AND `expiry`=? AND `contexts`=?"
            ));
            c.exec_batch(
                q,
                by_props.iter().map(|n| {
                    let (p, v, s, w, e, x) = node_columns(n);
                    (holder.to_owned(), p, v, s, w, e, x)
                }),
            )
            .map_err(err)?;
        }
        if !added.is_empty() {
            let q = self.q(&format!(
                "INSERT INTO `{{prefix}}{table}` (`{holder_col}`, `permission`, `value`, `server`, `world`, `expiry`, `contexts`) VALUES(?, ?, ?, ?, ?, ?, ?)"
            ));
            c.exec_batch(
                q,
                added.iter().map(|n| {
                    let (p, v, s, w, e, x) = node_columns(n);
                    (holder.to_owned(), p, v, s, w, e, x)
                }),
            )
            .map_err(err)?;
        }
        Ok(())
    }

    fn select_nodes(&self, c: &mut PooledConn, table: &str, holder_col: &str, holder: &str) -> Result<Vec<Node>> {
        let q = self.q(&format!(
            "SELECT `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{{prefix}}{table}` WHERE `{holder_col}`=?"
        ));
        let rows: Vec<Row7> = c.exec(q, (holder,)).map_err(err)?;
        Ok(rows.into_iter().filter_map(|(id, p, v, s, w, e, x)| read_node(id, p, v, s, w, e, x)).collect())
    }
}

impl Storage for SqlStorage {
    fn name(&self) -> &'static str {
        "MariaDB"
    }

    fn init(&mut self) -> Result<()> {
        let mut c = self.conn()?;
        let first = format!("{}user_permissions", self.prefix);
        if !self.table_exists(&mut c, &first)? {
            for s in SCHEMA {
                c.query_drop(self.q(s)).map_err(err)?;
            }
        }
        c.exec_drop(self.q("INSERT INTO `{prefix}groups` (`name`) VALUES(?) ON DUPLICATE KEY UPDATE `name`=`name`"), (DEFAULT_GROUP,))
            .map_err(err)?;
        Ok(())
    }

    fn load_user(&mut self, uuid: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        let mut c = self.conn()?;
        let id = crate::uuid::format(uuid);
        let nodes = self.select_nodes(&mut c, "user_permissions", "uuid", &id)?;
        let row: Option<(String, String)> = c
            .exec_first(self.q("SELECT `username`, `primary_group` FROM `{prefix}players` WHERE `uuid`=? LIMIT 1"), (&id,))
            .map_err(err)?;
        Ok((nodes, row.map(|(username, primary_group)| PlayerRow { username, primary_group })))
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        let mut c = self.conn()?;
        let id = crate::uuid::format(user.uuid);
        if !user.is_non_default() {
            // `deleteUser`: no rows, primary group back to default.
            c.exec_drop(self.q("DELETE FROM `{prefix}user_permissions` WHERE `uuid`=?"), (&id,)).map_err(err)?;
            c.exec_drop(self.q("UPDATE `{prefix}players` SET `primary_group`=? WHERE `uuid`=?"), (DEFAULT_GROUP, &id))
                .map_err(err)?;
            return Ok(());
        }
        let (added, removed) = diff(&user.saved, &user.data.nodes);
        self.update_permissions(&mut c, "user_permissions", "uuid", &id, &added, &removed)?;
        // `insertPlayerData`: update the primary group of an existing row, else insert one.
        let existing: Option<String> = c
            .exec_first(self.q("SELECT `primary_group` FROM `{prefix}players` WHERE `uuid`=? LIMIT 1"), (&id,))
            .map_err(err)?;
        let primary = if user.primary_group.is_empty() { DEFAULT_GROUP.to_owned() } else { user.primary_group.clone() };
        match existing {
            Some(p) if p == primary => {}
            Some(_) => {
                c.exec_drop(self.q("UPDATE `{prefix}players` SET `primary_group`=? WHERE `uuid`=?"), (&primary, &id))
                    .map_err(err)?;
            }
            None => {
                let name = user.username.clone().unwrap_or_else(|| "null".into()).to_lowercase();
                c.exec_drop(self.q("INSERT INTO `{prefix}players` (`uuid`, `username`, `primary_group`) VALUES(?, ?, ?)"), (&id, name, &primary))
                    .map_err(err)?;
            }
        }
        Ok(())
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        let mut c = self.conn()?;
        let names: Vec<String> = c.query(self.q("SELECT `name` FROM `{prefix}groups`")).map_err(err)?;
        let rows: Vec<Row8> = c
            .query(self.q("SELECT `name`, `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{prefix}group_permissions`"))
            .map_err(err)?;
        let mut groups: std::collections::BTreeMap<String, Group> =
            names.into_iter().map(|n| (n.to_lowercase(), Group::new(&n))).collect();
        for (name, id, p, v, s, w, e, x) in rows {
            if let (Some(g), Some(n)) = (groups.get_mut(&name.to_lowercase()), read_node(id, p, v, s, w, e, x)) {
                g.data.nodes.push(n);
            }
        }
        Ok(groups.into_values().collect())
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        let mut c = self.conn()?;
        let exists: Option<String> =
            c.exec_first(self.q("SELECT `name` FROM `{prefix}groups` WHERE `name`=? LIMIT 1"), (name,)).map_err(err)?;
        if exists.is_none() {
            return Ok(None);
        }
        let mut g = Group::new(name);
        g.data.nodes = self.select_nodes(&mut c, "group_permissions", "name", name)?;
        Ok(Some(g))
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(self.q("INSERT INTO `{prefix}groups` (`name`) VALUES(?) ON DUPLICATE KEY UPDATE `name`=`name`"), (name,))
            .map_err(err)
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        let mut c = self.conn()?;
        let (added, removed) = diff(before, &group.data.nodes);
        self.update_permissions(&mut c, "group_permissions", "name", &group.name, &added, &removed)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(self.q("DELETE FROM `{prefix}group_permissions` WHERE `name`=?"), (name,)).map_err(err)?;
        c.exec_drop(self.q("DELETE FROM `{prefix}groups` WHERE `name`=?"), (name,)).map_err(err)
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        let mut c = self.conn()?;
        let rows: Vec<(String, String)> = c.query(self.q("SELECT `name`, `groups` FROM `{prefix}tracks`")).map_err(err)?;
        Ok(rows.into_iter().map(|(n, g)| Track::from_json(&n, &g)).collect())
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(
            self.q("INSERT INTO `{prefix}tracks` (`name`, `groups`) VALUES(?, ?) ON DUPLICATE KEY UPDATE `groups`=VALUES(`groups`)"),
            (&track.name, track.groups_json()),
        )
        .map_err(err)
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(self.q("DELETE FROM `{prefix}tracks` WHERE `name`=?"), (name,)).map_err(err)
    }

    fn save_player_data(&mut self, uuid: u128, username: &str) -> Result<PlayerSave> {
        let mut c = self.conn()?;
        let id = crate::uuid::format(uuid);
        let name = username.to_lowercase();
        let mut r = PlayerSave::default();
        let current: Option<String> =
            c.exec_first(self.q("SELECT `username` FROM `{prefix}players` WHERE `uuid`=? LIMIT 1"), (&id,)).map_err(err)?;
        match current {
            None => {
                c.exec_drop(self.q("INSERT INTO `{prefix}players` (`uuid`, `username`, `primary_group`) VALUES(?, ?, ?)"), (&id, &name, DEFAULT_GROUP))
                    .map_err(err)?;
                r.new_player = true;
            }
            Some(old) if old != name => {
                c.exec_drop(self.q("UPDATE `{prefix}players` SET `username`=? WHERE `uuid`=?"), (&name, &id)).map_err(err)?;
                r.username_changed = Some(old);
            }
            _ => {}
        }
        let others: Vec<String> = c
            .exec(self.q("SELECT `uuid` FROM `{prefix}players` WHERE `username`=? AND NOT `uuid`=?"), (&name, &id))
            .map_err(err)?;
        if !others.is_empty() {
            c.exec_drop(self.q("DELETE FROM `{prefix}players` WHERE `username`=? AND NOT `uuid`=?"), (&name, &id)).map_err(err)?;
            r.other_uuids = others.iter().filter_map(|u| crate::uuid::parse(u)).collect();
        }
        Ok(r)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        let mut c = self.conn()?;
        let r: Option<String> = c
            .exec_first(self.q("SELECT `uuid` FROM `{prefix}players` WHERE `username`=? LIMIT 1"), (username.to_lowercase(),))
            .map_err(err)?;
        Ok(r.and_then(|u| crate::uuid::parse(&u)))
    }

    fn name_of(&mut self, uuid: u128) -> Result<Option<String>> {
        let mut c = self.conn()?;
        let r: Option<String> = c
            .exec_first(self.q("SELECT `username` FROM `{prefix}players` WHERE `uuid`=? LIMIT 1"), (crate::uuid::format(uuid),))
            .map_err(err)?;
        Ok(r.filter(|n| n != "null"))
    }

    fn log_action(&mut self, a: &Action) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(
            self.q("INSERT INTO `{prefix}actions` (`time`, `actor_uuid`, `actor_name`, `type`, `acted_uuid`, `acted_name`, `action`) VALUES(?, ?, ?, ?, ?, ?, ?)"),
            (
                a.timestamp,
                crate::uuid::format(a.source_uuid),
                &a.source_name,
                a.target_type.code().to_string(),
                a.target_uuid.map(crate::uuid::format).unwrap_or_else(|| "null".into()),
                &a.target_name,
                &a.description,
            ),
        )
        .map_err(err)
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        let mut c = self.conn()?;
        type ActionRow = (i64, String, String, String, String, String, String);
        let base = "SELECT `time`, `actor_uuid`, `actor_name`, `type`, `acted_uuid`, `acted_name`, `action` FROM `{prefix}actions`";
        let rows: Vec<ActionRow> = match target {
            None => c.exec(self.q(&format!("{base} ORDER BY `time` DESC, `id` DESC LIMIT ?")), (limit as u64,)).map_err(err)?,
            Some((t, name)) => c
                .exec(
                    self.q(&format!("{base} WHERE `type`=? AND (`acted_name`=? OR `acted_uuid`=?) ORDER BY `time` DESC, `id` DESC LIMIT ?")),
                    (t.code().to_string(), name, name, limit as u64),
                )
                .map_err(err)?,
        };
        Ok(rows
            .into_iter()
            .filter_map(|(time, au, an, ty, tu, tn, d)| {
                Some(Action {
                    timestamp: time,
                    source_uuid: crate::uuid::parse(&au)?,
                    source_name: an,
                    target_type: TargetType::parse(&ty)?,
                    target_uuid: if tu == "null" { None } else { crate::uuid::parse(&tu) },
                    target_name: tn,
                    description: d,
                })
            })
            .collect())
    }

    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        let mut c = self.conn()?;
        let (cond, arg) = match key {
            KeyMatch::Exact(k) => ("`permission` = ?", k.clone()),
            KeyMatch::NotEqual(k) => ("`permission` <> ?", k.clone()),
            KeyMatch::Like(p) => ("`permission` LIKE ?", p.clone()),
            KeyMatch::NotLike(p) => ("`permission` NOT LIKE ?", p.clone()),
        };
        let users: Vec<Row8> = c
            .exec(
                self.q(&format!("SELECT `uuid`, `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{{prefix}}user_permissions` WHERE {cond}")),
                (&arg,),
            )
            .map_err(err)?;
        let groups: Vec<Row8> = c
            .exec(
                self.q(&format!("SELECT `name`, `id`, `permission`, `value`, `server`, `world`, `expiry`, `contexts` FROM `{{prefix}}group_permissions` WHERE {cond}")),
                (&arg,),
            )
            .map_err(err)?;
        let mut out = Vec::new();
        for (u, id, p, v, s, w, e, x) in users {
            if let (Some(uuid), Some(n)) = (crate::uuid::parse(&u), read_node(id, p, v, s, w, e, x)) {
                out.push((HolderKey::User(uuid), n));
            }
        }
        for (g, id, p, v, s, w, e, x) in groups {
            if let Some(n) = read_node(id, p, v, s, w, e, x) {
                out.push((HolderKey::Group(g.to_lowercase()), n));
            }
        }
        Ok(out)
    }

    fn messenger_init(&mut self) -> Result<i64> {
        let mut c = self.conn()?;
        let table = format!("{}messenger", self.prefix);
        let create = format!(
            "CREATE TABLE IF NOT EXISTS `{table}` (`id` INT AUTO_INCREMENT NOT NULL, `time` TIMESTAMP NOT NULL, `msg` TEXT NOT NULL, PRIMARY KEY (`id`), KEY (`time`)) DEFAULT CHARSET = utf8mb4"
        );
        if let Err(e) = c.query_drop(&create) {
            if e.to_string().contains("Unknown character set") {
                c.query_drop(create.replace("utf8mb4", "utf8")).map_err(err)?;
            } else {
                return Err(err(e));
            }
        }
        let has_index: Option<i64> = c
            .exec_first(
                "SELECT 1 FROM INFORMATION_SCHEMA.STATISTICS WHERE TABLE_SCHEMA = DATABASE() AND TABLE_NAME = ? AND COLUMN_NAME = 'time' LIMIT 1",
                (&table,),
            )
            .map_err(err)?;
        if has_index.is_none() {
            c.query_drop(format!("CREATE INDEX `time` ON `{table}` (`time`)")).map_err(err)?;
        }
        let latest: Option<Option<i64>> = c.query_first(format!("SELECT MAX(`id`) FROM `{table}`")).map_err(err)?;
        Ok(latest.flatten().unwrap_or(-1))
    }

    fn messenger_send(&mut self, message: &str) -> Result<()> {
        let mut c = self.conn()?;
        c.exec_drop(self.q("INSERT INTO `{prefix}messenger` (`time`, `msg`) VALUES(NOW(), ?)"), (message,)).map_err(err)
    }

    fn messenger_poll(&mut self, last_id: i64) -> Result<Vec<(i64, String)>> {
        let mut c = self.conn()?;
        c.exec(
            self.q("SELECT `id`, `msg` FROM `{prefix}messenger` WHERE `id` > ? AND `time` > (NOW() - INTERVAL 30 SECOND)"),
            (last_id,),
        )
        .map_err(err)
    }

    fn messenger_housekeep(&mut self) -> Result<()> {
        let mut c = self.conn()?;
        c.query_drop(self.q("DELETE FROM `{prefix}messenger` WHERE `time` < (NOW() - INTERVAL 60 SECOND)")).map_err(err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_round_trip() {
        let n = Node::permanent(
            "essentials.fly",
            true,
            ContextSet::of(&[("server", "survival"), ("world", "world_nether"), ("gamemode", "creative")]),
        );
        let (p, v, s, w, e, x) = node_columns(&n);
        assert_eq!((p.as_str(), v, s.as_str(), w.as_str(), e, x.as_str()), ("essentials.fly", true, "survival", "world_nether", 0, r#"{"gamemode":"creative"}"#));
        let back = read_node(1, p, v, s, w, e, x).unwrap();
        assert_eq!(back, n);
        assert_eq!(back.row_id, Some(1));
    }

    #[test]
    fn global_columns() {
        let n = Node::permanent("a", false, ContextSet::new());
        let (_, _, s, w, _, x) = node_columns(&n);
        assert_eq!((s.as_str(), w.as_str(), x.as_str()), ("global", "global", "{}"));
        let back = read_node(2, "a".into(), false, "global".into(), "".into(), 0, "{}".into()).unwrap();
        assert!(back.contexts.is_empty());
    }

    #[test]
    fn two_servers_spill_into_json() {
        let n = Node::permanent("a", true, ContextSet::of(&[("server", "b"), ("server", "a")]));
        let (_, _, s, _, _, x) = node_columns(&n);
        assert_eq!(s, "a");
        assert_eq!(x, r#"{"server":"b"}"#);
        let back = read_node(3, "a".into(), true, s, "global".into(), 0, x).unwrap();
        assert_eq!(back.contexts, n.contexts);
    }
}
