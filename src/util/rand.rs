//! A tiny xorshift64* generator - enough for SPOP/SRANDMEMBER/RANDOMKEY,
//! which only need "some member, not always the same one". Deliberately
//! not a crate dependency: the project builds with `libc` alone, and
//! nothing here is security-sensitive.

use std::cell::Cell;
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    static STATE: Cell<u64> = const { Cell::new(0) };
}

fn seed() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x2545_F491_4F6C_DD1D);
    // A zero state is a fixed point for xorshift, so never return one.
    nanos | 1
}

/// The next pseudo-random `u64`.
pub fn next_u64() -> u64 {
    STATE.with(|s| {
        let mut x = s.get();
        if x == 0 {
            x = seed();
        }
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        s.set(x);
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    })
}

/// A uniform-enough value in `[0, 1)`, for the LFU counter's
/// probabilistic increment. Built from the top 53 bits, which is every
/// bit an `f64` can hold without rounding.
pub fn unit_interval() -> f64 {
    (next_u64() >> 11) as f64 / (1u64 << 53) as f64
}

/// A uniform-enough value in `0..n`. Returns 0 when `n` is 0.
pub fn below(n: usize) -> usize {
    if n == 0 {
        0
    } else {
        (next_u64() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn below_stays_in_range_and_varies() {
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let v = below(10);
            assert!(v < 10);
            seen.insert(v);
        }
        assert!(seen.len() > 1, "generator returned a constant");
    }

    #[test]
    fn below_zero_is_zero() {
        assert_eq!(below(0), 0);
    }

    #[test]
    fn unit_interval_stays_in_range_and_spreads() {
        let mut sum = 0.0;
        for _ in 0..1000 {
            let v = unit_interval();
            assert!((0.0..1.0).contains(&v), "{v} left [0, 1)");
            sum += v;
        }
        // A wide band: this asserts the generator is not stuck near an
        // end, not that it passes a randomness test.
        let mean = sum / 1000.0;
        assert!((0.4..0.6).contains(&mean), "mean was {mean}");
    }
}
