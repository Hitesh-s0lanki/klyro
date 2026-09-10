//! A sorted vector of (member, score) pairs kept ordered by
//! `(score, member)`; simpler than Redis's skip list, adequate at the
//! scale this project targets (same O(n) tradeoff the original C
//! version made).

use std::cmp::Ordering;

use crate::types::list::resolve_range;
use crate::util::bytes::{parse_f64, Bytes};

#[derive(Clone)]
pub struct Zset {
    entries: Vec<(Bytes, f64)>,
}

/// One end of a ZRANGEBYSCORE-style interval: a score plus whether the
/// score itself is included. Redis spells the exclusive form with a
/// leading `(`, and accepts `-inf`/`+inf` at either end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoreBound {
    pub score: f64,
    pub exclusive: bool,
}

impl ScoreBound {
    /// Parses `5`, `(5`, `-inf`, or `+inf`. `None` if `s` isn't a score.
    pub fn parse(s: &[u8]) -> Option<ScoreBound> {
        match s.strip_prefix(b"(") {
            Some(inner) => parse_f64(inner).map(|score| ScoreBound {
                score,
                exclusive: true,
            }),
            None => parse_f64(s).map(|score| ScoreBound {
                score,
                exclusive: false,
            }),
        }
    }

    fn accepts_as_min(&self, score: f64) -> bool {
        if self.exclusive {
            score > self.score
        } else {
            score >= self.score
        }
    }

    fn accepts_as_max(&self, score: f64) -> bool {
        if self.exclusive {
            score < self.score
        } else {
            score <= self.score
        }
    }
}

fn cmp_entry(a: (&[u8], f64), b: (&[u8], f64)) -> Ordering {
    a.1.partial_cmp(&b.1)
        .unwrap_or(Ordering::Equal)
        .then_with(|| a.0.cmp(b.0))
}

impl Zset {
    pub fn new() -> Self {
        Zset {
            entries: Vec::new(),
        }
    }

    fn find_index(&self, member: &[u8]) -> Option<usize> {
        self.entries
            .iter()
            .position(|(m, _)| m.as_slice() == member)
    }

    /// Adds `member` with `score`, or repositions it if already present.
    /// Returns `true` if newly added, `false` if it already existed.
    pub fn add(&mut self, member: &[u8], score: f64) -> bool {
        let is_new = match self.find_index(member) {
            Some(idx) => {
                self.entries.remove(idx);
                false
            }
            None => true,
        };

        let pos = self
            .entries
            .partition_point(|(m, s)| cmp_entry((m, *s), (member, score)) == Ordering::Less);
        self.entries.insert(pos, (member.to_vec(), score));
        is_new
    }

    pub fn rem(&mut self, member: &[u8]) -> bool {
        match self.find_index(member) {
            Some(idx) => {
                self.entries.remove(idx);
                true
            }
            None => false,
        }
    }

