//! The in-memory keyspace: maps keys to typed values (string, list,
//! hash, set, or sorted set), with optional per-key expiry (lazy on
//! lookup plus a periodic active sweep).

use std::collections::HashMap;
use std::time::SystemTime;

use crate::types::hash::Hash;
use crate::types::list::List;
use crate::types::memory::Memory;
use crate::types::set::Set;
use crate::types::zset::Zset;
use crate::util::bytes::Bytes;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreType {
    String,
    List,
    Hash,
    Set,
    Zset,
    Memory,
}

impl StoreType {
    /// The name Redis's TYPE command replies with - lowercase, and the
    /// same spelling INFO's keyspace breakdown uses.
    pub fn name(self) -> &'static str {
        match self {
            StoreType::String => "string",
            StoreType::List => "list",
            StoreType::Hash => "hash",
            StoreType::Set => "set",
            StoreType::Zset => "zset",
            StoreType::Memory => "memory",
        }
    }
}

#[derive(Clone)]
enum Value {
    Str(Bytes),
    List(List),
    Hash(Hash),
    Set(Set),
    Zset(Zset),
    Memory(Box<Memory>),
}

impl Value {
    fn type_of(&self) -> StoreType {
        match self {
            Value::Str(_) => StoreType::String,
            Value::List(_) => StoreType::List,
            Value::Hash(_) => StoreType::Hash,
            Value::Set(_) => StoreType::Set,
            Value::Zset(_) => StoreType::Zset,
            Value::Memory(_) => StoreType::Memory,
        }
    }

    /// Whether this value is an empty collection (strings are never
    /// emptied this way - matches Redis's "empty collections don't
    /// exist" for List/Hash/Set/Zset only).
    ///
    /// A memory index is never empty in this sense. It carries a mode,
    /// a dimension, and a metric that took a `MEM.CREATE` to establish,
    /// so emptying it of records must not silently discard that.
    fn is_empty_collection(&self) -> bool {
        match self {
            Value::Str(_) | Value::Memory(_) => false,
            Value::List(l) => l.is_empty(),
            Value::Hash(h) => h.is_empty(),
            Value::Set(s) => s.is_empty(),
            Value::Zset(z) => z.size() == 0,
        }
    }
}

#[derive(Clone)]
struct Entry {
    value: Value,
    expire_at: Option<SystemTime>, // None = never expires
}

pub struct Store {
    map: HashMap<Bytes, Entry>,
    dirty: usize,
    expired: u64,
    lookup_hits: u64,
    lookup_misses: u64,
}

impl Store {
    pub fn new() -> Self {
        Store {
            map: HashMap::new(),
            dirty: 0,
            expired: 0,
            lookup_hits: 0,
            lookup_misses: 0,
        }
    }

    fn is_live(entry: &Entry, now: SystemTime) -> bool {
        match entry.expire_at {
            Some(t) => t > now,
            None => true,
        }
    }

    /// Drops `key` if its deadline has passed, so a lookup never sees a
    /// stale entry. Shared by `find`/`find_mut`.
    fn expire_if_due(&mut self, key: &[u8]) {
        let now = SystemTime::now();
        if self.map.get(key).is_some_and(|e| !Self::is_live(e, now)) {
            self.map.remove(key);
            self.dirty += 1;
            self.expired += 1;
        }
    }

    /// Records whether a lookup found anything, for INFO's hit ratio.
    fn account(&mut self, found: bool) {
        if found {
            self.lookup_hits += 1;
        } else {
            self.lookup_misses += 1;
        }
    }

    /// Finds a live (non-expired) entry for `key`, lazily erasing it if
    /// it has expired.
    fn find(&mut self, key: &[u8]) -> Option<&Entry> {
        self.expire_if_due(key);
        let found = self.map.contains_key(key);
        self.account(found);
        self.map.get(key)
    }

    fn find_mut(&mut self, key: &[u8]) -> Option<&mut Entry> {
        self.expire_if_due(key);
        let found = self.map.contains_key(key);
        self.account(found);
        self.map.get_mut(key)
    }

    /// Keyspace lookups so far, as (hits, misses). The dispatcher reads
    /// the delta across a single command so it can attribute only
    /// read-command lookups to INFO's counters.
    pub fn lookup_counts(&self) -> (u64, u64) {
        (self.lookup_hits, self.lookup_misses)
    }

