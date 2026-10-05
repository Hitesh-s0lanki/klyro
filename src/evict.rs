//! `maxmemory` and the eviction policies.
//!
//! The measurement half of this was already in place: the counting
//! global allocator in [`crate::util::memory`] knows how many bytes the
//! process holds. What is here is the policy half - when to act on that
//! number, which key to drop, and what to tell a client whose write
//! cannot be made room for.
//!
//! Two things are worth knowing about the number being compared against
//! `maxmemory`. It is the whole process, not just the keyspace, so a
//! limit set below the few megabytes the runtime and the client buffers
//! need can never be satisfied. And a freed value leaves the allocator's
//! total immediately, but the map's own table does not shrink, so
//! evicting keys reclaims values rather than the space the keyspace
//! index occupies. Redis behaves the same way on both counts.

use std::time::SystemTime;

use crate::app::App;
use crate::store::Candidate;
use crate::util::memory::used_bytes;

/// Which keys may be evicted, and how a victim is picked from them.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Policy {
    /// Evict nothing; refuse the writes instead. The default, because
    /// silently dropping data a client believes it stored is a worse
    /// surprise than an error that names the reason.
    #[default]
    NoEviction,
    AllkeysLru,
    AllkeysLfu,
    AllkeysRandom,
    VolatileLru,
    VolatileLfu,
    VolatileRandom,
    /// Among keys with an expiry, the one due to go soonest.
    VolatileTtl,
}

/// Every policy name, in the order CONFIG GET and the docs list them.
pub const POLICIES: &[(&str, Policy)] = &[
    ("noeviction", Policy::NoEviction),
    ("allkeys-lru", Policy::AllkeysLru),
    ("allkeys-lfu", Policy::AllkeysLfu),
    ("allkeys-random", Policy::AllkeysRandom),
    ("volatile-lru", Policy::VolatileLru),
    ("volatile-lfu", Policy::VolatileLfu),
    ("volatile-random", Policy::VolatileRandom),
    ("volatile-ttl", Policy::VolatileTtl),
];

impl Policy {
    /// Parses a policy name, case-insensitively. `None` if unknown.
    pub fn parse(name: &str) -> Option<Policy> {
        let lower = name.to_ascii_lowercase();
        POLICIES
            .iter()
            .find(|(spelling, _)| *spelling == lower)
            .map(|(_, policy)| *policy)
    }

    pub fn name(self) -> &'static str {
        POLICIES
            .iter()
            .find(|(_, policy)| *policy == self)
            .map(|(spelling, _)| *spelling)
            .expect("every variant is listed")
    }

    /// Whether this policy may only take keys that carry an expiry.
    fn volatile_only(self) -> bool {
        matches!(
            self,
            Policy::VolatileLru
                | Policy::VolatileLfu
                | Policy::VolatileRandom
                | Policy::VolatileTtl
        )
    }

    fn evicts(self) -> bool {
        self != Policy::NoEviction
    }

    /// The best victim among `candidates`, by this policy's ordering.
    /// `None` when nothing was sampled.
    fn choose(self, candidates: Vec<Candidate>) -> Option<Candidate> {
        match self {
            // Nothing is a victim under noeviction, and a random policy
            // takes the first draw - the sample was already random, so
            // choosing within it would be choosing twice.
            Policy::NoEviction => None,
            Policy::AllkeysRandom | Policy::VolatileRandom => candidates.into_iter().next(),
            Policy::AllkeysLru | Policy::VolatileLru => {
                candidates.into_iter().min_by_key(|c| c.last_access)
            }
            Policy::AllkeysLfu | Policy::VolatileLfu => {
                // Ties on the counter are broken by age, which matters
                // more than it looks: the counter is one byte, so a
                // cold keyspace has every key sitting on the same
                // value and nothing else to separate them by.
                candidates
                    .into_iter()
                    .min_by_key(|c| (c.freq, c.last_access))
            }
            Policy::VolatileTtl => candidates
                .into_iter()
                .min_by_key(|c| c.expire_at.unwrap_or(far_future())),
        }
    }
}

/// The stand-in deadline for a candidate with no expiry, so it sorts
/// last under `volatile-ttl`. Only reachable if a key loses its TTL
/// between being sampled and being compared.
fn far_future() -> SystemTime {
    SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(u32::MAX as u64)
}

/// A ceiling on evictions in one pass, so a `maxmemory` set below what
/// the process needs at rest cannot park the event loop in a loop that
/// frees nothing. Past it the pass gives up and the caller reports OOM;
/// the next command tries again.
const MAX_EVICTIONS_PER_PASS: usize = 10_000;