    pub fn score(&self, member: &[u8]) -> Option<f64> {
        self.find_index(member).map(|i| self.entries[i].1)
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// Adds `delta` to `member`'s score (treating a missing member as
    /// score 0, as Redis's ZINCRBY does) and returns the new score.
    pub fn incr_by(&mut self, member: &[u8], delta: f64) -> f64 {
        let new_score = self.score(member).unwrap_or(0.0) + delta;
        self.add(member, new_score);
        new_score
    }

    /// 0-based position in ascending score order, or `None` if absent.
    pub fn rank(&self, member: &[u8]) -> Option<usize> {
        self.find_index(member)
    }

    /// 0-based position counting from the highest score down.
    pub fn rev_rank(&self, member: &[u8]) -> Option<usize> {
        self.rank(member).map(|r| self.entries.len() - 1 - r)
    }

    /// Members in ascending order over the inclusive range
    /// `[start, stop]`; negative indices count from the end, as in
    /// Redis's ZRANGE.
    pub fn range(&self, start: i64, stop: i64) -> Vec<(&[u8], f64)> {
        match resolve_range(self.entries.len(), start, stop) {
            None => Vec::new(),
            Some((start, stop)) => self.entries[start..=stop]
                .iter()
                .map(|(m, s)| (m.as_slice(), *s))
                .collect(),
        }
    }

    /// Like [`Zset::range`], but the indices count from the highest
    /// score down and the result is descending - Redis's ZREVRANGE.
    pub fn rev_range(&self, start: i64, stop: i64) -> Vec<(&[u8], f64)> {
        let len = self.entries.len();
        match resolve_range(len, start, stop) {
            None => Vec::new(),
            Some((start, stop)) => self.entries[len - 1 - stop..=len - 1 - start]
                .iter()
                .rev()
                .map(|(m, s)| (m.as_slice(), *s))
                .collect(),
        }
    }

    /// Ascending members whose score falls inside `[min, max]`, honoring
    /// each bound's exclusivity - Redis's ZRANGEBYSCORE.
    pub fn range_by_score(&self, min: ScoreBound, max: ScoreBound) -> Vec<(&[u8], f64)> {
        self.entries
            .iter()
            .filter(|(_, s)| min.accepts_as_min(*s) && max.accepts_as_max(*s))
            .map(|(m, s)| (m.as_slice(), *s))
            .collect()
    }

    /// How many members fall inside `[min, max]` - Redis's ZCOUNT.
    pub fn count_by_score(&self, min: ScoreBound, max: ScoreBound) -> usize {
        self.entries
            .iter()
            .filter(|(_, s)| min.accepts_as_min(*s) && max.accepts_as_max(*s))
            .count()
    }

    /// Drops members in the inclusive rank range, returning how many
    /// were removed - Redis's ZREMRANGEBYRANK.
    pub fn remove_range_by_rank(&mut self, start: i64, stop: i64) -> usize {
        match resolve_range(self.entries.len(), start, stop) {
            None => 0,
            Some((start, stop)) => self.entries.drain(start..=stop).count(),
        }
    }

    /// Drops members whose score falls inside `[min, max]`, returning how
    /// many were removed - Redis's ZREMRANGEBYSCORE.
    pub fn remove_range_by_score(&mut self, min: ScoreBound, max: ScoreBound) -> usize {
        let before = self.entries.len();
        self.entries
            .retain(|(_, s)| !(min.accepts_as_min(*s) && max.accepts_as_max(*s)));
        before - self.entries.len()
    }

    /// Removes and returns up to `count` members, lowest score first
    /// (`from_max` flips that to highest first) - ZPOPMIN / ZPOPMAX.
    pub fn pop(&mut self, count: usize, from_max: bool) -> Vec<(Bytes, f64)> {
        let count = count.min(self.entries.len());
        if from_max {
            let mut popped: Vec<(Bytes, f64)> =
                self.entries.drain(self.entries.len() - count..).collect();
            popped.reverse();
            popped
        } else {
            self.entries.drain(..count).collect()
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&[u8], f64)> {
        self.entries.iter().map(|(m, s)| (m.as_slice(), *s))
    }
}

impl Default for Zset {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> Zset {
        let mut z = Zset::new();
        z.add(b"bob", 50.0);
        z.add(b"carol", 75.0);
        z.add(b"alice", 100.0);
        z
    }

    /// Renders (member, score) pairs as text so assertions stay
    /// readable now that members are raw bytes.
    fn text(pairs: Vec<(&[u8], f64)>) -> Vec<(String, f64)> {
        pairs
            .into_iter()
            .map(|(m, s)| (String::from_utf8(m.to_vec()).unwrap(), s))
            .collect()
    }

    fn named(pairs: &[(&str, f64)]) -> Vec<(String, f64)> {
        pairs.iter().map(|(m, s)| (m.to_string(), *s)).collect()
    }

    fn inclusive(score: f64) -> ScoreBound {
        ScoreBound {
            score,
            exclusive: false,
        }
    }

    #[test]
    fn add_is_new_then_repositions() {
        let mut z = Zset::new();
        assert!(z.add(b"alice", 100.0));
        assert!(!z.add(b"alice", 5.0));
        assert_eq!(z.score(b"alice"), Some(5.0));
    }

    #[test]
    fn range_is_ascending_by_score() {
        assert_eq!(
            text(board().range(0, -1)),
            named(&[("bob", 50.0), ("carol", 75.0), ("alice", 100.0)])
        );
    }

    #[test]
    fn rev_range_is_descending_and_indexes_from_the_top() {
        let z = board();
        assert_eq!(
            text(z.rev_range(0, -1)),
            named(&[("alice", 100.0), ("carol", 75.0), ("bob", 50.0)])
        );
        assert_eq!(
            text(z.rev_range(0, 1)),
            named(&[("alice", 100.0), ("carol", 75.0)])
        );
    }

    #[test]
    fn members_may_hold_arbitrary_bytes() {
        let mut z = Zset::new();
        let member = b"a b\r\n\0c".to_vec();
        assert!(z.add(&member, 1.0));
        assert_eq!(z.score(&member), Some(1.0));
        assert_eq!(z.rank(&member), Some(0));
    }

    #[test]
    fn rem_and_size() {
        let mut z = Zset::new();
        z.add(b"only", 1.0);
        assert_eq!(z.size(), 1);
        assert!(z.rem(b"only"));
        assert_eq!(z.size(), 0);
        assert!(!z.rem(b"only"));
    }

    #[test]
    fn incr_by_treats_a_missing_member_as_zero() {
        let mut z = Zset::new();
        assert_eq!(z.incr_by(b"new", 5.0), 5.0);
        assert_eq!(z.incr_by(b"new", -2.0), 3.0);
        assert_eq!(z.score(b"new"), Some(3.0));
    }

    #[test]
    fn ranks_count_from_both_ends() {
        let z = board();
        assert_eq!(z.rank(b"bob"), Some(0));
        assert_eq!(z.rank(b"alice"), Some(2));
        assert_eq!(z.rev_rank(b"alice"), Some(0));
        assert_eq!(z.rev_rank(b"bob"), Some(2));
        assert_eq!(z.rank(b"nobody"), None);
    }

    #[test]
    fn score_bounds_parse_exclusive_and_infinite_forms() {
        assert_eq!(ScoreBound::parse(b"5"), Some(inclusive(5.0)));
        assert_eq!(
            ScoreBound::parse(b"(5"),
            Some(ScoreBound {
                score: 5.0,
                exclusive: true
            })
        );
        assert_eq!(ScoreBound::parse(b"-inf").unwrap().score, f64::NEG_INFINITY);
        assert_eq!(ScoreBound::parse(b"+inf").unwrap().score, f64::INFINITY);
        assert_eq!(ScoreBound::parse(b"abc"), None);
    }

    #[test]
    fn range_by_score_respects_exclusive_bounds() {
        let z = board();
        assert_eq!(
            text(z.range_by_score(inclusive(50.0), inclusive(75.0))),
            named(&[("bob", 50.0), ("carol", 75.0)])
        );
        let exclusive_low = ScoreBound {
            score: 50.0,
            exclusive: true,
        };
        assert_eq!(
            text(z.range_by_score(exclusive_low, inclusive(75.0))),
            named(&[("carol", 75.0)])
        );
        assert_eq!(z.count_by_score(inclusive(50.0), inclusive(100.0)), 3);
    }

    #[test]
    fn remove_range_by_rank_and_score() {
        let mut z = board();
        assert_eq!(z.remove_range_by_rank(0, 0), 1);
        assert_eq!(z.size(), 2);

        let mut z = board();
        assert_eq!(z.remove_range_by_score(inclusive(60.0), inclusive(80.0)), 1);
        assert_eq!(z.score(b"carol"), None);
        assert_eq!(z.size(), 2);
    }

    #[test]
    fn pop_takes_from_the_requested_end() {
        let mut z = board();
        assert_eq!(z.pop(1, false), vec![(b"bob".to_vec(), 50.0)]);

        let mut z = board();
        assert_eq!(
            z.pop(2, true),
            vec![(b"alice".to_vec(), 100.0), (b"carol".to_vec(), 75.0)]
        );

        let mut z = board();
        assert_eq!(z.pop(99, false).len(), 3);
    }
}
