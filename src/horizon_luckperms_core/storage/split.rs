//! Split storage (`SplitStorage`): user, group, track, uuid and log data each go to the storage the
//! `split-storage.methods` name. One storage object serves every kind that names the same method, as in
//! LuckPerms (a `yaml` for groups and tracks is one folder). The SQL messenger runs on the first backing
//! that can carry it.

use super::{ActionFilter, HolderKey, KeyMatch, PlayerRow, PlayerSave, Result, Storage};
use crate::horizon_luckperms_core::actionlog::{Action, TargetType};
use crate::horizon_luckperms_core::model::{Group, Track, User};
use crate::horizon_luckperms_core::node::Node;
use std::sync::{Arc, Mutex, MutexGuard};

type Shared = Arc<Mutex<Box<dyn Storage>>>;

fn lock(s: &Shared) -> MutexGuard<'_, Box<dyn Storage>> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct SplitStorage {
    /// Every distinct storage, with the method that opened it.
    backings: Vec<(String, Shared)>,
    user: Shared,
    group: Shared,
    track: Shared,
    uuid: Shared,
    log: Shared,
    messenger: Option<Shared>,
}

impl SplitStorage {
    /// `methods` are the storages of user, group, track, uuid and log, in that order. `open` makes the storage of a
    /// method; it is called once per distinct method.
    pub fn new(methods: [&str; 5], mut open: impl FnMut(&str) -> Result<Box<dyn Storage>>) -> Result<Self> {
        let mut backings: Vec<(String, Shared)> = Vec::new();
        let mut pick = |method: &str| -> Result<Shared> {
            let m = method.trim().to_lowercase();
            if let Some((_, s)) = backings.iter().find(|(name, _)| *name == m) {
                return Ok(s.clone());
            }
            let s: Shared = Arc::new(Mutex::new(open(&m)?));
            backings.push((m, s.clone()));
            Ok(s)
        };
        let user = pick(methods[0])?;
        let group = pick(methods[1])?;
        let track = pick(methods[2])?;
        let uuid = pick(methods[3])?;
        let log = pick(methods[4])?;
        let messenger = backings.iter().find(|(_, s)| lock(s).sql_messenger()).map(|(_, s)| s.clone());
        Ok(SplitStorage { backings, user, group, track, uuid, log, messenger })
    }

    /// How many distinct storages are open.
    pub fn backing_count(&self) -> usize {
        self.backings.len()
    }
}

