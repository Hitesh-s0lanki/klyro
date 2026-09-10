//! The watched-key registry behind WATCH and EXEC.
//!
//! Optimistic locking needs one question answered: has anything
//! touched this key since the client watched it? Redis answers it by
//! marking every watching client dirty at the moment of the write,
//! which means a write has to reach into other connections. Klyro
//! keeps a version counter per watched key instead: WATCH records the
//! version it saw, a write bumps it, and EXEC compares. Nothing but
//! the transaction's own connection is ever touched.
//!
//! Only watched keys are tracked. An entry appears on the first WATCH
//! and goes away when the last watcher drops it, so the map holds
//! nothing at all in the common case where no transaction is open.

use std::collections::HashMap;

use crate::util::bytes::Bytes;

struct Watched {
    version: u64,
    watchers: usize,
}

#[derive(Default)]
pub struct Watch {
    keys: HashMap<Bytes, Watched>,
}

impl Watch {
    pub fn new() -> Watch {
        Watch::default()
    }

    /// Registers a watcher on `key` and returns the version to compare
    /// against later.
    pub fn watch(&mut self, key: &[u8]) -> u64 {
        let entry = self.keys.entry(key.to_vec()).or_insert(Watched {
            version: 0,
            watchers: 0,
        });
        entry.watchers += 1;
        entry.version
    }

    /// Drops one watcher from `key`, forgetting the key entirely once
    /// the last one goes.
    pub fn unwatch(&mut self, key: &[u8]) {
        let Some(entry) = self.keys.get_mut(key) else {
            return;
        };
        entry.watchers -= 1;
        if entry.watchers == 0 {
            self.keys.remove(key);
        }
    }

    /// `None` when nothing is watching `key`, which a watcher can never
    /// see for a key it holds itself.
    pub fn version(&self, key: &[u8]) -> Option<u64> {
        self.keys.get(key).map(|w| w.version)
    }

    /// Records that `key` was modified. Untracked keys cost one failed
    /// hash lookup and nothing else.
    pub fn touch(&mut self, key: &[u8]) {
        if let Some(entry) = self.keys.get_mut(key) {
            entry.version += 1;
        }
    }

    /// Records that every key was modified - what FLUSHDB and FLUSHALL
    /// do to the keyspace.
    pub fn touch_all(&mut self) {
        for entry in self.keys.values_mut() {
            entry.version += 1;
        }
    }

    /// How many keys are being watched, for INFO.
    pub fn tracked(&self) -> usize {
        self.keys.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_untouched_key_keeps_its_version() {
        let mut watch = Watch::new();
        let version = watch.watch(b"k");
        assert_eq!(watch.version(b"k"), Some(version));
    }

    #[test]
    fn a_write_moves_the_version_on() {
        let mut watch = Watch::new();
        let version = watch.watch(b"k");
        watch.touch(b"k");
        assert_ne!(watch.version(b"k"), Some(version));
    }

    #[test]
    fn touching_an_unwatched_key_records_nothing() {
        let mut watch = Watch::new();
        watch.touch(b"nobody-watches-me");
        assert_eq!(watch.tracked(), 0);
    }

    #[test]
    fn the_last_watcher_out_forgets_the_key() {
        let mut watch = Watch::new();
        watch.watch(b"k");
        watch.watch(b"k");
        watch.unwatch(b"k");
        assert_eq!(watch.tracked(), 1);
        watch.unwatch(b"k");
        assert_eq!(watch.tracked(), 0);
    }

    #[test]
    fn a_flush_moves_every_version_on() {
        let mut watch = Watch::new();
        let a = watch.watch(b"a");
        let b = watch.watch(b"b");
        watch.touch_all();
        assert_ne!(watch.version(b"a"), Some(a));
        assert_ne!(watch.version(b"b"), Some(b));
    }
}
