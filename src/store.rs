//! The in-memory keyspace: maps keys to typed values (string, list,
//! hash, set, or sorted set), with optional per-key expiry (lazy on
//! lookup plus a periodic active sweep).
//!
//! Beside the map sit two vectors of keys - every key, and the subset
//! carrying an expiry. They exist so eviction can draw a random sample
//! in constant time: std's `HashMap` has no indexable bucket, and
//! walking the whole keyspace to pick one victim would make every
//! write under `maxmemory` cost O(N). They are maintained by
//! `insert_entry`/`remove_entry`, which are the only two places the map
//! itself grows or shrinks.

use std::collections::HashMap;
use std::time::{Instant, SystemTime};

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

/// What a key looks like to the eviction policies.
///
/// `at` is a reading of the store's logical clock rather than a wall
/// time, so LRU ordering is exact and a clock adjustment cannot reorder
/// it. `freq` is Redis's logarithmic frequency counter: it saturates at
/// 255, climbs ever more slowly, and decays with time, so a key that
/// was hot an hour ago does not outlive one that is hot now.
#[derive(Clone, Copy)]
struct Access {
    at: u64,
    freq: u8,
    /// The minute, on the store's clock, `freq` was last decayed.
    decayed_at: u32,
}

/// The counter a new key starts at. Above zero on purpose: a key that
/// was just written has no history, and starting at zero would make it
/// the first victim of the very command that created it.
const LFU_INIT: u8 = 5;

/// How steeply the counter's climb flattens out. Redis's default; not
/// exposed as a parameter, because the two knobs around it are the ones
/// nobody in practice turns.
const LFU_LOG_FACTOR: f64 = 10.0;

/// Minutes of idleness that halve the counter.
const LFU_DECAY_MINUTES: u32 = 1;

/// How many draws RANDOMKEY makes before reporting the database empty.
/// A draw can land on a key whose TTL has passed but which the sweep
/// has not reached, and a handful of retries makes that vanishingly
/// unlikely to be the whole answer.
const RANDOM_KEY_ATTEMPTS: usize = 20;

/// One key as the eviction policies see it.
pub struct Candidate {
    pub key: Bytes,
    /// The store clock when it was last accessed; smaller is older.
    pub last_access: u64,
    /// The frequency counter, already decayed to now.
    pub freq: u8,
    /// Its deadline, for `volatile-ttl`. Always `Some` for a candidate
    /// drawn from the volatile index.
    pub expire_at: Option<SystemTime>,
}

impl Access {
    fn new(clock: u64, minute: u32) -> Access {
        Access {
            at: clock,
            freq: LFU_INIT,
            decayed_at: minute,
        }
    }

    /// The counter as it stands at `minute`, halved once per
    /// `LFU_DECAY_MINUTES` of idleness. Pure, so sampling can ask
    /// without writing the answer back.
    fn decayed_freq(&self, minute: u32) -> u8 {
        let elapsed = minute.saturating_sub(self.decayed_at);
        let halvings = elapsed / LFU_DECAY_MINUTES;
        if halvings == 0 {
            return self.freq;
        }
        // Past eight halvings there is nothing left to halve.
        if halvings >= 8 {
            return 0;
        }
        self.freq >> halvings
    }

    /// Records one access: decay first, then a probabilistic increment
    /// whose odds fall as the counter rises.
    fn record_hit(&mut self, clock: u64, minute: u32) {
        self.at = clock;
        self.freq = self.decayed_freq(minute);
        self.decayed_at = minute;
        if self.freq == u8::MAX {
            return;
        }
        let base = (self.freq as f64) - (LFU_INIT as f64);
        let odds = 1.0 / (base.max(0.0) * LFU_LOG_FACTOR + 1.0);
        if crate::util::rand::unit_interval() < odds {
            self.freq += 1;
        }
    }
}

