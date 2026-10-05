//! Embedding storage and similarity.
//!
//! Vectors sit end to end in one `Vec<f32>`: slot `i` occupies
//! `data[i * dim .. (i + 1) * dim]`. A search is then a single forward
//! scan over contiguous memory, which is the layout that matters most
//! for brute-force top-k. Deleted slots go on a free list and are
//! reused, so a churning index doesn't grow without bound.
//!
//! Cosine vectors are normalized once on the way in, which turns every
//! later comparison into a plain dot product.

use crate::util::bytes::{eq_ignore_case, Bytes};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Cosine,
    L2,
    InnerProduct,
}

impl Metric {
    pub fn parse(word: &[u8]) -> Option<Metric> {
        if eq_ignore_case(word, "COSINE") {
            Some(Metric::Cosine)
        } else if eq_ignore_case(word, "L2") {
            Some(Metric::L2)
        } else if eq_ignore_case(word, "IP") || eq_ignore_case(word, "INNERPRODUCT") {
            Some(Metric::InnerProduct)
        } else {
            None
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Metric::Cosine => "COSINE",
            Metric::L2 => "L2",
            Metric::InnerProduct => "IP",
        }
    }

    /// Whether stored vectors are unit-normalized under this metric.
    pub fn normalizes(self) -> bool {
        self == Metric::Cosine
    }
}

/// Why a vector was refused.
#[derive(Debug, PartialEq)]
pub enum VectorError {
    /// Length isn't a multiple of four, so it isn't float32 bytes.
    NotFloats,
    /// Right shape, wrong size.
    WrongDimension { expected: usize, got: usize },
    /// A NaN or infinity, which no distance function can rank.
    NotFinite,
    /// All zeros, which has no direction to compare against.
    ZeroVector,
}

/// Decodes a raw little-endian float32 blob - the form every client
/// already has after `struct.pack` or a `Float32Array`, and four bytes
/// per dimension on the wire instead of the fifteen a decimal string
/// would cost.
pub fn parse_le_f32(blob: &[u8]) -> Result<Vec<f32>, VectorError> {
    if !blob.len().is_multiple_of(4) {
        return Err(VectorError::NotFloats);
    }
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|c| {
            let value = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
            if value.is_finite() {
                Ok(value)
            } else {
                Err(VectorError::NotFinite)
            }
        })
        .collect()
}

/// The same encoding, for handing a stored vector back to a client.
pub fn encode_le_f32(values: &[f32]) -> Bytes {
    let mut out = Vec::with_capacity(values.len() * 4);
    for value in values {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out
}

pub fn norm(values: &[f32]) -> f32 {
    values.iter().map(|v| v * v).sum::<f32>().sqrt()
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn l2_distance(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y) * (x - y))
        .sum::<f32>()
        .sqrt()
}

pub struct VectorIndex {
    dim: usize,
    metric: Metric,
    data: Vec<f32>,
    /// Slots whose record was deleted, ready to be handed out again.
    free: Vec<usize>,
    live: usize,
}