impl Storage for SplitStorage {
    fn name(&self) -> &'static str {
        "Split Storage"
    }

    fn init(&mut self) -> Result<()> {
        let mut failed = Vec::new();
        for (method, s) in &self.backings {
            if let Err(e) = lock(s).init() {
                eprintln!("[LuckPerms] the {method} storage failed to start: {e}");
                failed.push(method.clone());
            }
        }
        if failed.is_empty() { Ok(()) } else { Err(format!("one of the backings failed to init ({})", failed.join(", "))) }
    }

    fn sql_messenger(&self) -> bool {
        self.messenger.is_some()
    }

    fn load_user(&mut self, uuid: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
        lock(&self.user).load_user(uuid)
    }

    fn save_user(&mut self, user: &User) -> Result<()> {
        lock(&self.user).save_user(user)
    }

    fn load_all_groups(&mut self) -> Result<Vec<Group>> {
        lock(&self.group).load_all_groups()
    }

    fn load_group(&mut self, name: &str) -> Result<Option<Group>> {
        lock(&self.group).load_group(name)
    }

    fn create_group(&mut self, name: &str) -> Result<()> {
        lock(&self.group).create_group(name)
    }

    fn save_group(&mut self, group: &Group, before: &[Node]) -> Result<()> {
        lock(&self.group).save_group(group, before)
    }

    fn delete_group(&mut self, name: &str) -> Result<()> {
        lock(&self.group).delete_group(name)
    }

    fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
        lock(&self.track).load_all_tracks()
    }

    fn save_track(&mut self, track: &Track) -> Result<()> {
        lock(&self.track).save_track(track)
    }

    fn delete_track(&mut self, name: &str) -> Result<()> {
        lock(&self.track).delete_track(name)
    }

    fn save_player_data(&mut self, uuid: u128, username: &str) -> Result<PlayerSave> {
        lock(&self.uuid).save_player_data(uuid, username)
    }

    fn uuid_of(&mut self, username: &str) -> Result<Option<u128>> {
        lock(&self.uuid).uuid_of(username)
    }

    fn name_of(&mut self, uuid: u128) -> Result<Option<String>> {
        lock(&self.uuid).name_of(uuid)
    }

    fn log_action(&mut self, action: &Action) -> Result<()> {
        lock(&self.log).log_action(action)
    }

    fn actions(&mut self, limit: usize, target: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
        lock(&self.log).actions(limit, target)
    }

    fn log_entries(&mut self, filter: &ActionFilter) -> Result<Vec<Action>> {
        lock(&self.log).log_entries(filter)
    }

    /// The users' nodes come from the user storage and the groups' from the group storage.
    fn search(&mut self, key: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
        if Arc::ptr_eq(&self.user, &self.group) {
            return lock(&self.user).search(key);
        }
        let mut out: Vec<(HolderKey, Node)> = lock(&self.user).search(key)?.into_iter().filter(|(h, _)| matches!(h, HolderKey::User(_))).collect();
        out.extend(lock(&self.group).search(key)?.into_iter().filter(|(h, _)| matches!(h, HolderKey::Group(_))));
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn unique_users(&mut self) -> Result<Vec<u128>> {
        lock(&self.user).unique_users()
    }

    fn messenger_init(&mut self) -> Result<i64> {
        match &self.messenger {
            Some(m) => lock(m).messenger_init(),
            None => Err("no storage of this split setup can carry the SQL messenger".into()),
        }
    }

    fn messenger_send(&mut self, message: &str) -> Result<()> {
        match &self.messenger {
            Some(m) => lock(m).messenger_send(message),
            None => Err("no storage of this split setup can carry the SQL messenger".into()),
        }
    }

    fn messenger_poll(&mut self, last_id: i64) -> Result<Vec<(i64, String)>> {
        match &self.messenger {
            Some(m) => lock(m).messenger_poll(last_id),
            None => Ok(Vec::new()),
        }
    }

    fn messenger_housekeep(&mut self) -> Result<()> {
        match &self.messenger {
            Some(m) => lock(m).messenger_housekeep(),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_luckperms_core::context::ContextSet;
    use crate::horizon_luckperms_core::storage::file::tests::{open, Dir};
    use crate::horizon_luckperms_core::storage::MemoryStorage;

    const ALICE: u128 = 0xa11ce;

    fn node(k: &str) -> Node {
        Node::permanent(k, true, ContextSet::new())
    }

    fn opener<'a>(dir: &'a Dir, opened: &'a std::cell::RefCell<Vec<String>>) -> impl FnMut(&str) -> Result<Box<dyn Storage>> + 'a {
        move |m: &str| {
            opened.borrow_mut().push(m.to_owned());
            if m == "memory" {
                return Ok(Box::new(MemoryStorage::default()));
            }
            Ok(Box::new(open(m, &dir.0)))
        }
    }

    #[test]
    fn each_kind_goes_to_its_storage_and_one_method_opens_once() {
        let d = Dir::new();
        let opened = std::cell::RefCell::new(Vec::new());
        let mut s = SplitStorage::new(["yaml-combined", "json", "json", "toml", "memory"], opener(&d, &opened)).unwrap();
        assert_eq!(s.backing_count(), 4);
        assert_eq!(opened.borrow().iter().filter(|m| *m == "json").count(), 1, "groups and tracks share one json storage");
        s.init().unwrap();

        let mut u = User::new(ALICE);
        u.username = Some("Alice".into());
        u.data.nodes = vec![node("essentials.fly")];
        s.save_user(&u).unwrap();
        let mut g = Group::new("admin");
        g.data.nodes = vec![node("essentials.fly"), node("x.y")];
        s.save_group(&g, &[]).unwrap();
        s.save_track(&Track { name: "staff".into(), groups: vec!["admin".into()] }).unwrap();
        s.save_player_data(ALICE, "Alice").unwrap();
        let a = Action { timestamp: 1, source_uuid: ALICE, source_name: "Alice".into(), target_type: TargetType::Group, target_uuid: None, target_name: "admin".into(), description: "x".into() };
        s.log_action(&a).unwrap();

        // Each file landed in its own folder.
        assert!(d.0.join("yaml-storage/users.yml").exists());
        assert!(d.0.join("json-storage/groups/admin.json").exists());
        assert!(d.0.join("json-storage/tracks/staff.json").exists());
        assert!(d.0.join("toml-storage/uuidcache.txt").exists());
        // Reads come back through the same routes.
        assert_eq!(s.load_user(ALICE).unwrap().0.len(), 1);
        assert_eq!(s.load_all_groups().unwrap().len(), 1);
        assert_eq!(s.load_all_tracks().unwrap().len(), 1);
        assert_eq!(s.uuid_of("alice").unwrap(), Some(ALICE));
        assert_eq!(s.actions(10, None).unwrap(), vec![a]);
        assert_eq!(s.unique_users().unwrap(), vec![ALICE]);
        // Search joins the users of one storage and the groups of the other.
        let found = s.search(&KeyMatch::Exact("essentials.fly".into())).unwrap();
        assert_eq!(found.iter().map(|(h, _)| h.clone()).collect::<Vec<_>>(), vec![HolderKey::User(ALICE), HolderKey::Group("admin".into())]);
        s.delete_group("admin").unwrap();
        assert!(s.load_group("admin").unwrap().is_none());
    }

    #[test]
    fn the_messenger_runs_on_a_backing_that_can_carry_it() {
        let d = Dir::new();
        let opened = std::cell::RefCell::new(Vec::new());
        let mut files_only = SplitStorage::new(["yaml", "yaml", "yaml", "json", "json"], opener(&d, &opened)).unwrap();
        assert!(!files_only.sql_messenger());
        assert!(files_only.messenger_init().is_err());
        assert!(files_only.messenger_poll(0).unwrap().is_empty());

        let opened = std::cell::RefCell::new(Vec::new());
        let mut with_sql = SplitStorage::new(["memory", "yaml", "yaml", "memory", "yaml"], opener(&d, &opened)).unwrap();
        assert!(with_sql.sql_messenger());
        with_sql.init().unwrap();
        let start = with_sql.messenger_init().unwrap();
        with_sql.messenger_send("{\"x\":1}").unwrap();
        let got = with_sql.messenger_poll(start).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].1, "{\"x\":1}");
    }

    #[test]
    fn a_method_that_cannot_open_fails_the_setup_with_its_reason() {
        let d = Dir::new();
        let opened = std::cell::RefCell::new(Vec::new());
        let mut open_fn = opener(&d, &opened);
        let r = SplitStorage::new(["yaml", "yaml", "yaml", "yaml", "nonsense"], |m| if m == "nonsense" { Err("storage method nonsense is not available".into()) } else { open_fn(m) });
        assert_eq!(r.err().as_deref(), Some("storage method nonsense is not available"));
    }

    #[test]
    fn one_failing_backing_fails_init() {
        struct Broken;
        impl Storage for Broken {
            fn name(&self) -> &'static str {
                "broken"
            }
            fn init(&mut self) -> Result<()> {
                Err("down".into())
            }
            fn load_user(&mut self, _: u128) -> Result<(Vec<Node>, Option<PlayerRow>)> {
                Err("down".into())
            }
            fn save_user(&mut self, _: &User) -> Result<()> {
                Err("down".into())
            }
            fn load_all_groups(&mut self) -> Result<Vec<Group>> {
                Err("down".into())
            }
            fn load_group(&mut self, _: &str) -> Result<Option<Group>> {
                Err("down".into())
            }
            fn create_group(&mut self, _: &str) -> Result<()> {
                Err("down".into())
            }
            fn save_group(&mut self, _: &Group, _: &[Node]) -> Result<()> {
                Err("down".into())
            }
            fn delete_group(&mut self, _: &str) -> Result<()> {
                Err("down".into())
            }
            fn load_all_tracks(&mut self) -> Result<Vec<Track>> {
                Err("down".into())
            }
            fn save_track(&mut self, _: &Track) -> Result<()> {
                Err("down".into())
            }
            fn delete_track(&mut self, _: &str) -> Result<()> {
                Err("down".into())
            }
            fn save_player_data(&mut self, _: u128, _: &str) -> Result<PlayerSave> {
                Err("down".into())
            }
            fn uuid_of(&mut self, _: &str) -> Result<Option<u128>> {
                Err("down".into())
            }
            fn name_of(&mut self, _: u128) -> Result<Option<String>> {
                Err("down".into())
            }
            fn log_action(&mut self, _: &Action) -> Result<()> {
                Err("down".into())
            }
            fn actions(&mut self, _: usize, _: Option<(TargetType, &str)>) -> Result<Vec<Action>> {
                Err("down".into())
            }
            fn search(&mut self, _: &KeyMatch) -> Result<Vec<(HolderKey, Node)>> {
                Err("down".into())
            }
            fn messenger_init(&mut self) -> Result<i64> {
                Err("down".into())
            }
            fn messenger_send(&mut self, _: &str) -> Result<()> {
                Err("down".into())
            }
            fn messenger_poll(&mut self, _: i64) -> Result<Vec<(i64, String)>> {
                Err("down".into())
            }
            fn messenger_housekeep(&mut self) -> Result<()> {
                Err("down".into())
            }
        }
        let mut s = SplitStorage::new(["a", "a", "a", "a", "a"], |_| Ok(Box::new(Broken) as Box<dyn Storage>)).unwrap();
        let e = s.init().unwrap_err();
        assert!(e.contains("failed to init") && e.contains('a'), "{e}");
    }
}