/// Whether the process is within `maxmemory` right now. Always true
/// when no limit is set.
pub fn within_limit(app: &App) -> bool {
    app.config.maxmemory == 0 || used_bytes() <= app.config.maxmemory
}

/// Evicts until the process is back under `maxmemory`, and reports
/// whether it got there.
///
/// Called before every command rather than only before a write, which
/// is what Redis does: a read leaves the server over its limit just as
/// surely as a write does, and waiting for the next write to notice
/// would leave it over the limit for as long as the traffic stayed
/// read-only.
pub fn make_room(app: &mut App) -> bool {
    if within_limit(app) {
        return true;
    }
    let policy = app.config.maxmemory_policy;
    if !policy.evicts() {
        return false;
    }

    let volatile_only = policy.volatile_only();
    for _ in 0..MAX_EVICTIONS_PER_PASS {
        if volatile_only && app.store.volatile_is_empty() {
            return false;
        }
        let candidates = app
            .store
            .sample(volatile_only, app.config.maxmemory_samples);
        let Some(victim) = policy.choose(candidates) else {
            return false; // nothing left that this policy may take
        };
        if !app.store.evict(&victim.key) {
            continue;
        }
        app.stats.evicted_keys += 1;
        // A transaction watching an evicted key has to abort: the read
        // it based its queue on is no longer what the keyspace holds.
        app.signal_modified(&victim.key);

        if within_limit(app) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn candidate(key: &str, last_access: u64, freq: u8, ttl: Option<u64>) -> Candidate {
        Candidate {
            key: key.as_bytes().to_vec(),
            last_access,
            freq,
            expire_at: ttl.map(|s| SystemTime::now() + Duration::from_secs(s)),
        }
    }

    fn victim(policy: Policy, candidates: Vec<Candidate>) -> Option<String> {
        policy
            .choose(candidates)
            .map(|c| String::from_utf8(c.key).unwrap())
    }

    #[test]
    fn every_policy_round_trips_through_its_name() {
        for (spelling, policy) in POLICIES {
            assert_eq!(Policy::parse(spelling), Some(*policy));
            assert_eq!(policy.name(), *spelling);
        }
        assert_eq!(Policy::parse("ALLKEYS-LRU"), Some(Policy::AllkeysLru));
        assert_eq!(Policy::parse("allkeys-mru"), None);
    }

    #[test]
    fn lru_takes_the_least_recently_used() {
        let pool = vec![
            candidate("recent", 900, 5, None),
            candidate("old", 12, 5, None),
            candidate("middling", 400, 5, None),
        ];
        assert_eq!(victim(Policy::AllkeysLru, pool), Some("old".to_string()));
    }

    #[test]
    fn lfu_takes_the_least_frequently_used_before_the_oldest() {
        let pool = vec![
            candidate("hot-but-old", 1, 200, None),
            candidate("cold-but-recent", 999, 3, None),
        ];
        assert_eq!(
            victim(Policy::AllkeysLfu, pool),
            Some("cold-but-recent".to_string())
        );
    }

    #[test]
    fn lfu_breaks_a_tie_on_the_counter_by_age() {
        let pool = vec![
            candidate("newer", 500, 5, None),
            candidate("older", 5, 5, None),
        ];
        assert_eq!(victim(Policy::AllkeysLfu, pool), Some("older".to_string()));
    }

    #[test]
    fn volatile_ttl_takes_the_one_expiring_soonest() {
        let pool = vec![
            candidate("later", 1, 5, Some(600)),
            candidate("soonest", 1, 5, Some(5)),
            candidate("never", 1, 5, None),
        ];
        assert_eq!(
            victim(Policy::VolatileTtl, pool),
            Some("soonest".to_string())
        );
    }

    #[test]
    fn noeviction_never_names_a_victim() {
        let pool = vec![candidate("k", 1, 5, Some(5))];
        assert_eq!(victim(Policy::NoEviction, pool), None);
    }

    #[test]
    fn an_empty_sample_yields_no_victim() {
        for (_, policy) in POLICIES {
            assert_eq!(victim(*policy, Vec::new()), None);
        }
    }

    #[test]
    fn only_the_volatile_policies_restrict_the_pool() {
        assert!(Policy::VolatileLru.volatile_only());
        assert!(Policy::VolatileTtl.volatile_only());
        assert!(!Policy::AllkeysLru.volatile_only());
        assert!(!Policy::NoEviction.volatile_only());
    }
}