impl VectorIndex {
    /// A `dim` of zero makes a disabled index: it accepts nothing,
    /// which is what a `SEARCH`-mode memory wants.
    pub fn new(dim: usize, metric: Metric) -> VectorIndex {
        VectorIndex {
            dim,
            metric,
            data: Vec::new(),
            free: Vec::new(),
            live: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.live
    }

    /// Heap held by the float array, including slots on the free list.
    pub fn heap_bytes(&self) -> usize {
        self.data.capacity() * 4
    }

    /// Checks shape and, for cosine, direction. Separated from
    /// `insert` so a command can reject a bad vector before it has
    /// mutated anything else.
    pub fn validate(&self, values: &[f32]) -> Result<(), VectorError> {
        if values.len() != self.dim {
            return Err(VectorError::WrongDimension {
                expected: self.dim,
                got: values.len(),
            });
        }
        if self.metric.normalizes() && norm(values) == 0.0 {
            return Err(VectorError::ZeroVector);
        }
        Ok(())
    }

    /// Normalizes in place when the metric calls for it. Applied to
    /// stored vectors and to query vectors alike, so both sides of a
    /// cosine comparison are unit length.
    pub fn prepare(&self, values: &mut [f32]) {
        if !self.metric.normalizes() {
            return;
        }
        let length = norm(values);
        if length > 0.0 {
            for value in values.iter_mut() {
                *value /= length;
            }
        }
    }

    /// Stores `values` and returns its slot. The caller must have
    /// passed [`validate`](Self::validate) first.
    pub fn insert(&mut self, values: &[f32]) -> usize {
        let slot = match self.free.pop() {
            Some(slot) => slot,
            None => {
                self.data.resize(self.data.len() + self.dim, 0.0);
                self.data.len() / self.dim - 1
            }
        };
        self.live += 1;
        self.write(slot, values);
        slot
    }

    /// Replaces the vector in an existing slot, for `MEM.SETTEXT`.
    pub fn overwrite(&mut self, slot: usize, values: &[f32]) {
        self.write(slot, values);
    }

    fn write(&mut self, slot: usize, values: &[f32]) {
        let at = slot * self.dim;
        let target = &mut self.data[at..at + self.dim];
        target.copy_from_slice(values);
        if self.metric.normalizes() {
            let length = norm(target);
            if length > 0.0 {
                for value in target.iter_mut() {
                    *value /= length;
                }
            }
        }
    }

    pub fn remove(&mut self, slot: usize) {
        let at = slot * self.dim;
        if at + self.dim > self.data.len() || self.free.contains(&slot) {
            return;
        }
        self.data[at..at + self.dim].fill(0.0);
        self.free.push(slot);
        self.live -= 1;
    }

    pub fn get(&self, slot: usize) -> Option<&[f32]> {
        let at = slot.checked_mul(self.dim)?;
        self.data.get(at..at + self.dim)
    }

    /// Comparisons a full scan of this index would cost, so a caller
    /// can refuse a query before running it rather than during.
    pub fn scan_cost(&self) -> usize {
        self.live
    }

    /// How well `query` matches the vector in `slot`, higher being
    /// better for every metric. L2 is a distance, so it is negated -
    /// callers only ever compare and rank these, never read them as
    /// absolute distances.
    pub fn similarity(&self, slot: usize, query: &[f32]) -> Option<f32> {
        let stored = self.get(slot)?;
        Some(match self.metric {
            Metric::Cosine | Metric::InnerProduct => dot(stored, query),
            Metric::L2 => -l2_distance(stored, query),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }

    #[test]
    fn blobs_round_trip_through_float32() {
        let values = vec![0.023, -0.182, 0.441];
        let blob = encode_le_f32(&values);
        assert_eq!(blob.len(), 12);
        assert_eq!(parse_le_f32(&blob).unwrap(), values);
    }

    #[test]
    fn a_blob_that_is_not_float32_is_rejected() {
        assert_eq!(parse_le_f32(b"12345"), Err(VectorError::NotFloats));
        let nan = encode_le_f32(&[f32::NAN]);
        assert_eq!(parse_le_f32(&nan), Err(VectorError::NotFinite));
    }

    #[test]
    fn cosine_normalizes_on_the_way_in() {
        let mut index = VectorIndex::new(2, Metric::Cosine);
        let slot = index.insert(&[3.0, 4.0]);
        let stored = index.get(slot).unwrap();
        approx(norm(stored), 1.0);
        approx(stored[0], 0.6);
        approx(stored[1], 0.8);
    }

    #[test]
    fn cosine_of_identical_directions_is_one() {
        let mut index = VectorIndex::new(2, Metric::Cosine);
        let slot = index.insert(&[3.0, 4.0]);
        let mut query = vec![30.0, 40.0];
        index.prepare(&mut query);
        approx(index.similarity(slot, &query).unwrap(), 1.0);
    }

    #[test]
    fn l2_ranks_nearer_vectors_higher() {
        let mut index = VectorIndex::new(2, Metric::L2);
        let near = index.insert(&[1.0, 1.0]);
        let far = index.insert(&[9.0, 9.0]);
        let query = [1.0, 2.0];
        assert!(index.similarity(near, &query) > index.similarity(far, &query));
    }

    #[test]
    fn wrong_dimension_and_zero_vectors_are_refused() {
        let index = VectorIndex::new(3, Metric::Cosine);
        assert_eq!(
            index.validate(&[1.0, 2.0]),
            Err(VectorError::WrongDimension {
                expected: 3,
                got: 2
            })
        );
        assert_eq!(
            index.validate(&[0.0, 0.0, 0.0]),
            Err(VectorError::ZeroVector)
        );
        // Inner product has no direction requirement, so zero is fine.
        let ip = VectorIndex::new(3, Metric::InnerProduct);
        assert!(ip.validate(&[0.0, 0.0, 0.0]).is_ok());
    }

    #[test]
    fn deleted_slots_are_reused_rather_than_grown_past() {
        let mut index = VectorIndex::new(2, Metric::InnerProduct);
        let first = index.insert(&[1.0, 0.0]);
        index.insert(&[0.0, 1.0]);
        index.remove(first);
        assert_eq!(index.len(), 1);
        let reused = index.insert(&[5.0, 5.0]);
        assert_eq!(reused, first);
        assert_eq!(index.heap_bytes(), 4 * 4);
    }

    #[test]
    fn removing_a_slot_twice_does_not_double_free() {
        let mut index = VectorIndex::new(2, Metric::InnerProduct);
        let slot = index.insert(&[1.0, 0.0]);
        index.remove(slot);
        index.remove(slot);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn a_zero_dimension_index_stores_nothing() {
        let index = VectorIndex::new(0, Metric::Cosine);
        assert_eq!(index.len(), 0);
        assert!(index.validate(&[1.0]).is_err());
    }
}
