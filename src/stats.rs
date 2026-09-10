//! Counters behind the INFO command. Everything here is incremented on
//! the one event-loop thread, so plain fields are enough - no atomics.

use std::time::{Duration, Instant, SystemTime};

pub struct Stats {
    started_at: Instant,
    /// Connections accepted since startup, including ones later turned
    /// away for exceeding `maxclients`.
    pub total_connections: u64,
    pub rejected_connections: u64,
    pub connected_clients: usize,
    pub total_commands: u64,
    /// Lookups by read-only commands that found a live key, and that
    /// didn't. Write commands are deliberately excluded, so the ratio
    /// means the same thing it does in Redis.
    pub keyspace_hits: u64,
    pub keyspace_misses: u64,
    /// Memory records dropped because their own TTL passed - the
    /// per-record counterpart to the keyspace's expired-key count.
    pub memory_records_expired: u64,
    /// PUBLISH calls, whether or not anyone was listening.
    pub messages_published: u64,
    /// Transactions that reached EXEC and ran, which excludes the ones
    /// a WATCH aborted.
    pub transactions: u64,
    pub last_save_at: Option<SystemTime>,
    pub last_save_ok: bool,
    pub save_count: u64,
}

impl Stats {
    pub fn new() -> Self {
        Stats {
            started_at: Instant::now(),
            total_connections: 0,
            rejected_connections: 0,
            connected_clients: 0,
            total_commands: 0,
            keyspace_hits: 0,
            keyspace_misses: 0,
            memory_records_expired: 0,
            messages_published: 0,
            transactions: 0,
            last_save_at: None,
            last_save_ok: true,
            save_count: 0,
        }
    }

    pub fn uptime(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Resets the counters CONFIG RESETSTAT covers. Uptime, the client
    /// count, and the save record describe the server's current state
    /// rather than accumulated activity, so they stay.
    pub fn reset(&mut self) {
        self.total_connections = 0;
        self.rejected_connections = 0;
        self.total_commands = 0;
        self.keyspace_hits = 0;
        self.keyspace_misses = 0;
        self.messages_published = 0;
        self.transactions = 0;
    }
}

impl Default for Stats {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_clears_activity_but_keeps_state() {
        let mut stats = Stats::new();
        stats.total_commands = 10;
        stats.keyspace_hits = 5;
        stats.connected_clients = 3;
        stats.save_count = 2;

        stats.reset();

        assert_eq!(stats.total_commands, 0);
        assert_eq!(stats.keyspace_hits, 0);
        assert_eq!(stats.connected_clients, 3);
        assert_eq!(stats.save_count, 2);
    }
}
