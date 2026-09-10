//! Saves/loads the whole keyspace to a single dump file on disk, so data
//! survives a restart. Text format, byte-compatible with the original C
//! implementation: a `KLYRO-DUMP 1` header line, then one record per
//! key (`STRING key value`, or `LIST|HASH|SET|ZSET key count` followed
//! by `count` data lines), plus an optional `EXPIREAT key
//! unix-timestamp` line after a key's record if it has a TTL. Saves
//! are atomic (written to `<path>.tmp`, then renamed over the real
//! path).

use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Lines, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::store::{Store, StoreType};
use crate::util::strutil::{format_g, next_token, parse_int, parse_long, trim};

const DUMP_MAGIC: &str = "KLYRO-DUMP 1";

pub struct Persist {
    path: PathBuf,
    last_check: Option<Instant>,
}

impl Persist {
    pub fn new(path: &str) -> Self {
        Persist {
            path: PathBuf::from(path),
            last_check: None,
        }
    }

    /// Loads the dump file at the configured path into `store`, if one
    /// exists. Call once at startup, before accepting connections.
    pub fn load(&self, store: &mut Store) {
        let file = match File::open(&self.path) {
            Ok(f) => f,
            Err(_) => return, // no dump yet; nothing to load
        };
        let mut lines = BufReader::new(file).lines();

        match lines.next() {
            Some(Ok(first)) if first.starts_with("KLYRO-DUMP") => {}
            _ => {
                eprintln!(
                    "persist_load: {} is not a valid Klyro dump, ignoring",
                    self.path.display()
                );
                return;
            }
        }

        let mut loaded = 0usize;
        while let Some(Ok(raw_line)) = lines.next() {
            let mut rest = trim(&raw_line);
            let type_tag = next_token(&mut rest);
            let key = next_token(&mut rest);
            let (type_tag, key) = match (type_tag, key) {
                (Some(t), Some(k)) => (t, k),
                _ => continue,
            };

            match type_tag {
                "STRING" => {
                    store.set_string(key, rest);
                    loaded += 1;
                }
                "LIST" => {
                    if let Some(count) = parse_int(rest) {
                        Self::load_list(&mut lines, store, key, count);
                        loaded += 1;
                    }
                }
                "HASH" => {
                    if let Some(count) = parse_int(rest) {
                        Self::load_hash(&mut lines, store, key, count);
                        loaded += 1;
                    }
                }
                "SET" => {
                    if let Some(count) = parse_int(rest) {
                        Self::load_set(&mut lines, store, key, count);
                        loaded += 1;
                    }
                }
                "ZSET" => {
                    if let Some(count) = parse_int(rest) {
                        Self::load_zset(&mut lines, store, key, count);
                        loaded += 1;
                    }
                }
                "EXPIREAT" => {
                    if let Some(expire_at) = parse_long(rest) {
                        let now = unix_now();
                        let remaining = expire_at - now;
                        if remaining <= 0 {
                            store.del(key);
                        } else {
                            store.expire(key, remaining);
                        }
                    }
                }
                _ => {}
            }
        }

        println!("loaded {} key(s) from {}", loaded, self.path.display());
    }

    fn load_list(lines: &mut Lines<BufReader<File>>, store: &mut Store, key: &str, count: i32) {
        let count = count.max(0) as usize;
        match store.get_or_create_list(key) {
            Some(list) => {
                for _ in 0..count {
                    match lines.next() {
                        Some(Ok(line)) => list.push_back(trim(&line).to_string()),
                        _ => break,
                    }
                }
            }
            None => {
                for _ in 0..count {
                    if lines.next().is_none() {
                        break;
                    }
                }
            }
        }
    }

    fn load_hash(lines: &mut Lines<BufReader<File>>, store: &mut Store, key: &str, count: i32) {
        let count = count.max(0) as usize;
        match store.get_or_create_hash(key) {
            Some(hash) => {
                for _ in 0..count {
                    match lines.next() {
                        Some(Ok(line)) => {
                            let mut rest = trim(&line);
                            if let Some(field) = next_token(&mut rest) {
                                hash.insert(field.to_string(), rest.to_string());
                            }
                        }
                        _ => break,
                    }
                }
            }
            None => {
                for _ in 0..count {
                    if lines.next().is_none() {
                        break;
                    }
                }
            }
        }
    }

    fn load_set(lines: &mut Lines<BufReader<File>>, store: &mut Store, key: &str, count: i32) {
        let count = count.max(0) as usize;
        match store.get_or_create_set(key) {
            Some(set) => {
                for _ in 0..count {
                    match lines.next() {
                        Some(Ok(line)) => {
                            set.insert(trim(&line).to_string());
                        }
                        _ => break,
                    }
                }
            }
            None => {
                for _ in 0..count {
                    if lines.next().is_none() {
                        break;
                    }
                }
            }
        }
    }

