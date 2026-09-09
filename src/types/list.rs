//! An ordered sequence of strings - the backing store for the
//! Redis-style List data type. `std::collections::VecDeque` already
//! gives us O(1) push/pop at both ends, so there's no need for a
//! hand-rolled doubly linked list.

use std::collections::VecDeque;

pub type List = VecDeque<String>;

/// Values covering the inclusive range `[start, stop]`; negative indices
/// count from the end, as in Redis's LRANGE.
pub fn range(list: &List, start: i64, stop: i64) -> Vec<&str> {
    let len = list.len() as i64;

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

    list.iter()
        .skip(start as usize)
        .take((stop - start + 1) as usize)
        .map(|s| s.as_str())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_handles_negative_indices() {
        let mut list: List = List::new();
        for v in ["a", "b", "c", "d"] {
            list.push_back(v.to_string());
        }
        assert_eq!(range(&list, 0, -1), vec!["a", "b", "c", "d"]);
        assert_eq!(range(&list, 1, 2), vec!["b", "c"]);
        assert_eq!(range(&list, -2, -1), vec!["c", "d"]);
    }

    #[test]
    fn range_on_empty_list_is_empty() {
        let list: List = List::new();
        assert!(range(&list, 0, -1).is_empty());
    }
}