    /// Keys removed because their TTL passed, whether by the periodic
    /// sweep or lazily on lookup.
    pub fn expired_count(&self) -> u64 {
        self.expired
    }

    /// How many live keys carry an expiry, for INFO's keyspace line.
    pub fn volatile_size(&self) -> usize {
        let now = SystemTime::now();
        self.map
            .values()
            .filter(|e| Self::is_live(e, now) && e.expire_at.is_some())
            .count()
    }

    /// Live key counts broken down by type, in `StoreType` order.
    pub fn type_breakdown(&self) -> Vec<(StoreType, usize)> {
        let now = SystemTime::now();
        let mut counts = [0usize; 6];
        for entry in self.map.values().filter(|e| Self::is_live(e, now)) {
            counts[entry.value.type_of() as usize] += 1;
        }
        [
            StoreType::String,
            StoreType::List,
            StoreType::Hash,
            StoreType::Set,
            StoreType::Zset,
            StoreType::Memory,
        ]
        .into_iter()
        .map(|t| (t, counts[t as usize]))
        .collect()
    }

    pub fn dirty_count(&self) -> usize {
        self.dirty
    }

    pub fn reset_dirty(&mut self) {
        self.dirty = 0;
    }

    pub fn exists(&mut self, key: &[u8]) -> bool {
        self.find(key).is_some()
    }

    /// The type `key` holds, counted as a keyspace lookup. For the
    /// TYPE command and anything else a user asked for directly.
    pub fn type_of(&mut self, key: &[u8]) -> Option<StoreType> {
        self.find(key).map(|e| e.value.type_of())
    }

    /// The same answer, without touching the hit/miss counters. For
    /// internal type checks, which would otherwise make every read
    /// command look like two keyspace lookups instead of one.
    pub fn peek_type(&mut self, key: &[u8]) -> Option<StoreType> {
        self.expire_if_due(key);
        self.map.get(key).map(|e| e.value.type_of())
    }

    pub fn del(&mut self, key: &[u8]) -> bool {
        if self.find(key).is_none() {
            return false; // also lazily expires
        }
        self.map.remove(key);
        self.dirty += 1;
        true
    }

    /// Deletes `key` if it holds an empty collection.
    pub fn delete_if_empty(&mut self, key: &[u8]) {
        let should_delete = self
            .find(key)
            .is_some_and(|e| e.value.is_empty_collection());
        if should_delete {
            self.map.remove(key);
            self.dirty += 1;
        }
    }

    /// Sets (or with `None`, clears) `key`'s expiry deadline. Returns
    /// `false` if the key doesn't exist. Backs EXPIRE, PEXPIRE,
    /// EXPIREAT, PEXPIREAT, PERSIST, and SET's EX/PX options.
    pub fn set_expire_at(&mut self, key: &[u8], at: Option<SystemTime>) -> bool {
        match self.find_mut(key) {
            Some(e) => {
                e.expire_at = at;
                self.dirty += 1;
                true
            }
            None => false,
        }
    }

    /// Whether `key` currently has an expiry set. `false` if missing.
    pub fn has_expiry(&mut self, key: &[u8]) -> bool {
        self.find(key).is_some_and(|e| e.expire_at.is_some())
    }

    /// -2 missing, -1 no expiry, else seconds left.
    pub fn ttl(&mut self, key: &[u8]) -> i64 {
        match self.pttl_ms(key) {
            n if n < 0 => n,
            ms => ms / 1000,
        }
    }

    /// -2 missing, -1 no expiry, else milliseconds left.
    pub fn pttl_ms(&mut self, key: &[u8]) -> i64 {
        match self.find(key) {
            None => -2,
            Some(e) => match e.expire_at {
                None => -1,
                Some(t) => match t.duration_since(SystemTime::now()) {
                    Ok(d) => d.as_millis() as i64,
                    Err(_) => 0,
                },
            },
        }
    }

    /// Moves `key` to `new_key`, replacing whatever was there and
    /// carrying the TTL across. `false` if `key` doesn't exist.
    pub fn rename(&mut self, key: &[u8], new_key: &[u8]) -> bool {
        if self.find(key).is_none() {
            return false;
        }
        if key == new_key {
            return true;
        }
        let entry = self.map.remove(key).expect("find() proved it is live");
        self.map.insert(new_key.to_vec(), entry);
        self.dirty += 1;
        true
    }

