//! Turning several ranked lists into one.
//!
//! The problem fusion solves is that the component scores are not
//! comparable. BM25 is unbounded and depends on the collection; cosine
//! sits in [-1, 1]; recency and importance are already fractions.
//! Adding them raw would let whichever component happened to score
//! higher decide the ranking, and the configured weights would mean
//! nothing.
//!
//! Two strategies, because neither is right everywhere:
//!
//! - `LINEAR` min-max normalizes each component across the candidate
//!   set, then applies the weights. This is what the product
//!   specification describes, and it preserves the *margins* between
//!   candidates rather than only their order.
//! - `RRF` scores by rank alone. It needs no normalization, so it is
//!   unbothered by the case linear normalization handles worst: a
//!   candidate set where one index scored everything nearly the same,
//!   where min-max amplifies noise into a full-range spread.

use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use super::record::MemoryRecord;
use super::{Hit, Weights};
use crate::util::bytes::{eq_ignore_case, Bytes};

/// The constant in reciprocal rank fusion's `1 / (k + rank)`. Sixty is
/// the value the original paper settled on, and it is what every
/// implementation since has used: large enough that the top few ranks
/// don't dominate outright.
const RRF_K: f32 = 60.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fusion {
    Linear,
    Rrf,
}

impl Fusion {
    pub fn parse(word: &[u8]) -> Option<Fusion> {
        if eq_ignore_case(word, "LINEAR") {
            Some(Fusion::Linear)
        } else if eq_ignore_case(word, "RRF") {
            Some(Fusion::Rrf)
        } else {
            None
        }
    }
}

/// How much of its original weight a memory still carries, decaying by
/// half every `half_life`. Exponential rather than linear because
/// "twice as old" should not mean "half as relevant" forever - an
/// eighteen-month-old memory and a two-year-old one are both simply
/// old.
pub fn recency_score(updated_at: SystemTime, now: SystemTime, half_life: Duration) -> f32 {
    let half_life = half_life.as_secs_f64().max(1.0);
    let age = now
        .duration_since(updated_at)
        .map_or(0.0, |d| d.as_secs_f64());
    0.5f64.powf(age / half_life) as f32
}

/// Rescales a list of scores onto [0, 1].
///
/// When every candidate scored the same, the spread is zero and there
/// is nothing to rescale. Returning 1.0 for all of them - rather than
/// 0.0, or dividing by zero - keeps a component that agreed about
/// everything from silently deleting itself from the fused score.
fn normalize(values: &mut [f32]) {
    let (mut low, mut high) = (f32::INFINITY, f32::NEG_INFINITY);
    for value in values.iter() {
        low = low.min(*value);
        high = high.max(*value);
    }
    let spread = high - low;
    for value in values.iter_mut() {
        *value = if spread > 0.0 { (*value - low) / spread } else { 1.0 };
    }
}

