//! One stored memory.
//!
//! The record owns its text, metadata, and timestamps. It does *not*
//! own its embedding: vectors live contiguously in the index's
//! [`VectorIndex`](super::vector::VectorIndex) so a search scans one
//! flat array instead of chasing a pointer per record, and the record
//! holds only the slot it was given.

use std::time::SystemTime;

use crate::util::bytes::Bytes;

/// What a record scores for importance when the client doesn't say.
/// Mid-scale, so an unspecified memory neither wins nor loses a tie.
pub const DEFAULT_IMPORTANCE: f32 = 0.5;

#[derive(Clone)]
pub struct MemoryRecord {
    pub id: Bytes,
    pub text: Bytes,
    /// Sorted by field, so replies and dumps are deterministic.
    meta: Vec<(Bytes, Bytes)>,
    pub importance: f32,
    pub created_at: SystemTime,
    pub updated_at: SystemTime,
    /// Per-record TTL, independent of the TTL on the key holding the
    /// whole index. This is what makes session and cache memory
    /// possible without another data structure.
    pub expire_at: Option<SystemTime>,
    /// Position in the owning index's vector array, if this record was
    /// stored with an embedding.
    pub slot: Option<usize>,
}

impl MemoryRecord {
    pub fn new(id: Bytes, text: Bytes, now: SystemTime) -> MemoryRecord {
        MemoryRecord {
            id,
            text,
            meta: Vec::new(),
            importance: DEFAULT_IMPORTANCE,
            created_at: now,
            updated_at: now,
            expire_at: None,
            slot: None,
        }
    }

    pub fn is_live(&self, now: SystemTime) -> bool {
        match self.expire_at {
            Some(deadline) => deadline > now,
            None => true,
        }
    }

    pub fn meta(&self) -> &[(Bytes, Bytes)] {
        &self.meta
    }

    /// Sets one metadata field, replacing any previous value. Returns
    /// whether the field is new, so `MEM.SETMETA` can count additions
    /// the way `HSET` does.
    pub fn set_meta(&mut self, field: Bytes, value: Bytes) -> bool {
        match self.meta.binary_search_by(|(f, _)| f.as_slice().cmp(&field)) {
            Ok(at) => {
                self.meta[at].1 = value;
                false
            }
            Err(at) => {
                self.meta.insert(at, (field, value));
                true
            }
        }
    }

    pub fn get_meta(&self, field: &[u8]) -> Option<&[u8]> {
        self.meta
            .binary_search_by(|(f, _)| f.as_slice().cmp(field))
            .ok()
            .map(|at| self.meta[at].1.as_slice())
    }

    pub fn remove_meta(&mut self, field: &[u8]) -> bool {
        match self.meta.binary_search_by(|(f, _)| f.as_slice().cmp(field)) {
            Ok(at) => {
                self.meta.remove(at);
                true
            }
            Err(_) => false,
        }
    }

    /// Roughly what this record costs in the heap, for `MEM.INFO` and
    /// the INFO section. The vector is counted by the vector index.
    pub fn heap_bytes(&self) -> usize {
        let meta: usize = self.meta.iter().map(|(f, v)| f.len() + v.len()).sum();
        self.id.len() + self.text.len() + meta + std::mem::size_of::<MemoryRecord>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> MemoryRecord {
        MemoryRecord::new(b"m1".to_vec(), b"hello".to_vec(), SystemTime::now())
    }

    #[test]
    fn meta_replaces_and_reports_novelty() {
        let mut r = record();
        assert!(r.set_meta(b"type".to_vec(), b"preference".to_vec()));
        assert!(!r.set_meta(b"type".to_vec(), b"fact".to_vec()));
        assert_eq!(r.get_meta(b"type"), Some(b"fact".as_slice()));
        assert_eq!(r.meta().len(), 1);
    }

    #[test]
    fn meta_stays_sorted_by_field() {
        let mut r = record();
        for field in [b"zeta".as_slice(), b"alpha", b"mid"] {
            r.set_meta(field.to_vec(), b"v".to_vec());
        }
        let fields: Vec<&[u8]> = r.meta().iter().map(|(f, _)| f.as_slice()).collect();
        assert_eq!(fields, vec![b"alpha".as_slice(), b"mid", b"zeta"]);
    }

    #[test]
    fn removing_an_absent_field_reports_false() {
        let mut r = record();
        assert!(!r.remove_meta(b"nope"));
        r.set_meta(b"nope".to_vec(), b"v".to_vec());
        assert!(r.remove_meta(b"nope"));
    }

    #[test]
    fn expiry_is_checked_against_a_caller_supplied_now() {
        let mut r = record();
        assert!(r.is_live(SystemTime::now()));
        r.expire_at = Some(SystemTime::now() - std::time::Duration::from_secs(1));
        assert!(!r.is_live(SystemTime::now()));
    }
}
