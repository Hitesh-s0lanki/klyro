//! Bundles the keyspace, persistence, and the server's running flag -
//! the shared state threaded through command dispatch and the event
//! loop's periodic tick.

use std::time::{Duration, Instant};

use crate::persist::Persist;
use crate::store::Store;

const SWEEP_INTERVAL: Duration = Duration::from_secs(1);

pub struct App {
    pub store: Store,
    pub persist: Persist,
    pub running: bool,
    last_sweep: Option<Instant>,
}

impl App {
    pub fn new(dump_path: &str) -> Self {
        App {
            store: Store::new(),
            persist: Persist::new(dump_path),
            running: true,
            last_sweep: None,
        }
    }

    /// Periodic housekeeping: expired-key sweep, then a possible
    /// autosave. Called from the event loop.
    pub fn tick(&mut self) {
        let now = Instant::now();
        let should_sweep = match self.last_sweep {
            None => true,
            Some(last) => now.duration_since(last) >= SWEEP_INTERVAL,
        };
        if should_sweep {
            self.store.sweep_expired();
            self.last_sweep = Some(now);
        }
        self.persist.tick(&mut self.store);
    }
}
