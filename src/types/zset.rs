//! A sorted vector of (member, score) pairs kept ordered by
//! `(score, member)`; simpler than Redis's skip list, adequate at the
//! scale this project targets (same O(n) tradeoff the original C
//! version made).

use std::cmp::Ordering;

pub struct Zset {
    entries: Vec<(String, f64)>,
}

fn cmp_entry(a: (&str, f64), b: (&str, f64)) -> Ordering {
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

    fn find_index(&self, member: &str) -> Option<usize> {
        self.entries.iter().position(|(m, _)| m == member)
    }

    /// Adds `member` with `score`, or repositions it if already present.
    /// Returns `true` if newly added, `false` if it already existed.
    pub fn add(&mut self, member: &str, score: f64) -> bool {
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
        self.entries.insert(pos, (member.to_string(), score));
        is_new
    }

    pub fn rem(&mut self, member: &str) -> bool {
        match self.find_index(member) {
            Some(idx) => {
                self.entries.remove(idx);
                true
            }
            None => false,
        }
    }

    pub fn score(&self, member: &str) -> Option<f64> {
        self.find_index(member).map(|i| self.entries[i].1)
    }

    pub fn size(&self) -> usize {
        self.entries.len()
    }

    /// Members in ascending order over the inclusive range
    /// `[start, stop]`; negative indices count from the end, as in
    /// Redis's ZRANGE.
    pub fn range(&self, start: i64, stop: i64) -> Vec<(&str, f64)> {
        let len = self.entries.len() as i64;

        let mut start = if start < 0 { start + len } else { start };
        let mut stop = if stop < 0 { stop + len } else { stop };
        if start < 0 {
            start = 0;
        }
        if stop >= len {
            stop = len - 1;
        }

        if len == 0 || start > stop || start >= len {
            return Vec::new();
        }

        self.entries[start as usize..=stop as usize]
            .iter()
            .map(|(m, s)| (m.as_str(), *s))
            .collect()
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

    #[test]
    fn add_is_new_then_repositions() {
        let mut z = Zset::new();
        assert!(z.add("alice", 100.0));
        assert!(!z.add("alice", 5.0));
        assert_eq!(z.score("alice"), Some(5.0));
    }

    #[test]
    fn range_is_ascending_by_score() {
        let mut z = Zset::new();
        z.add("alice", 100.0);
        z.add("bob", 50.0);
        z.add("carol", 75.0);
        assert_eq!(
            z.range(0, -1),
            vec![("bob", 50.0), ("carol", 75.0), ("alice", 100.0)]
        );
    }

    #[test]
    fn rem_and_size() {
        let mut z = Zset::new();
        z.add("only", 1.0);
        assert_eq!(z.size(), 1);
        assert!(z.rem("only"));
        assert_eq!(z.size(), 0);
        assert!(!z.rem("only"));
    }
}
