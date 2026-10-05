//! Bundles the keyspace, persistence, configuration, and runtime
//! counters - the shared state threaded through command dispatch and
//! the event loop's periodic tick.

use std::time::Instant;

use crate::client::{Blocked, Client};
use crate::config::Config;
use crate::persist::Persist;
use crate::pubsub::PubSub;
use crate::resp::Reply;
use crate::stats::Stats;
use crate::store::{Store, StoreType};
use crate::util::bytes::Bytes;
use crate::watch::Watch;

pub struct App {
    pub store: Store,
    pub persist: Persist,
    pub config: Config,
    pub stats: Stats,
    pub running: bool,
    /// Who is subscribed to what.
    pub pubsub: PubSub,
    /// Version counters for the keys open transactions are watching.
    pub watch: Watch,
    /// Blocking commands waiting on a key, oldest first - the order
    /// they are served in when one arrives.
    pub blocked: Vec<Blocked>,
    /// Keys modified since the last wake-up scan. A blocking command
    /// waiting on one of these is retried before the loop polls again.
    pub ready_keys: Vec<Bytes>,
    /// Frames bound for *other* connections: pub/sub deliveries, which
    /// are the one thing a command produces that its own client never
    /// sees. The event loop drains this after every batch of commands.
    pub outbox: Vec<(u64, Reply)>,
    next_client_id: u64,
    last_sweep: Option<Instant>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let persist = Persist::new(&config.dbfilename);
        App {
            store: Store::new(),
            persist,
            config,
            stats: Stats::new(),
            running: true,
            pubsub: PubSub::new(),
            watch: Watch::new(),
            blocked: Vec::new(),
            ready_keys: Vec::new(),
            outbox: Vec::new(),
            next_client_id: 0,
            last_sweep: None,
        }
    }

    /// Ids start at 1, so 0 stays available as "no client" - which is
    /// what INFO and the log mean when they print it.
    pub fn take_client_id(&mut self) -> u64 {
        self.next_client_id += 1;
        self.next_client_id
    }

    /// Records that `key` was modified: any transaction watching it
    /// must now abort, and any command blocked on it should try again.
    ///
    /// Only a key that now holds a list or a sorted set can wake a
    /// waiter, since those are the only types anything blocks on.
    /// Redis draws the same line, by signalling readiness with the type
    /// that was written: a `SET` over a key somebody is BLPOPing does
    /// not wake them to a WRONGTYPE, it leaves them waiting.
    pub fn signal_modified(&mut self, key: &[u8]) {
        self.watch.touch(key);
        if self.blocked.is_empty() {
            return;
        }
        if matches!(
            self.store.peek_type(key),
            Some(StoreType::List) | Some(StoreType::Zset)
        ) {
            self.ready_keys.push(key.to_vec());
        }
    }

    /// The same signal for every key at once - what a flush is.
    pub fn signal_flushed(&mut self) {
        self.watch.touch_all();
        for waiter in &self.blocked {
            self.ready_keys.extend(waiter.keys.iter().cloned());
        }
    }

    /// Releases everything a departing connection held. Called once,
    /// from the event loop, so no registry can be left with an entry
    /// pointing at a socket that is gone.
    pub fn forget_client(&mut self, client: &mut Client) {
        self.pubsub.drop_client(client.id);
        self.blocked.retain(|waiter| waiter.id != client.id);
        for (key, _) in client.watched.drain(..) {
            self.watch.unwatch(&key);
        }
    }

    /// Drops a client's WATCH list, the half of UNWATCH that touches
    /// the shared registry.
    pub fn unwatch_all(&mut self, client: &mut Client) {
        for (key, _) in client.watched.drain(..) {
            self.watch.unwatch(&key);
        }
    }

    /// How long the event loop may sleep before a parked command has
    /// to be answered with its timeout reply.
    pub fn next_block_deadline(&self) -> Option<Instant> {
        self.blocked.iter().filter_map(|w| w.deadline).min()
    }

    /// Removes and returns the parked commands whose deadline has
    /// passed.
    pub fn take_timed_out(&mut self) -> Vec<Blocked> {
        let now = Instant::now();
        let mut expired = Vec::new();
        let mut index = 0;
        while index < self.blocked.len() {
            if self.blocked[index].deadline.is_some_and(|at| at <= now) {
                expired.push(self.blocked.remove(index));
            } else {
                index += 1;
            }
        }
        expired
    }

    /// Periodic housekeeping: expired-key sweep, then a possible
    /// autosave. Called from the event loop. Both intervals come from
    /// the config, so CONFIG SET changes them without a restart.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let should_sweep = match self.last_sweep {
            None => true,
            Some(last) => now.duration_since(last) >= self.config.sweep_interval,
        };
        if should_sweep {
            self.store.sweep_expired();
            self.sweep_memory_records();
            self.last_sweep = Some(now);
        }

        if self
            .persist
            .autosave_due(self.config.save_interval, &self.store)
        {
            self.save();
        }
    }

    /// Drops memory records whose own TTL has passed.
    ///
    /// A record's deadline is independent of the one on the key holding
    /// its index, so the keyspace sweep never sees it. Reads already
    /// hide an expired record, which makes this a memory-reclaiming
    /// pass rather than a correctness-preserving one.
    fn sweep_memory_records(&mut self) {
        for key in self.store.memory_keys() {
            let Some(memory) = self.store.get_existing_memory(&key) else {
                continue;
            };
            let dropped = memory.sweep_expired(self.config.mem_max_terms_per_doc);
            if dropped > 0 {
                self.stats.memory_records_expired += dropped as u64;
                self.store.mark_dirty();
            }
        }
    }

    /// Writes the keyspace to the configured dump path and records the
    /// outcome for INFO. The single place a save happens, so the
    /// bookkeeping can't be forgotten at a call site.
    pub fn save(&mut self) -> bool {
        let ok = self
            .persist
            .save_to(&self.config.dbfilename, &mut self.store);
        self.stats.last_save_at = Some(std::time::SystemTime::now());
        self.stats.last_save_ok = ok;
        if ok {
            self.stats.save_count += 1;
        }
        ok
    }
}