#[derive(Clone)]
struct Entry {
    value: Value,
    expire_at: Option<SystemTime>, // None = never expires
    /// Where this key sits in `Store::keys`.
    slot: usize,
    /// Where it sits in `Store::volatile`, while it has an expiry.
    volatile_slot: Option<usize>,
    access: Access,
}

pub struct Store {
    map: HashMap<Bytes, Entry>,
    /// Every key in `map`, in no particular order, for random sampling.
    keys: Vec<Bytes>,
    /// The subset of `keys` that carries an expiry, so the `volatile-*`
    /// policies sample from the keys they are allowed to evict rather
    /// than drawing from the whole keyspace and discarding most of it.
    volatile: Vec<Bytes>,
    /// Ticks once per keyspace access. Only the ordering matters.
    clock: u64,
    started: Instant,
    dirty: usize,
    expired: u64,
    lookup_hits: u64,
    lookup_misses: u64,
}

impl Store {
    pub fn new() -> Self {
        Store {
            map: HashMap::new(),
            keys: Vec::new(),
            volatile: Vec::new(),
            clock: 0,
            started: Instant::now(),
            dirty: 0,
            expired: 0,
            lookup_hits: 0,
            lookup_misses: 0,
        }
    }

    fn minute(&self) -> u32 {
        (self.started.elapsed().as_secs() / 60) as u32
    }

    /// Installs `entry` at `key`, taking over whatever was there.
    ///
    /// An overwrite keeps the old key's slots, so replacing a value
    /// does not churn either index vector; a fresh key is appended.
    fn insert_entry(&mut self, key: &[u8], mut entry: Entry) {
        match self.map.get(key) {
            Some(previous) => {
                entry.slot = previous.slot;
                entry.volatile_slot = previous.volatile_slot;
            }
            None => {
                entry.slot = self.keys.len();
                entry.volatile_slot = None;
                self.keys.push(key.to_vec());
            }
        }
        self.map.insert(key.to_vec(), entry);
        self.sync_volatile(key);
    }

    /// Builds an entry for a value that has just been created, with a
    /// fresh access record. The slots are filled in by `insert_entry`.
    fn fresh(&self, value: Value, expire_at: Option<SystemTime>) -> Entry {
        Entry {
            value,
            expire_at,
            slot: 0,
            volatile_slot: None,
            access: Access::new(self.clock, self.minute()),
        }
    }

    /// Drops `key` and repairs both index vectors.
    fn remove_entry(&mut self, key: &[u8]) -> Option<Entry> {
        let entry = self.map.remove(key)?;
        self.keys.swap_remove(entry.slot);
        // swap_remove moved the last key into the hole, unless the hole
        // was the last key - in which case there is nothing to repair.
        if let Some(moved) = self.keys.get(entry.slot).cloned() {
            if let Some(e) = self.map.get_mut(&moved) {
                e.slot = entry.slot;
            }
        }
        if let Some(slot) = entry.volatile_slot {
            self.detach_volatile(slot);
        }
        Some(entry)
    }

    /// The same repair, for the volatile index.
    fn detach_volatile(&mut self, slot: usize) {
        self.volatile.swap_remove(slot);
        if let Some(moved) = self.volatile.get(slot).cloned() {
            if let Some(e) = self.map.get_mut(&moved) {
                e.volatile_slot = Some(slot);
            }
        }
    }

    /// Brings `key`'s membership of the volatile index in line with
    /// whether it currently carries an expiry. Called wherever
    /// `expire_at` is written.
    fn sync_volatile(&mut self, key: &[u8]) {
        let Some(entry) = self.map.get(key) else {
            return;
        };
        match (entry.expire_at.is_some(), entry.volatile_slot) {
            (true, None) => {
                let slot = self.volatile.len();
                self.volatile.push(key.to_vec());
                self.map.get_mut(key).expect("just read").volatile_slot = Some(slot);
            }
            (false, Some(slot)) => {
                self.map.get_mut(key).expect("just read").volatile_slot = None;
                self.detach_volatile(slot);
            }
            _ => {}
        }
    }