    /// Deep-copies `key` to `dest` (TTL included). `None` if `key` is
    /// missing, `Some(false)` if `dest` already exists and `replace` is
    /// unset, `Some(true)` on success.
    pub fn copy(&mut self, key: &[u8], dest: &[u8], replace: bool) -> Option<bool> {
        self.find(key)?;
        if !replace && self.exists(dest) {
            return Some(false);
        }
        let entry = self.map.get(key).expect("find() proved it is live").clone();
        self.map.insert(dest.to_vec(), entry);
        self.dirty += 1;
        Some(true)
    }

    /// Drops every key. Returns how many were removed.
    pub fn flush(&mut self) -> usize {
        let removed = self.map.len();
        self.map.clear();
        self.dirty += 1;
        removed
    }

    /// Some live key chosen pseudo-randomly, or `None` if empty.
    pub fn random_key(&mut self) -> Option<Bytes> {
        let now = SystemTime::now();
        let live: Vec<&Bytes> = self
            .map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now))
            .map(|(k, _)| k)
            .collect();
        if live.is_empty() {
            return None;
        }
        Some(live[crate::util::rand::below(live.len())].clone())
    }

    /// Live key count, skipping any expired-but-not-yet-swept entries so
    /// DBSIZE always agrees with what KEYS would return.
    pub fn size(&self) -> usize {
        let now = SystemTime::now();
        self.map.values().filter(|e| Self::is_live(e, now)).count()
    }

    pub fn sweep_expired(&mut self) {
        let now = SystemTime::now();
        let expired: Vec<Bytes> = self
            .map
            .iter()
            .filter(|(_, e)| !Self::is_live(e, now))
            .map(|(k, _)| k.clone())
            .collect();
        for key in expired {
            self.map.remove(&key);
            self.dirty += 1;
            self.expired += 1;
        }
    }

    /// Every live key, unfiltered (KEYS applies its own glob filter).
    pub fn foreach_key(&self) -> Vec<Bytes> {
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
    pub fn scan(&self, start_cursor: usize, min_count: usize) -> (Vec<Bytes>, usize) {
        let now = SystemTime::now();
        let mut keys: Vec<&Bytes> = self
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
    pub fn foreach_entry(&self) -> Vec<(Bytes, StoreType)> {
        let now = SystemTime::now();
        self.map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now))
            .map(|(k, e)| (k.clone(), e.value.type_of()))
            .collect()
    }

    /// Clears any existing expiry, matching Redis's SET - always
    /// overwrites regardless of the key's previous type.
    pub fn set_string(&mut self, key: &[u8], value: &[u8]) {
        self.dirty += 1;
        self.map.insert(
            key.to_vec(),
            Entry {
                value: Value::Str(value.to_vec()),
                expire_at: None,
            },
        );
    }

    /// Keeps any existing expiry, matching Redis's INCR/DECR/APPEND/
    /// SETRANGE (an in-place mutation, not a fresh SET).
    pub fn update_string(&mut self, key: &[u8], value: &[u8]) {
        self.dirty += 1;
        if let Some(e) = self.find_mut(key) {
            e.value = Value::Str(value.to_vec());
        } else {
            self.map.insert(
                key.to_vec(),
                Entry {
                    value: Value::Str(value.to_vec()),
                    expire_at: None,
                },
            );
        }
    }

    /// `None` if missing or the wrong type.
    pub fn get_string(&mut self, key: &[u8]) -> Option<Bytes> {
        match self.find(key) {
            Some(Entry {
                value: Value::Str(s),
                ..
            }) => Some(s.clone()),
            _ => None,
        }
    }
}

impl Store {
    /// Marks the keyspace changed, so autosave notices.
    ///
    /// Most types record this inside `get_or_create_*`, which is only
    /// ever called to mutate. A memory index is reached through
    /// `get_existing_memory`, which cannot tell `MEM.GET` from
    /// `MEM.ADD`, so its mutating commands say so explicitly rather
    /// than having every read look like a write.
    pub fn mark_dirty(&mut self) {
        self.dirty += 1;
    }

    /// Installs a new memory index at `key`. Returns `false` without
    /// touching anything if the key is already taken - `MEM.CREATE`
    /// never silently replaces an index, because doing so would drop
    /// every record in it.
    pub fn create_memory(&mut self, key: &[u8], memory: Memory) -> bool {
        if self.exists(key) {
            return false;
        }
        self.dirty += 1;
        self.map.insert(
            key.to_vec(),
            Entry {
                value: Value::Memory(Box::new(memory)),
                expire_at: None,
            },
        );
        true
    }

