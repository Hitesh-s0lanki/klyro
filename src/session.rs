//! Per-connection state.
//!
//! Everything else in the server is shared: one `Store`, one `Config`,
//! one set of counters. Transactions are the first feature that needs
//! state belonging to a single client, so it lives here rather than
//! being smuggled into `App`.

use crate::resp::Protocol;
use crate::store::Store;
use crate::util::bytes::Bytes;

pub struct Session {
    /// RESP2 until a `HELLO 3` switches it.
    pub protocol: Protocol,
    /// `Some` once MULTI has been seen, holding the commands queued so
    /// far. `None` means no transaction is open.
    queued: Option<Vec<Vec<Bytes>>>,
    /// Set when a command could not be queued. EXEC refuses to run a
    /// transaction in this state, the way Redis does, rather than
    /// running the half of it that parsed.
    broken: bool,
    /// Keys this session is WATCHing, each with the stamp it had when
    /// the watch started.
    watches: Vec<(Bytes, u64)>,
}

impl Session {
    pub fn new() -> Session {
        Session {
            protocol: Protocol::Resp2,
            queued: None,
            broken: false,
            watches: Vec::new(),
        }
    }

    pub fn in_transaction(&self) -> bool {
        self.queued.is_some()
    }

    /// Opens a transaction. `false` if one is already open, which Redis
    /// treats as an error rather than a no-op.
    pub fn begin(&mut self) -> bool {
        if self.queued.is_some() {
            return false;
        }
        self.queued = Some(Vec::new());
        self.broken = false;
        true
    }

    pub fn queue(&mut self, argv: &[Bytes]) {
        if let Some(queue) = self.queued.as_mut() {
            queue.push(argv.to_vec());
        }
    }

    pub fn mark_broken(&mut self) {
        self.broken = true;
    }

    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// Ends the transaction, handing back what was queued.
    pub fn take_queue(&mut self) -> Option<Vec<Vec<Bytes>>> {
        self.broken = false;
        self.queued.take()
    }

    /// Ends the transaction, discarding the queue.
    pub fn discard(&mut self) -> bool {
        self.broken = false;
        self.queued.take().is_some()
    }

    /// Starts watching `key`. Watching the same key twice is harmless
    /// and is not deduplicated, matching Redis.
    pub fn watch(&mut self, store: &mut Store, key: &[u8]) {
        let stamp = store.watch(key);
        self.watches.push((key.to_vec(), stamp));
    }

    /// Whether every watched key still carries the stamp it had when
    /// the watch started. `false` means EXEC must abort.
    pub fn watches_intact(&self, store: &Store) -> bool {
        self.watches
            .iter()
            .all(|(key, stamp)| store.watch_stamp(key) == *stamp)
    }

    /// Releases every watch. Called by UNWATCH, by EXEC and DISCARD,
    /// and when the connection closes - the registry would otherwise
    /// keep entries alive for a client that has gone away.
    pub fn unwatch_all(&mut self, store: &mut Store) {
        for (key, _) in self.watches.drain(..) {
            store.unwatch(&key);
        }
    }

    /// RESET: back to a freshly connected state.
    pub fn reset(&mut self, store: &mut Store) {
        self.discard();
        self.unwatch_all(store);
        self.protocol = Protocol::Resp2;
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_refuses_to_nest() {
        let mut session = Session::new();
        assert!(session.begin());
        assert!(!session.begin());
    }

    #[test]
    fn take_queue_ends_the_transaction() {
        let mut session = Session::new();
        session.begin();
        session.queue(&[b"PING".to_vec()]);
        assert!(session.in_transaction());

        let queued = session.take_queue().expect("a transaction was open");
        assert_eq!(queued.len(), 1);
        assert!(!session.in_transaction());
        assert!(session.take_queue().is_none());
    }

    #[test]
    fn discard_clears_the_queue_and_the_broken_flag() {
        let mut session = Session::new();
        session.begin();
        session.queue(&[b"PING".to_vec()]);
        session.mark_broken();
        assert!(session.discard());
        assert!(!session.in_transaction());
        assert!(!session.is_broken());
        assert!(!session.discard());
    }

    #[test]
    fn a_watch_survives_an_unrelated_write() {
        let mut store = Store::new();
        let mut session = Session::new();
        store.set_string(b"other", b"v");
        session.watch(&mut store, b"watched");

        store.set_string(b"other", b"changed");
        assert!(session.watches_intact(&store));
    }

    #[test]
    fn a_watch_breaks_when_its_key_changes() {
        let mut store = Store::new();
        let mut session = Session::new();
        session.watch(&mut store, b"k");
        assert!(session.watches_intact(&store));

        store.set_string(b"k", b"v");
        assert!(!session.watches_intact(&store));
    }

    #[test]
    fn a_watch_breaks_on_a_delete_and_on_a_collection_edit() {
        let mut store = Store::new();
        store.set_string(b"k", b"v");
        let mut session = Session::new();
        session.watch(&mut store, b"k");
        store.del(b"k");
        assert!(!session.watches_intact(&store));

        let mut store = Store::new();
        store
            .get_or_create_list(b"l")
            .unwrap()
            .push_back(b"a".into());
        let mut session = Session::new();
        session.watch(&mut store, b"l");
        assert!(session.watches_intact(&store));
        // Popping through the write accessor must move the stamp.
        store.write_list(b"l").unwrap().pop_front();
        assert!(!session.watches_intact(&store));
    }

    #[test]
    fn reading_a_watched_key_does_not_break_the_watch() {
        let mut store = Store::new();
        store
            .get_or_create_hash(b"h")
            .unwrap()
            .insert(b"f".into(), b"v".into());
        let mut session = Session::new();
        session.watch(&mut store, b"h");

        assert!(store.read_hash(b"h").is_some());
        assert!(store.get_string(b"nope").is_none());
        assert!(session.watches_intact(&store));
    }

    #[test]
    fn unwatch_all_releases_the_registry() {
        let mut store = Store::new();
        let mut session = Session::new();
        session.watch(&mut store, b"a");
        session.watch(&mut store, b"b");
        assert_eq!(store.watched_count(), 2);

        session.unwatch_all(&mut store);
        assert_eq!(store.watched_count(), 0);
        // Watching again after releasing starts a fresh entry.
        session.watch(&mut store, b"a");
        assert_eq!(store.watched_count(), 1);
    }

    #[test]
    fn two_sessions_watching_one_key_are_refcounted() {
        let mut store = Store::new();
        let mut first = Session::new();
        let mut second = Session::new();
        first.watch(&mut store, b"k");
        second.watch(&mut store, b"k");
        assert_eq!(store.watched_count(), 1);

        first.unwatch_all(&mut store);
        // The second session still cares, so the entry stays.
        assert_eq!(store.watched_count(), 1);
        store.set_string(b"k", b"v");
        assert!(!second.watches_intact(&store));

        second.unwatch_all(&mut store);
        assert_eq!(store.watched_count(), 0);
    }
}
