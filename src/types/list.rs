//! An ordered sequence of strings - the backing store for the
//! Redis-style List data type. `std::collections::VecDeque` already
//! gives us O(1) push/pop at both ends, so there's no need for a
//! hand-rolled doubly linked list.

use std::collections::VecDeque;

pub type List = VecDeque<String>;

/// Resolves an inclusive `[start, stop]` index pair against a
/// collection of `len` items, translating negative indices (counted from
/// the end) and clamping to bounds. `None` means the range is empty.
/// Shared by LRANGE, LTRIM, ZRANGE, and ZREMRANGEBYRANK, which all use
/// Redis's identical index rules.
pub fn resolve_range(len: usize, start: i64, stop: i64) -> Option<(usize, usize)> {
    let len = len as i64;
    let mut start = if start < 0 { start + len } else { start };
    let mut stop = if stop < 0 { stop + len } else { stop };
    if start < 0 {
        start = 0;
    }
    if stop >= len {
        stop = len - 1;
    }

    if len == 0 || start > stop || start >= len {
        return None;
    }
    Some((start as usize, stop as usize))
}

/// Translates a single possibly-negative index into a real position, or
/// `None` if it falls outside the collection.
pub fn resolve_index(len: usize, index: i64) -> Option<usize> {
    let real = if index < 0 { index + len as i64 } else { index };
    if real < 0 || real >= len as i64 {
        None
    } else {
        Some(real as usize)
    }
}

/// Values covering the inclusive range `[start, stop]`; negative indices
/// count from the end, as in Redis's LRANGE.
pub fn range(list: &List, start: i64, stop: i64) -> Vec<&str> {
    match resolve_range(list.len(), start, stop) {
        None => Vec::new(),
        Some((start, stop)) => list
            .iter()
            .skip(start)
            .take(stop - start + 1)
            .map(|s| s.as_str())
            .collect(),
    }
}

/// Keeps only the inclusive range `[start, stop]`, discarding the rest.
/// An empty range clears the list, as in Redis's LTRIM.
pub fn trim(list: &mut List, start: i64, stop: i64) {
    match resolve_range(list.len(), start, stop) {
        None => list.clear(),
        Some((start, stop)) => {
            list.truncate(stop + 1);
            list.drain(..start);
        }
    }
}

/// Removes occurrences of `value`, following Redis's LREM count rules:
/// `count > 0` removes that many from the head, `count < 0` that many
/// from the tail, `count == 0` removes every match. Returns how many
/// were removed.
pub fn remove(list: &mut List, count: i64, value: &str) -> usize {
    let limit = if count == 0 {
        usize::MAX
    } else {
        count.unsigned_abs() as usize
    };
    let from_tail = count < 0;

    let mut positions: Vec<usize> = list
        .iter()
        .enumerate()
        .filter(|(_, v)| v.as_str() == value)
        .map(|(i, _)| i)
        .collect();
    if from_tail {
        positions.reverse();
    }
    positions.truncate(limit);

    // Remove high indices first so earlier positions stay valid.
    positions.sort_unstable_by(|a, b| b.cmp(a));
    for idx in &positions {
        list.remove(*idx);
    }
    positions.len()
}

/// Inserts `value` immediately before or after the first occurrence of
/// `pivot`. Returns the new length, or `None` if `pivot` is absent.
pub fn insert(list: &mut List, before: bool, pivot: &str, value: &str) -> Option<usize> {
    let pos = list.iter().position(|v| v.as_str() == pivot)?;
    let at = if before { pos } else { pos + 1 };
    list.insert(at, value.to_string());
    Some(list.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list_of(values: &[&str]) -> List {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn range_handles_negative_indices() {
        let list = list_of(&["a", "b", "c", "d"]);
        assert_eq!(range(&list, 0, -1), vec!["a", "b", "c", "d"]);
        assert_eq!(range(&list, 1, 2), vec!["b", "c"]);
        assert_eq!(range(&list, -2, -1), vec!["c", "d"]);
    }

    #[test]
    fn range_on_empty_list_is_empty() {
        let list: List = List::new();
        assert!(range(&list, 0, -1).is_empty());
    }

    #[test]
    fn resolve_index_translates_negatives_and_rejects_out_of_range() {
        assert_eq!(resolve_index(4, 0), Some(0));
        assert_eq!(resolve_index(4, -1), Some(3));
        assert_eq!(resolve_index(4, 4), None);
        assert_eq!(resolve_index(4, -5), None);
        assert_eq!(resolve_index(0, 0), None);
    }

    #[test]
    fn trim_keeps_only_the_requested_window() {
        let mut list = list_of(&["a", "b", "c", "d", "e"]);
        trim(&mut list, 1, 3);
        assert_eq!(range(&list, 0, -1), vec!["b", "c", "d"]);
    }

    #[test]
    fn trim_with_an_empty_range_clears_the_list() {
        let mut list = list_of(&["a", "b"]);
        trim(&mut list, 5, 10);
        assert!(list.is_empty());
    }

    #[test]
    fn remove_honors_count_direction() {
        let mut list = list_of(&["x", "a", "x", "b", "x"]);
        assert_eq!(remove(&mut list, 2, "x"), 2);
        assert_eq!(range(&list, 0, -1), vec!["a", "b", "x"]);

        let mut list = list_of(&["x", "a", "x", "b", "x"]);
        assert_eq!(remove(&mut list, -1, "x"), 1);
        assert_eq!(range(&list, 0, -1), vec!["x", "a", "x", "b"]);

        let mut list = list_of(&["x", "a", "x"]);
        assert_eq!(remove(&mut list, 0, "x"), 2);
        assert_eq!(range(&list, 0, -1), vec!["a"]);
    }

    #[test]
    fn insert_before_and_after_a_pivot() {
        let mut list = list_of(&["a", "c"]);
        assert_eq!(insert(&mut list, true, "c", "b"), Some(3));
        assert_eq!(range(&list, 0, -1), vec!["a", "b", "c"]);
        assert_eq!(insert(&mut list, false, "c", "d"), Some(4));
        assert_eq!(range(&list, 0, -1), vec!["a", "b", "c", "d"]);
        assert_eq!(insert(&mut list, true, "nope", "z"), None);
    }
}
