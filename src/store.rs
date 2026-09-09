//! The in-memory keyspace: maps keys to typed values (string, list,
//! hash, set, or sorted set), with optional per-key expiry (lazy on
//! lookup plus a periodic active sweep).

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use crate::types::hash::Hash;
use crate::types::list::List;
use crate::types::set::Set;
use crate::types::zset::Zset;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreType {
    String,
    List,
    Hash,
    Set,
    Zset,
}

impl StoreType {
    pub fn name(self) -> &'static str {
        match self {
            StoreType::String => "STRING",
            StoreType::List => "LIST",
            StoreType::Hash => "HASH",
            StoreType::Set => "SET",
            StoreType::Zset => "ZSET",
        }
    }
}

enum Value {
    Str(String),
    List(List),
    Hash(Hash),
    Set(Set),
    Zset(Zset),
}

impl Value {
    fn type_of(&self) -> StoreType {
        match self {
            Value::Str(_) => StoreType::String,
            Value::List(_) => StoreType::List,
            Value::Hash(_) => StoreType::Hash,
            Value::Set(_) => StoreType::Set,
            Value::Zset(_) => StoreType::Zset,
        }
    }

    /// Whether this value is an empty collection (strings are never
    /// emptied this way - matches Redis's "empty collections don't
    /// exist" for List/Hash/Set/Zset only).
    fn is_empty_collection(&self) -> bool {
        match self {
            Value::Str(_) => false,
            Value::List(l) => l.is_empty(),
            Value::Hash(h) => h.is_empty(),
            Value::Set(s) => s.is_empty(),
            Value::Zset(z) => z.size() == 0,
        }
    }
}

struct Entry {
    value: Value,
    expire_at: Option<SystemTime>, // None = never expires
}

pub struct Store {
    map: HashMap<String, Entry>,
    dirty: usize,
}

impl Store {
    pub fn new() -> Self {
        Store {
            map: HashMap::new(),
            dirty: 0,
        }
    }

    fn is_live(entry: &Entry, now: SystemTime) -> bool {
        match entry.expire_at {
            Some(t) => t > now,
            None => true,
        }
    }

    /// Finds a live (non-expired) entry for `key`, lazily erasing it if
    /// it has expired.
    fn find(&mut self, key: &str) -> Option<&Entry> {
        let now = SystemTime::now();
        let expired = self.map.get(key).is_some_and(|e| !Self::is_live(e, now));
        if expired {
            self.map.remove(key);
            self.dirty += 1;
        }
        self.map.get(key)
    }

    fn find_mut(&mut self, key: &str) -> Option<&mut Entry> {
        let now = SystemTime::now();
        let expired = self.map.get(key).is_some_and(|e| !Self::is_live(e, now));
        if expired {
            self.map.remove(key);
            self.dirty += 1;
        }
        self.map.get_mut(key)
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty
    }

    pub fn reset_dirty(&mut self) {
        self.dirty = 0;
    }

    /// Part of the generic key-operations API (kept for parity with the
    /// original), unused by any command today.
    #[allow(dead_code)]
    pub fn exists(&mut self, key: &str) -> bool {
        self.find(key).is_some()
    }

    pub fn type_of(&mut self, key: &str) -> Option<StoreType> {
        self.find(key).map(|e| e.value.type_of())
    }

    pub fn del(&mut self, key: &str) -> bool {
        if self.find(key).is_none() {
            return false; // also lazily expires
        }
        self.map.remove(key);
        self.dirty += 1;
        true
    }

    /// Deletes `key` if it holds an empty collection.
    pub fn delete_if_empty(&mut self, key: &str) {
        let should_delete = self
            .find(key)
            .is_some_and(|e| e.value.is_empty_collection());
        if should_delete {
            self.map.remove(key);
            self.dirty += 1;
        }
    }

    pub fn expire(&mut self, key: &str, seconds: i64) -> bool {
        let expire_at = expire_at_from_secs(seconds);
        match self.find_mut(key) {
            Some(e) => {
                e.expire_at = Some(expire_at);
                self.dirty += 1;
                true
            }
            None => false,
        }
    }