/// Ranks, 1-based, for reciprocal rank fusion. Equal scores share the
/// best rank they are entitled to, so fusion can't be swayed by the
/// arbitrary order a tie came back in.
fn ranks(scores: &[f32]) -> Vec<f32> {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|a, b| {
        scores[*b]
            .partial_cmp(&scores[*a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut out = vec![0.0; scores.len()];
    let mut rank = 0.0;
    for (position, index) in order.iter().enumerate() {
        if position == 0 || scores[*index] != scores[order[position - 1]] {
            rank = position as f32 + 1.0;
        }
        out[*index] = rank;
    }
    out
}

/// Combines the keyword and vector candidate lists into one ranking.
///
/// A candidate that only one index found keeps its place: its missing
/// component scores zero rather than disqualifying it. That is what
/// lets a hybrid query surface a memory that matched semantically but
/// shared no words with the query, which is the whole reason the mode
/// exists.
pub fn fuse<'a>(
    keyword: Vec<(Bytes, f32)>,
    vector: Vec<(Bytes, f32)>,
    lookup: impl Fn(&[u8]) -> Option<&'a MemoryRecord>,
    weights: Weights,
    fusion: Fusion,
    half_life: Duration,
    now: SystemTime,
) -> Vec<Hit> {
    // Column index per id, so each candidate is one row whichever list
    // (or both) it arrived in.
    let mut rows: HashMap<Bytes, usize> = HashMap::new();
    let mut ids: Vec<Bytes> = Vec::new();
    let mut raw_keyword: Vec<f32> = Vec::new();
    let mut raw_vector: Vec<f32> = Vec::new();
    let mut had_keyword: Vec<bool> = Vec::new();
    let mut had_vector: Vec<bool> = Vec::new();

    let mut row_for = |id: &Bytes,
                       ids: &mut Vec<Bytes>,
                       kw: &mut Vec<f32>,
                       vc: &mut Vec<f32>,
                       hk: &mut Vec<bool>,
                       hv: &mut Vec<bool>| {
        *rows.entry(id.clone()).or_insert_with(|| {
            ids.push(id.clone());
            kw.push(0.0);
            vc.push(0.0);
            hk.push(false);
            hv.push(false);
            ids.len() - 1
        })
    };

    for (id, score) in &keyword {
        let row = row_for(id, &mut ids, &mut raw_keyword, &mut raw_vector, &mut had_keyword, &mut had_vector);
        raw_keyword[row] = *score;
        had_keyword[row] = true;
    }
    for (id, score) in &vector {
        let row = row_for(id, &mut ids, &mut raw_keyword, &mut raw_vector, &mut had_keyword, &mut had_vector);
        raw_vector[row] = *score;
        had_vector[row] = true;
    }

    // Components are scaled among the candidates that actually have
    // them: a record no keyword search found should score zero for
    // keyword, not be dragged into that component's range.
    let keyword_component = component(&raw_keyword, &had_keyword, fusion);
    let vector_component = component(&raw_vector, &had_vector, fusion);

    let mut hits: Vec<Hit> = Vec::with_capacity(ids.len());
    for (row, id) in ids.iter().enumerate() {
        let Some(record) = lookup(id) else { continue };
        let recency = recency_score(record.updated_at, now, half_life);
        let importance = record.importance.clamp(0.0, 1.0);
        let score = weights.keyword * keyword_component[row]
            + weights.vector * vector_component[row]
            + weights.recency * recency
            + weights.importance * importance;
        hits.push(Hit {
            id: id.clone(),
            score,
            keyword: raw_keyword[row],
            vector: raw_vector[row],
            recency,
        });
    }

    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    hits
}

/// One component's contribution per candidate, zero where the
/// candidate never appeared in that list.
fn component(raw: &[f32], present: &[bool], fusion: Fusion) -> Vec<f32> {
    let found: Vec<f32> = raw
        .iter()
        .zip(present)
        .filter(|(_, present)| **present)
        .map(|(score, _)| *score)
        .collect();
    if found.is_empty() {
        return vec![0.0; raw.len()];
    }

    let mut scaled = match fusion {
        Fusion::Linear => {
            let mut values = found.clone();
            normalize(&mut values);
            values
        }
        Fusion::Rrf => ranks(&found).into_iter().map(|r| 1.0 / (RRF_K + r)).collect(),
    };
    scaled.reverse(); // popped from the back below, restoring input order

    raw.iter()
        .zip(present)
        .map(|(_, present)| {
            if *present {
                scaled.pop().unwrap_or(0.0)
            } else {
                0.0
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }

    #[test]
    fn recency_halves_at_each_half_life() {
        let half_life = Duration::from_secs(100);
        let now = SystemTime::now();
        approx(recency_score(now, now, half_life), 1.0);
        approx(
            recency_score(now - Duration::from_secs(100), now, half_life),
            0.5,
        );
        approx(
            recency_score(now - Duration::from_secs(200), now, half_life),
            0.25,
        );
    }

    #[test]
    fn a_future_timestamp_scores_as_brand_new() {
        let now = SystemTime::now();
        approx(
            recency_score(now + Duration::from_secs(60), now, Duration::from_secs(10)),
            1.0,
        );
    }

    #[test]
    fn normalize_spreads_scores_over_the_unit_range() {
        let mut values = vec![2.0, 4.0, 6.0];
        normalize(&mut values);
        assert_eq!(values, vec![0.0, 0.5, 1.0]);
    }

    #[test]
    fn normalize_treats_an_all_equal_component_as_full_agreement() {
        // The case that would otherwise divide by zero. Scoring them 0
        // would silently drop a component that matched everything.
        let mut values = vec![3.0, 3.0, 3.0];
        normalize(&mut values);
        assert_eq!(values, vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn ranks_are_one_based_and_ties_share_a_rank() {
        assert_eq!(ranks(&[0.9, 0.5, 0.7]), vec![1.0, 3.0, 2.0]);
        assert_eq!(ranks(&[0.5, 0.5, 0.1]), vec![1.0, 1.0, 3.0]);
    }

    fn record(id: &str, importance: f32) -> MemoryRecord {
        let mut r = MemoryRecord::new(id.as_bytes().to_vec(), b"text".to_vec(), SystemTime::now());
        r.importance = importance;
        r
    }

    /// Weights that isolate one component, so a test can assert about
    /// it without the other three moving the answer.
    fn only_keyword() -> Weights {
        Weights { keyword: 1.0, vector: 0.0, recency: 0.0, importance: 0.0 }
    }

    fn fuse_with(
        keyword: Vec<(&str, f32)>,
        vector: Vec<(&str, f32)>,
        weights: Weights,
        fusion: Fusion,
    ) -> Vec<Hit> {
        // One record per id mentioned, so a candidate is never dropped
        // just because the fixture forgot to declare it.
        let mut names: Vec<&str> = keyword.iter().chain(&vector).map(|(id, _)| *id).collect();
        names.sort();
        names.dedup();
        let owned: Vec<MemoryRecord> = names.iter().map(|id| record(id, 0.5)).collect();
        let to_pairs = |list: Vec<(&str, f32)>| -> Vec<(Bytes, f32)> {
            list.into_iter()
                .map(|(id, s)| (id.as_bytes().to_vec(), s))
                .collect()
        };
        fuse(
            to_pairs(keyword),
            to_pairs(vector),
            |id| owned.iter().find(|r| r.id == id),
            weights,
            fusion,
            Duration::from_secs(3600),
            SystemTime::now(),
        )
    }

    fn order(hits: &[Hit]) -> Vec<String> {
        hits.iter()
            .map(|h| String::from_utf8(h.id.clone()).unwrap())
            .collect()
    }

    #[test]
    fn a_candidate_found_by_one_index_only_still_ranks() {
        let hits = fuse_with(
            vec![("a", 5.0)],
            vec![("b", 0.9)],
            Weights::default(),
            Fusion::Linear,
        );
        assert_eq!(hits.len(), 2);
        // The vector weight is the larger of the two by default.
        assert_eq!(order(&hits)[0], "b");
        let a = hits.iter().find(|h| h.id == b"a").unwrap();
        assert_eq!(a.vector, 0.0, "a component the record lacks scores zero");
    }

    #[test]
    fn a_candidate_both_indexes_found_outranks_one_either_found_alone() {
        let hits = fuse_with(
            vec![("a", 5.0), ("both", 5.0)],
            vec![("b", 0.9), ("both", 0.9)],
            Weights::default(),
            Fusion::Linear,
        );
        assert_eq!(order(&hits)[0], "both");
    }

    #[test]
    fn weights_decide_which_component_leads() {
        let keyword_first = fuse_with(
            vec![("a", 9.0), ("b", 1.0)],
            vec![("a", 0.1), ("b", 0.9)],
            only_keyword(),
            Fusion::Linear,
        );
        assert_eq!(order(&keyword_first)[0], "a");

        let vector_first = fuse_with(
            vec![("a", 9.0), ("b", 1.0)],
            vec![("a", 0.1), ("b", 0.9)],
            Weights { keyword: 0.0, vector: 1.0, recency: 0.0, importance: 0.0 },
            Fusion::Linear,
        );
        assert_eq!(order(&vector_first)[0], "b");
    }

    #[test]
    fn components_are_reported_unnormalized() {
        let hits = fuse_with(
            vec![("a", 7.5)],
            vec![("a", 0.42)],
            Weights::default(),
            Fusion::Linear,
        );
        // WITHSCORES shows the client the real BM25 and cosine numbers,
        // not the rescaled ones fusion worked with.
        approx(hits[0].keyword, 7.5);
        approx(hits[0].vector, 0.42);
    }

    #[test]
    fn rrf_ranks_by_position_and_ignores_the_score_spread() {
        // One index scored everything nearly identically. Linear
        // normalization would stretch that noise across the full range;
        // RRF only sees the order.
        let hits = fuse_with(
            vec![("a", 1.0001), ("b", 1.0)],
            vec![("b", 0.9), ("a", 0.1)],
            Weights { keyword: 0.5, vector: 0.5, recency: 0.0, importance: 0.0 },
            Fusion::Rrf,
        );
        assert_eq!(order(&hits), vec!["a", "b"], "each led one list, so ties break on id");
        approx(hits[0].score, 0.5 / 61.0 + 0.5 / 62.0);
    }

    #[test]
    fn importance_and_recency_break_a_tie_the_other_components_leave() {
        let now = SystemTime::now();
        let mut old = record("old", 0.9);
        old.updated_at = now - Duration::from_secs(7200);
        let fresh = record("fresh", 0.1);
        let records = vec![old, fresh];
        let hits = fuse(
            vec![(b"old".to_vec(), 1.0), (b"fresh".to_vec(), 1.0)],
            Vec::new(),
            |id| records.iter().find(|r| r.id == id),
            Weights { keyword: 0.0, vector: 0.0, recency: 1.0, importance: 0.0 },
            Fusion::Linear,
            Duration::from_secs(3600),
            now,
        );
        assert_eq!(order(&hits)[0], "fresh");
        approx(hits[1].recency, 0.25);
    }

    #[test]
    fn fusing_two_empty_lists_gives_nothing() {
        assert!(fuse_with(Vec::new(), Vec::new(), Weights::default(), Fusion::Linear).is_empty());
    }
}