    /// Records an access against the eviction policies. Every read or
    /// write that reaches a value goes through here.
    fn touch(&mut self, key: &[u8]) {
        self.clock += 1;
        let (clock, minute) = (self.clock, self.minute());
        if let Some(entry) = self.map.get_mut(key) {
            entry.access.record_hit(clock, minute);
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
            self.remove_entry(key);
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
        if found {
            self.touch(key);
        }
        self.map.get(key)
    }

    fn find_mut(&mut self, key: &[u8]) -> Option<&mut Entry> {
        self.expire_if_due(key);
        let found = self.map.contains_key(key);
        self.account(found);
        if found {
            self.touch(key);
        }
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
    /// Walks the volatile index rather than the keyspace, so it costs
    /// what it measures rather than what the whole database holds.
    pub fn volatile_size(&self) -> usize {
        let now = SystemTime::now();
        self.volatile
            .iter()
            .filter(|k| self.map.get(*k).is_some_and(|e| Self::is_live(e, now)))
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
        self.remove_entry(key);
        self.dirty += 1;
        true
    }

    /// Deletes `key` if it holds an empty collection.
    pub fn delete_if_empty(&mut self, key: &[u8]) {
        let should_delete = self
            .find(key)
            .is_some_and(|e| e.value.is_empty_collection());
        if should_delete {
            self.remove_entry(key);
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
                self.sync_volatile(key);
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
        let entry = self.remove_entry(key).expect("find() proved it is live");
        self.insert_entry(new_key, entry);
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
        self.insert_entry(dest, entry);
        self.dirty += 1;
        Some(true)
    }

    /// Drops every key. Returns how many were removed.
    pub fn flush(&mut self) -> usize {
        let removed = self.map.len();
        self.map.clear();
        self.keys.clear();
        self.volatile.clear();
        self.dirty += 1;
        removed
    }

    /// Some live key chosen pseudo-randomly, or `None` if empty.
    ///
    /// Draws from the key index rather than collecting the keyspace, so
    /// RANDOMKEY costs the same on a large database as on a small one.
    /// An expired-but-unswept key can be drawn, so a few draws are
    /// tried before giving up and reporting the database as empty.
    pub fn random_key(&mut self) -> Option<Bytes> {
        let now = SystemTime::now();
        for _ in 0..RANDOM_KEY_ATTEMPTS {
            if self.keys.is_empty() {
                return None;
            }
            let key = &self.keys[crate::util::rand::below(self.keys.len())];
            if self.map.get(key).is_some_and(|e| Self::is_live(e, now)) {
                return Some(key.clone());
            }
        }
        None
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
            self.remove_entry(&key);
            self.dirty += 1;
            self.expired += 1;
        }
    }

    /// A pseudo-random sample of live keys for the eviction policies to
    /// choose a victim from, drawn either from the whole keyspace or
    /// from the keys carrying an expiry.
    ///
    /// Sampling rather than sorting is what Redis does, and for the
    /// same reason: an exact answer would mean an ordered structure
    /// updated on every access, and the approximate answer from a
    /// handful of draws is close enough to it that the difference does
    /// not show up in a hit rate.
    ///
    /// Draws may repeat, so fewer than `count` distinct candidates can
    /// come back; that costs a little accuracy, never correctness.
    pub fn sample(&self, volatile_only: bool, count: usize) -> Vec<Candidate> {
        let pool = if volatile_only {
            &self.volatile
        } else {
            &self.keys
        };
        if pool.is_empty() || count == 0 {
            return Vec::new();
        }
        let now = SystemTime::now();
        let minute = self.minute();
        let mut sampled = Vec::with_capacity(count);
        for _ in 0..count {
            let key = &pool[crate::util::rand::below(pool.len())];
            let Some(entry) = self.map.get(key) else {
                continue;
            };
            // An expired-but-unswept key is about to go anyway, so
            // reporting it as a candidate would credit the sweep's work
            // to eviction. Skip it and let the sweep have it.
            if !Self::is_live(entry, now) {
                continue;
            }
            sampled.push(Candidate {
                key: key.clone(),
                last_access: entry.access.at,
                freq: entry.access.decayed_freq(minute),
                expire_at: entry.expire_at,
            });
        }
        sampled
    }

    /// Whether any key is eligible for `volatile-*` eviction. Lets the
    /// eviction loop give up at once rather than sampling an index it
    /// already knows is empty.
    pub fn volatile_is_empty(&self) -> bool {
        self.volatile.is_empty()
    }

    /// Drops `key` because memory ran short. Separate from `del` so it
    /// bypasses the hit/miss counters - an eviction is the server's
    /// decision, not a lookup anybody made.
    pub fn evict(&mut self, key: &[u8]) -> bool {
        if self.remove_entry(key).is_none() {
            return false;
        }
        self.dirty += 1;
        true
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
        let entry = self.fresh(Value::Str(value.to_vec()), None);
        self.insert_entry(key, entry);
    }

    /// Keeps any existing expiry, matching Redis's INCR/DECR/APPEND/
    /// SETRANGE (an in-place mutation, not a fresh SET).
    pub fn update_string(&mut self, key: &[u8], value: &[u8]) {
        self.dirty += 1;
        if let Some(e) = self.find_mut(key) {
            e.value = Value::Str(value.to_vec());
        } else {
            let entry = self.fresh(Value::Str(value.to_vec()), None);
            self.insert_entry(key, entry);
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
        let entry = self.fresh(Value::Memory(Box::new(memory)), None);
        self.insert_entry(key, entry);
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
                        let entry = self.fresh(Value::$variant($default), None);
                        self.insert_entry(key, entry);
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

    /// Both index vectors have to agree with the map after every
    /// operation, or eviction samples a key that is gone - or worse,
    /// swap-remove leaves an entry pointing at somebody else's slot.
    fn check_indexes(store: &Store) {
        assert_eq!(store.keys.len(), store.map.len(), "key index lost a key");
        for (slot, key) in store.keys.iter().enumerate() {
            let entry = store
                .map
                .get(key)
                .unwrap_or_else(|| panic!("key index holds {key:?}, the map does not"));
            assert_eq!(entry.slot, slot, "{key:?} thinks it is elsewhere");
        }

        let volatile = store.map.values().filter(|e| e.expire_at.is_some()).count();
        assert_eq!(store.volatile.len(), volatile, "volatile index is off");
        for (slot, key) in store.volatile.iter().enumerate() {
            let entry = store
                .map
                .get(key)
                .expect("volatile index holds a stray key");
            assert!(entry.expire_at.is_some(), "{key:?} has no expiry");
            assert_eq!(entry.volatile_slot, Some(slot));
        }
    }

    fn in_a_minute() -> SystemTime {
        SystemTime::now() + Duration::from_secs(60)
    }

    #[test]
    fn the_key_index_survives_inserts_overwrites_and_deletes() {
        let mut store = Store::new();
        for i in 0..8 {
            store.set_string(format!("k{i}").as_bytes(), b"v");
        }
        check_indexes(&store);

        // An overwrite must not append a second slot for the same key.
        store.set_string(b"k3", b"again");
        assert_eq!(store.keys.len(), 8);
        check_indexes(&store);

        // Deleting from the middle is the case swap_remove has to
        // repair; deleting the last key is the case it must not.
        store.del(b"k2");
        check_indexes(&store);
        store.del(b"k7");
        check_indexes(&store);
        assert_eq!(store.size(), 6);
    }

    #[test]
    fn the_volatile_index_tracks_expiry_being_set_and_cleared() {
        let mut store = Store::new();
        for i in 0..5 {
            store.set_string(format!("k{i}").as_bytes(), b"v");
        }
        assert!(store.volatile_is_empty());

        store.set_expire_at(b"k1", Some(in_a_minute()));
        store.set_expire_at(b"k3", Some(in_a_minute()));
        check_indexes(&store);
        assert_eq!(store.volatile_size(), 2);

        // PERSIST takes a key back out.
        store.set_expire_at(b"k1", None);
        check_indexes(&store);
        assert_eq!(store.volatile_size(), 1);

        // SET clears the expiry, so k3 leaves the index too.
        store.set_string(b"k3", b"fresh");
        check_indexes(&store);
        assert!(store.volatile_is_empty());
    }

    #[test]
    fn rename_copy_and_flush_leave_the_indexes_consistent() {
        let mut store = Store::new();
        store.set_string(b"src", b"v");
        store.set_expire_at(b"src", Some(in_a_minute()));
        store.set_string(b"other", b"v");

        store.copy(b"src", b"dest", false);
        check_indexes(&store);
        assert_eq!(store.volatile_size(), 2, "a copy carries the TTL across");

        store.rename(b"src", b"other");
        check_indexes(&store);
        assert_eq!(store.size(), 2);
        assert!(store.has_expiry(b"other"), "rename carries the TTL across");

        store.flush();
        check_indexes(&store);
        assert_eq!(store.size(), 0);
    }

    #[test]
    fn the_expiry_sweep_leaves_the_indexes_consistent() {
        let mut store = Store::new();
        for i in 0..6 {
            let key = format!("k{i}");
            store.set_string(key.as_bytes(), b"v");
            if i % 2 == 0 {
                // Already past, so the sweep takes it.
                store.set_expire_at(key.as_bytes(), Some(SystemTime::UNIX_EPOCH));
            }
        }
        check_indexes(&store);

        store.sweep_expired();
        check_indexes(&store);
        assert_eq!(store.size(), 3);
        assert!(store.volatile_is_empty());
    }

    #[test]
    fn sampling_draws_from_the_pool_the_policy_asked_for() {
        let mut store = Store::new();
        for i in 0..20 {
            let key = format!("k{i}");
            store.set_string(key.as_bytes(), b"v");
            if i < 5 {
                store.set_expire_at(key.as_bytes(), Some(in_a_minute()));
            }
        }

        let sampled = store.sample(false, 8);
        assert!(!sampled.is_empty());
        assert!(
            sampled.len() <= 8,
            "sampling returned more than it was asked for"
        );

        for candidate in store.sample(true, 8) {
            assert!(
                candidate.expire_at.is_some(),
                "a volatile sample returned {:?}, which has no expiry",
                candidate.key
            );
        }

        // An empty pool is not an error, it is an empty answer.
        assert!(Store::new().sample(false, 5).is_empty());
        assert!(store.sample(true, 0).is_empty());
    }

    #[test]
    fn a_read_makes_a_key_look_newer_than_one_that_was_not_read() {
        let mut store = Store::new();
        store.set_string(b"cold", b"v");
        store.set_string(b"hot", b"v");
        for _ in 0..5 {
            store.get_string(b"hot");
        }

        let sampled = store.sample(false, 32);
        let hot = sampled
            .iter()
            .find(|c| c.key == b"hot")
            .expect("hot sampled");
        let cold = sampled
            .iter()
            .find(|c| c.key == b"cold")
            .expect("cold sampled");
        assert!(
            hot.last_access > cold.last_access,
            "reading a key did not move it up the LRU order"
        );
    }

    #[test]
    fn evict_removes_a_key_without_counting_a_lookup() {
        let mut store = Store::new();
        store.set_string(b"k", b"v");
        let before = store.lookup_counts();

        assert!(store.evict(b"k"));
        assert!(
            !store.evict(b"k"),
            "evicting twice reports the second as a miss"
        );
        check_indexes(&store);
        assert_eq!(store.lookup_counts(), before, "an eviction is not a lookup");
        assert_eq!(store.size(), 0);
    }
}