    /// -2 missing, -1 no expiry, else seconds left.
    pub fn ttl(&mut self, key: &str) -> i64 {
        match self.find(key) {
            None => -2,
            Some(e) => match e.expire_at {
                None => -1,
                Some(t) => match t.duration_since(SystemTime::now()) {
                    Ok(d) => d.as_secs() as i64,
                    Err(_) => 0,
                },
            },
        }
    }

    /// Raw key count, including any not-yet-swept expired keys (matches
    /// the original's DBSIZE, which never filtered on expiry either).
    pub fn size(&self) -> usize {
        self.map.len()
    }

    pub fn sweep_expired(&mut self) {
        let now = SystemTime::now();
        let expired: Vec<String> = self
            .map
            .iter()
            .filter(|(_, e)| !Self::is_live(e, now))
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            self.map.remove(&key);
            self.dirty += 1;
        }
    }

    /// Every live key, unfiltered (KEYS applies its own glob filter).
    pub fn foreach_key(&self) -> Vec<String> {
        let now = SystemTime::now();
        self.map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now))
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// Resumable key iteration for SCAN: returns up to `min_count` live
    /// keys starting at `start_cursor` (a position in a stable sorted
    /// snapshot of the keyspace, not a hashtable bucket index like the
    /// C version - std's HashMap doesn't expose one) plus the cursor to
    /// resume from, or `0` once the whole keyspace has been covered.
    /// Like the original, concurrent inserts/deletes between calls can
    /// still cause a key to be skipped or repeated; fine for
    /// interactive/dev use.
    pub fn scan(&self, start_cursor: usize, min_count: usize) -> (Vec<String>, usize) {
        let now = SystemTime::now();
        let mut keys: Vec<&String> = self
            .map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now))
            .map(|(k, _)| k)
            .collect();
        keys.sort();

        if start_cursor >= keys.len() {
            return (Vec::new(), 0);
        }

        let end = (start_cursor + min_count.max(1)).min(keys.len());
        let batch = keys[start_cursor..end]
            .iter()
            .map(|s| (*s).clone())
            .collect();
        let next = if end >= keys.len() { 0 } else { end };
        (batch, next)
    }

    /// Every live (key, type) pair - the hook persistence uses to dump
    /// the whole keyspace.
    pub fn foreach_entry(&self) -> Vec<(String, StoreType)> {
        let now = SystemTime::now();
        self.map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now))
            .map(|(k, e)| (k.clone(), e.value.type_of()))
            .collect()
    }

    /// Clears any existing expiry, matching Redis's SET - always
    /// overwrites regardless of the key's previous type.
    pub fn set_string(&mut self, key: &str, value: &str) {
        self.dirty += 1;
        self.map.insert(
            key.to_string(),
            Entry {
                value: Value::Str(value.to_string()),
                expire_at: None,
            },
        );
    }

    /// Keeps any existing expiry, matching Redis's INCR/DECR/APPEND/
    /// SETRANGE (an in-place mutation, not a fresh SET).
    pub fn update_string(&mut self, key: &str, value: &str) {
        self.dirty += 1;
        if let Some(e) = self.find_mut(key) {
            e.value = Value::Str(value.to_string());
        } else {
            self.map.insert(
                key.to_string(),
                Entry {
                    value: Value::Str(value.to_string()),
                    expire_at: None,
                },
            );
        }
    }

    /// `None` if missing or the wrong type.
    pub fn get_string(&mut self, key: &str) -> Option<String> {
        match self.find(key) {
            Some(Entry {
                value: Value::Str(s),
                ..
            }) => Some(s.clone()),
            _ => None,
        }
    }
}

fn expire_at_from_secs(seconds: i64) -> SystemTime {
    let now = SystemTime::now();
    if seconds >= 0 {
        now + Duration::from_secs(seconds as u64)
    } else {
        now.checked_sub(Duration::from_secs((-seconds) as u64))
            .unwrap_or(SystemTime::UNIX_EPOCH)
    }
}