    /// `None` if the key is missing or holds another type. Never
    /// creates: an index needs a mode and a dimension, which only
    /// `MEM.CREATE` can supply.
    pub fn get_existing_memory(&mut self, key: &[u8]) -> Option<&mut Memory> {
        match self.find_mut(key) {
            Some(e) => match &mut e.value {
                Value::Memory(m) => Some(m),
                _ => None,
            },
            None => None,
        }
    }

    /// Every live memory index, for the periodic record sweep and for
    /// INFO's totals. Takes `&mut` because reaching a value means
    /// walking the same expiry check every lookup does.
    pub fn memory_keys(&self) -> Vec<Bytes> {
        let now = SystemTime::now();
        self.map
            .iter()
            .filter(|(_, e)| Self::is_live(e, now) && e.value.type_of() == StoreType::Memory)
            .map(|(k, _)| k.clone())
            .collect()
    }
}

/// Defines `get_or_create_<field>`/`get_existing_<field>` pairs.
/// "get_or_create" makes a new empty collection if the key is absent;
/// "get_existing" never creates. Both return `None` if the key holds a
/// different type.
macro_rules! define_collection_accessors {
    ($get_or_create:ident, $get_existing:ident, $variant:ident, $ty:ty, $default:expr) => {
        impl Store {
            pub fn $get_or_create(&mut self, key: &[u8]) -> Option<&mut $ty> {
                self.dirty += 1; // every caller is about to mutate the result
                match self.find(key) {
                    Some(e) => {
                        if !matches!(e.value, Value::$variant(_)) {
                            return None;
                        }
                    }
                    None => {
                        self.map.insert(
                            key.to_vec(),
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

            pub fn $get_existing(&mut self, key: &[u8]) -> Option<&mut $ty> {
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
    use std::time::Duration;

    /// Sets a TTL `seconds` from now - the convenience the commands
    /// layer builds for itself out of `set_expire_at`.
    fn expire(store: &mut Store, key: &[u8], seconds: u64) -> bool {
        store.set_expire_at(key, Some(SystemTime::now() + Duration::from_secs(seconds)))
    }

    #[test]
    fn set_and_get_string() {
        let mut store = Store::new();
        store.set_string(b"k", b"v");
        assert_eq!(store.get_string(b"k"), Some(b"v".to_vec()));
        assert_eq!(store.type_of(b"k"), Some(StoreType::String));
    }

    #[test]
    fn set_always_overwrites_regardless_of_type() {
        let mut store = Store::new();
        store.get_or_create_list(b"k");
        store.set_string(b"k", b"now-a-string");
        assert_eq!(store.type_of(b"k"), Some(StoreType::String));
    }

    #[test]
    fn set_clears_previous_expiry() {
        let mut store = Store::new();
        store.set_string(b"k", b"v");
        expire(&mut store, b"k", 100);
        store.set_string(b"k", b"v2");
        assert_eq!(store.ttl(b"k"), -1);
    }

    #[test]
    fn update_string_preserves_expiry() {
        let mut store = Store::new();
        store.set_string(b"k", b"5");
        expire(&mut store, b"k", 200);
        store.update_string(b"k", b"6");
        assert!(store.ttl(b"k") > 0);
    }

    #[test]
    fn missing_key_ttl_is_minus_two() {
        let mut store = Store::new();
        assert_eq!(store.ttl(b"nope"), -2);
    }

    #[test]
    fn wrong_type_accessor_returns_none() {
        let mut store = Store::new();
        store.set_string(b"k", b"v");
        assert!(store.get_or_create_list(b"k").is_none());
    }

    #[test]
    fn delete_if_empty_removes_only_empty_collections() {
        let mut store = Store::new();
        let list = store.get_or_create_list(b"k").unwrap();
        list.push_back(b"only".to_vec());
        list.pop_front();
        store.delete_if_empty(b"k");
        assert_eq!(store.type_of(b"k"), None);
    }

    #[test]
    fn scan_covers_everything_without_duplicates() {
        let mut store = Store::new();
        for i in 0..6 {
            store.set_string(format!("k{i}").as_bytes(), b"v");
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