    fn load_zset(lines: &mut Lines<BufReader<File>>, store: &mut Store, key: &str, count: i32) {
        let count = count.max(0) as usize;
        match store.get_or_create_zset(key) {
            Some(zset) => {
                for _ in 0..count {
                    match lines.next() {
                        Some(Ok(line)) => {
                            let mut rest = trim(&line);
                            let member = next_token(&mut rest);
                            if let (Some(member), Some(score)) =
                                (member, crate::util::strutil::parse_double(rest))
                            {
                                zset.add(member, score);
                            }
                        }
                        _ => break,
                    }
                }
            }
            None => {
                for _ in 0..count {
                    if lines.next().is_none() {
                        break;
                    }
                }
            }
        }
    }

    /// Writes the whole keyspace to `path`, atomically, by writing a
    /// temp file and renaming it over the target. `path` becomes this
    /// instance's path from now on, which is how `CONFIG SET
    /// dbfilename` redirects the next save. Returns whether the write
    /// succeeded.
    pub fn save_to(&mut self, path: &str, store: &mut Store) -> bool {
        self.path = PathBuf::from(path);
        match self.save_inner(store) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("persist_save: {}", e);
                false
            }
        }
    }

    fn save_inner(&mut self, store: &mut Store) -> io::Result<()> {
        let mut tmp_path = self.path.clone().into_os_string();
        tmp_path.push(".tmp");
        let tmp_path = PathBuf::from(tmp_path);

        let mut f = File::create(&tmp_path)?;
        writeln!(f, "{}", DUMP_MAGIC)?;

        for (key, ty) in store.foreach_entry() {
            match ty {
                StoreType::String => {
                    let value = store.get_string(&key).unwrap_or_default();
                    writeln!(f, "STRING {} {}", key, value)?;
                }
                StoreType::List => {
                    if let Some(list) = store.get_existing_list(&key) {
                        writeln!(f, "LIST {} {}", key, list.len())?;
                        for v in list.iter() {
                            writeln!(f, "{}", v)?;
                        }
                    }
                }
                StoreType::Hash => {
                    if let Some(hash) = store.get_existing_hash(&key) {
                        writeln!(f, "HASH {} {}", key, hash.len())?;
                        for (field, v) in hash.iter() {
                            writeln!(f, "{} {}", field, v)?;
                        }
                    }
                }
                StoreType::Set => {
                    if let Some(set) = store.get_existing_set(&key) {
                        writeln!(f, "SET {} {}", key, set.len())?;
                        for m in set.iter() {
                            writeln!(f, "{}", m)?;
                        }
                    }
                }
                StoreType::Zset => {
                    if let Some(zset) = store.get_existing_zset(&key) {
                        writeln!(f, "ZSET {} {}", key, zset.size())?;
                        for (member, score) in zset.iter() {
                            writeln!(f, "{} {}", member, format_g(score, 17))?;
                        }
                    }
                }
            }

            let ttl = store.ttl(&key);
            if ttl >= 0 {
                writeln!(f, "EXPIREAT {} {}", key, unix_now() + ttl)?;
            }
        }

        drop(f);
        fs::rename(&tmp_path, &self.path)?;
        store.reset_dirty();
        Ok(())
    }

    /// Whether `interval` has elapsed since the last check *and* the
    /// store has unsaved changes. Advances the check clock as a side
    /// effect, so the caller gets one `true` per interval rather than
    /// one per tick.
    pub fn autosave_due(&mut self, interval: Duration, store: &Store) -> bool {
        let now = Instant::now();
        match self.last_check {
            None => {
                // Start the clock rather than firing immediately: at
                // startup nothing has changed yet anyway.
                self.last_check = Some(now);
                return false;
            }
            Some(last) if now.duration_since(last) < interval => return false,
            Some(_) => self.last_check = Some(now),
        }
        store.dirty_count() > 0
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "klyro_persist_test_{}_{}",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn round_trips_all_types_and_ttl() {
        let path = temp_path("roundtrip");
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(format!("{}.tmp", path.display()));

        let mut store = Store::new();
        store.set_string("greeting", "hello persistence");
        store
            .get_or_create_list("mylist")
            .unwrap()
            .push_back("a".into());
        store
            .get_or_create_hash("user")
            .unwrap()
            .insert("name".into(), "Alice".into());
        store
            .get_or_create_set("tags")
            .unwrap()
            .insert("fast".into());
        store
            .get_or_create_zset("board")
            .unwrap()
            .add("alice", 100.0);
        store.expire("greeting", 300);

        let mut persist = Persist::new(path.to_str().unwrap());
        assert!(persist.save_to(path.to_str().unwrap(), &mut store));

        let mut reloaded = Store::new();
        let persist2 = Persist::new(path.to_str().unwrap());
        persist2.load(&mut reloaded);

        assert_eq!(
            reloaded.get_string("greeting"),
            Some("hello persistence".to_string())
        );
        assert!(reloaded.ttl("greeting") > 290);
        assert_eq!(
            reloaded
                .get_existing_list("mylist")
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            vec!["a"]
        );
        assert_eq!(
            reloaded
                .get_existing_hash("user")
                .unwrap()
                .get("name")
                .unwrap(),
            "Alice"
        );
        assert!(reloaded.get_existing_set("tags").unwrap().contains("fast"));
        assert_eq!(
            reloaded.get_existing_zset("board").unwrap().score("alice"),
            Some(100.0)
        );

        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(format!("{}.tmp", path.display()));
    }
}