/// Defines `get_or_create_<field>`/`get_existing_<field>` pairs.
/// "get_or_create" makes a new empty collection if the key is absent;
/// "get_existing" never creates. Both return `None` if the key holds a
/// different type.
macro_rules! define_collection_accessors {
    ($get_or_create:ident, $get_existing:ident, $variant:ident, $ty:ty, $default:expr) => {
        impl Store {
            pub fn $get_or_create(&mut self, key: &str) -> Option<&mut $ty> {
                self.dirty += 1; // every caller is about to mutate the result
                match self.find(key) {
                    Some(e) => {
                        if !matches!(e.value, Value::$variant(_)) {
                            return None;
                        }
                    }
                    None => {
                        self.map.insert(
                            key.to_string(),
                            Entry {
                                value: Value::$variant($default),
                                expire_at: None,
                            },
                        );
                    }
                }
                match &mut self.map.get_mut(key).unwrap().value {
                    Value::$variant(v) => Some(v),
                    _ => unreachable!(),
                }
            }

            pub fn $get_existing(&mut self, key: &str) -> Option<&mut $ty> {
                match self.find_mut(key) {
                    Some(e) => match &mut e.value {
                        Value::$variant(v) => Some(v),
                        _ => None,
                    },
                    None => None,
                }
            }
        }
    };
}

define_collection_accessors!(
    get_or_create_list,
    get_existing_list,
    List,
    List,
    List::new()
);
define_collection_accessors!(
    get_or_create_hash,
    get_existing_hash,
    Hash,
    Hash,
    Hash::new()
);
define_collection_accessors!(get_or_create_set, get_existing_set, Set, Set, Set::new());
define_collection_accessors!(
    get_or_create_zset,
    get_existing_zset,
    Zset,
    Zset,
    Zset::new()
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_get_string() {
        let mut store = Store::new();
        store.set_string("k", "v");
        assert_eq!(store.get_string("k"), Some("v".to_string()));
        assert_eq!(store.type_of("k"), Some(StoreType::String));
    }

    #[test]
    fn set_always_overwrites_regardless_of_type() {
        let mut store = Store::new();
        store.get_or_create_list("k");
        store.set_string("k", "now-a-string");
        assert_eq!(store.type_of("k"), Some(StoreType::String));
    }

    #[test]
    fn set_clears_previous_expiry() {
        let mut store = Store::new();
        store.set_string("k", "v");
        store.expire("k", 100);
        store.set_string("k", "v2");
        assert_eq!(store.ttl("k"), -1);
    }

    #[test]
    fn update_string_preserves_expiry() {
        let mut store = Store::new();
        store.set_string("k", "5");
        store.expire("k", 200);
        store.update_string("k", "6");
        assert!(store.ttl("k") > 0);
    }

    #[test]
    fn missing_key_ttl_is_minus_two() {
        let mut store = Store::new();
        assert_eq!(store.ttl("nope"), -2);
    }

    #[test]
    fn wrong_type_accessor_returns_none() {
        let mut store = Store::new();
        store.set_string("k", "v");
        assert!(store.get_or_create_list("k").is_none());
    }

    #[test]
    fn delete_if_empty_removes_only_empty_collections() {
        let mut store = Store::new();
        let list = store.get_or_create_list("k").unwrap();
        list.push_back("only".to_string());
        list.pop_front();
        store.delete_if_empty("k");
        assert_eq!(store.type_of("k"), None);
    }

    #[test]
    fn scan_covers_everything_without_duplicates() {
        let mut store = Store::new();
        for i in 0..6 {
            store.set_string(&format!("k{i}"), "v");
        }
        let mut seen = std::collections::HashSet::new();
        let mut cursor = 0;
        loop {
            let (batch, next) = store.scan(cursor, 2);
            for k in batch {
                assert!(seen.insert(k), "SCAN re-emitted a key mid-iteration");
            }
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        assert_eq!(seen.len(), 6);
    }
}
