//! `StorageFactory`: opens the storage a config names (`storage-method`, or the five `split-storage.methods`).
//!
//! Not available, and said so at start with the reason: `h2` (a Java database engine; no Rust implementation
//! exists among the dependencies native mods may use), `postgresql` and `mongodb` (no PostgreSQL or MongoDB client
//! among them either; the registry crates a mod may use are the ones `horizon-mod-deps` lists), `rest` and `custom`.

use super::file::{parse_method, FileStorage};
use super::split::SplitStorage;
use super::{MemoryStorage, Result, Storage};
use crate::horizon_luckperms_core::config::Config;

fn log(line: &str) {
    eprintln!("[LuckPerms] {line}");
}

/// The storage the config selects, not yet initialised (`Storage::init` is called by the runtime).
pub fn open(cfg: &Config) -> Result<Box<dyn Storage>> {
    if !cfg.split_storage.enabled {
        return open_method(cfg, &cfg.storage.method);
    }
    log("Loading storage provider... [SPLIT STORAGE]");
    let m = &cfg.split_storage.methods;
    let split = SplitStorage::new([m.user.as_str(), m.group.as_str(), m.track.as_str(), m.uuid.as_str(), m.log.as_str()], |method| open_method(cfg, method))?;
    Ok(Box::new(split))
}

/// One storage method by name.
pub fn open_method(cfg: &Config, method: &str) -> Result<Box<dyn Storage>> {
    let method = method.trim().to_lowercase();
    match method.as_str() {
        "memory" => {
            log("WARNING: storage method memory keeps everything in RAM; all data is lost at restart");
            Ok(Box::new(MemoryStorage::default()))
        }
        "mariadb" | "mysql" => {
            if cfg.storage.password.is_empty() {
                return Err("storage.password is empty (fill it in, or use method = \"memory\" for a lone test server)".into());
            }
            sql(cfg)
        }
        "sqlite" => sqlite(cfg),
        "h2" => Err("storage method h2 is not available: H2 is a Java database engine and no Rust implementation of it can be used by a mod (use sqlite, a file storage such as yaml or json, mariadb or mysql)".into()),
        "postgresql" => Err("storage method postgresql is not available: no PostgreSQL client is among the dependencies a mod may use (use mariadb, mysql, sqlite or a file storage)".into()),
        "mongodb" => Err("storage method mongodb is not available: no MongoDB client is among the dependencies a mod may use (use mariadb, mysql, sqlite or a file storage)".into()),
        "rest" | "custom" => Err(format!("storage method {method} is not supported")),
        other => match parse_method(other) {
            Some((format, combined)) => {
                if cfg.data_dir.as_os_str().is_empty() {
                    return Err("no data directory is known for the file storage".into());
                }
                Ok(Box::new(FileStorage::new(format, combined, &cfg.data_dir)))
            }
            None => Err(format!("storage method {other} is not known (mariadb, mysql, sqlite, yaml, json, hocon, toml, their -combined variants, memory)")),
        },
    }
}

#[cfg(feature = "mysql")]
fn sql(cfg: &Config) -> Result<Box<dyn Storage>> {
    Ok(Box::new(super::sql::SqlStorage::new(&cfg.storage)?))
}

#[cfg(not(feature = "mysql"))]
fn sql(_cfg: &Config) -> Result<Box<dyn Storage>> {
    Err("built without the mysql feature".into())
}

#[cfg(feature = "sqlite")]
fn sqlite(cfg: &Config) -> Result<Box<dyn Storage>> {
    if cfg.data_dir.as_os_str().is_empty() {
        return Err("no data directory is known for the SQLite file".into());
    }
    std::fs::create_dir_all(&cfg.data_dir).map_err(|e| format!("the data directory cannot be created: {}", e.kind()))?;
    Ok(Box::new(super::sqlite::SqliteStorage::open(&cfg.data_dir.join("luckperms-sqlite.db"), &cfg.storage.table_prefix)?))
}

#[cfg(not(feature = "sqlite"))]
fn sqlite(_cfg: &Config) -> Result<Box<dyn Storage>> {
    Err("built without the sqlite feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::horizon_luckperms_core::storage::file::tests::Dir;

    fn cfg(method: &str, dir: &Dir) -> Config {
        let mut c = Config::default();
        c.storage.method = method.into();
        c.data_dir = dir.0.clone();
        c
    }

    #[test]
    fn methods_that_cannot_run_say_why() {
        let d = Dir::new();
        for (m, why) in [("h2", "Java database engine"), ("postgresql", "PostgreSQL client"), ("mongodb", "MongoDB client")] {
            let e = open(&cfg(m, &d)).err().unwrap();
            assert!(e.contains(m) && e.contains(why), "{e}");
        }
        assert!(open(&cfg("rest", &d)).err().unwrap().contains("not supported"));
        assert!(open(&cfg("carrier-pigeon", &d)).err().unwrap().contains("not known"));
    }

    #[test]
    fn every_runnable_method_opens() {
        let d = Dir::new();
        for m in ["memory", "sqlite", "yaml", "json", "hocon", "toml", "yaml-combined", "json-combined", "hocon-combined", "toml-combined", "YAML", "flatfile"] {
            let mut s = open(&cfg(m, &d)).unwrap_or_else(|e| panic!("{m}: {e}"));
            s.init().unwrap_or_else(|e| panic!("{m}: {e}"));
        }
        assert!(d.0.join("luckperms-sqlite.db").exists());
        assert!(d.0.join("yaml-storage/users").is_dir());
        assert!(d.0.join("json-storage/groups").is_dir());
    }

    #[test]
    fn the_sql_methods_need_a_password_before_anything_connects() {
        let d = Dir::new();
        for m in ["mariadb", "mysql"] {
            let e = open(&cfg(m, &d)).err().unwrap();
            assert!(e.contains("storage.password is empty"), "{e}");
        }
    }

    #[test]
    fn a_file_storage_needs_a_data_directory() {
        let mut c = Config::default();
        c.storage.method = "yaml".into();
        assert!(open(&c).err().unwrap().contains("data directory"));
    }

    #[test]
    fn split_storage_opens_each_method_and_reports_the_one_that_cannot() {
        let d = Dir::new();
        let mut c = cfg("mariadb", &d);
        c.split_storage.enabled = true;
        c.split_storage.methods.user = "yaml-combined".into();
        c.split_storage.methods.group = "json".into();
        c.split_storage.methods.track = "json".into();
        c.split_storage.methods.uuid = "sqlite".into();
        c.split_storage.methods.log = "toml".into();
        let mut s = open(&c).unwrap();
        s.init().unwrap();
        assert_eq!(s.name(), "Split Storage");
        assert!(!s.sql_messenger());
        // The unset methods are h2 as in LuckPerms, which cannot run.
        c.split_storage.methods.log = "h2".into();
        assert!(open(&c).err().unwrap().contains("h2"));
        let mut c = cfg("yaml", &d);
        c.split_storage.enabled = true;
        assert!(open(&c).err().unwrap().contains("h2"));
    }
}
