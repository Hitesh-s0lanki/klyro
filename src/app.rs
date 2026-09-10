//! Bundles the keyspace, persistence, configuration, and runtime
//! counters - the shared state threaded through command dispatch and
//! the event loop's periodic tick.

use std::time::Instant;

use crate::config::Config;
use crate::persist::Persist;
use crate::stats::Stats;
use crate::store::Store;

pub struct App {
    pub store: Store,
    pub persist: Persist,
    pub config: Config,
    pub stats: Stats,
    pub running: bool,
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
            last_sweep: None,
        }
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
            self.last_sweep = Some(now);
        }

        if self
            .persist
            .autosave_due(self.config.save_interval, &self.store)
        {
            self.save();
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
