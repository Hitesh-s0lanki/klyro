//! Memory: Klyro's data structure for AI agent retrieval.
//!
//! One key holds one memory index; one index holds many records. The
//! three structures the product describes - Search, Vector, and
//! Hybrid - are three *modes* of this one type rather than three
//! implementations, because they differ only in which indexes they
//! maintain and which queries they will answer. Sharing the record
//! store means a hybrid query never has to join two half-populated
//! structures.
//!
//! ```text
//!   MEM.CREATE user:123 MODE HYBRID DIM 384
//!                     │
//!         ┌───────────┴───────────┐
//!         ▼                       ▼
//!    TextIndex (BM25)      VectorIndex (cosine)
//!         └───────────┬───────────┘
//!                     ▼
//!               MemoryRecord
//! ```

pub mod filter;
pub mod fuse;
pub mod record;
pub mod text;
pub mod vector;

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::util::bytes::{eq_ignore_case, Bytes};
use filter::Filter;
use fuse::{fuse, Fusion};
use record::MemoryRecord;
use text::TextIndex;
use vector::{Metric, VectorError, VectorIndex};

/// Wall-clock milliseconds since the epoch. Timestamps cross the wire
/// and land in dumps as absolute instants, so downtime is accounted
/// for the same way key expiry already accounts for it.
pub fn unix_millis(at: SystemTime) -> i64 {
    match at.duration_since(UNIX_EPOCH) {
        Ok(since) => since.as_millis() as i64,
        Err(_) => 0,
    }
}

pub fn from_unix_millis(millis: i64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis.max(0) as u64)
}

/// Which retrieval structure a key is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Keyword only. No embeddings, no vector index, lowest cost.
    Search,
    /// Semantic only. Text is stored and returned, but not indexed.
    Vector,
    /// Both, fused. The mode an agent should reach for by default.
    Hybrid,
}

impl Mode {
    pub fn parse(word: &[u8]) -> Option<Mode> {
        for (name, mode) in [
            ("SEARCH", Mode::Search),
            ("VECTOR", Mode::Vector),
            ("HYBRID", Mode::Hybrid),
        ] {
            if eq_ignore_case(word, name) {
                return Some(mode);
            }
        }
        None
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Search => "SEARCH",
            Mode::Vector => "VECTOR",
            Mode::Hybrid => "HYBRID",
        }
    }

    pub fn indexes_text(self) -> bool {
        matches!(self, Mode::Search | Mode::Hybrid)
    }

    pub fn stores_vectors(self) -> bool {
        matches!(self, Mode::Vector | Mode::Hybrid)
    }
}

/// How a hybrid query weighs its four signals. The defaults are the
/// ones the product specification names.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weights {
    pub keyword: f32,
    pub vector: f32,
    pub recency: f32,
    pub importance: f32,
}

impl Default for Weights {
    fn default() -> Weights {
        Weights {
            keyword: 0.35,
            vector: 0.50,
            recency: 0.10,
            importance: 0.05,
        }
    }
}

impl Weights {
    /// Weights must be finite and non-negative. They are deliberately
    /// *not* required to sum to one: a caller who wants only recency
    /// broken out of an otherwise unweighted query should be able to
    /// say so, and every score is comparable only within one result set
    /// anyway.
    pub fn is_valid(&self) -> bool {
        [self.keyword, self.vector, self.recency, self.importance]
            .iter()
            .all(|w| w.is_finite() && *w >= 0.0)
    }
}

#[derive(Clone, Debug)]
pub struct MemoryConfig {
    pub mode: Mode,
    /// Embedding width. Zero for `SEARCH`, fixed at creation otherwise,
    /// because every stored vector is laid out against it.
    pub dim: usize,
    pub metric: Metric,
    pub weights: Weights,
    /// How long a memory takes to lose half its recency score.
    pub half_life: Duration,
}

impl MemoryConfig {
    pub fn new(mode: Mode, dim: usize, metric: Metric) -> MemoryConfig {
        MemoryConfig {
            mode,
            dim: if mode.stores_vectors() { dim } else { 0 },
            metric,
            weights: Weights::default(),
            half_life: Duration::from_secs(7 * 24 * 3600),
        }
    }
}

/// Why an add or update was refused.
#[derive(Debug, PartialEq)]
pub enum MemoryError {
    /// This mode keeps no vector index.
    VectorsNotSupported,
    /// This mode indexes no text, so it cannot answer a keyword query.
    TextNotSupported,
    /// A vector-mode index needs a vector on every record.
    VectorRequired,
    Vector(VectorError),
    NoSuchRecord,
    /// A brute-force scan of this index would exceed `mem-max-scan`.
    /// Refused rather than answered from part of the index, because a
    /// silently partial answer is worse than none.
    TooLargeToScan {
        records: usize,
        limit: usize,
    },
    /// `NX` was given and the id exists, or `XX` and it does not.
    ExistenceUnmet,
    /// `mem-max-records` would be exceeded.
    Full {
        limit: usize,
    },
}

impl From<VectorError> for MemoryError {
    fn from(e: VectorError) -> MemoryError {
        MemoryError::Vector(e)
    }
}

/// Everything `MEM.ADD` can carry, gathered so the command layer parses
/// arguments and this layer applies them.
#[derive(Default)]
pub struct AddRequest {
    pub id: Option<Bytes>,
    pub text: Bytes,
    pub vector: Option<Vec<f32>>,
    pub meta: Vec<(Bytes, Bytes)>,
    pub importance: Option<f32>,
    pub ttl: Option<Duration>,
    pub require_new: bool,
    pub require_existing: bool,
}

/// One ranked result, with the components that produced its score kept
/// separate so `WITHSCORES` can show the client why it ranked there.
/// Importance is not among them: it is a stored field of the record,
/// which every reply already carries.
#[derive(Clone, Debug)]
pub struct Hit {
    pub id: Bytes,
    pub score: f32,
    pub keyword: f32,
    pub vector: f32,
    pub recency: f32,
}

pub struct Memory {
    config: MemoryConfig,
    records: HashMap<Bytes, MemoryRecord>,
    text: TextIndex,
    vectors: VectorIndex,
    /// Backs server-assigned ids. Monotonic for the life of the index,
    /// and persisted, so a reloaded index never reissues an id.
    next_id: u64,
}

impl Memory {
    pub fn new(config: MemoryConfig) -> Memory {
        let vectors = VectorIndex::new(config.dim, config.metric);
        Memory {
            config,
            records: HashMap::new(),
            text: TextIndex::new(),
            vectors,
            next_id: 1,
        }
    }

    pub fn config(&self) -> &MemoryConfig {
        &self.config
    }

    pub fn set_weights(&mut self, weights: Weights) {
        self.config.weights = weights;
    }

    pub fn set_half_life(&mut self, half_life: Duration) {
        self.config.half_life = half_life;
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn term_count(&self) -> usize {
        self.text.term_count()
    }

    pub fn avg_doc_len(&self) -> f32 {
        self.text.avg_doc_len()
    }

    pub fn vector_count(&self) -> usize {
        self.vectors.len()
    }

    pub fn heap_bytes(&self) -> usize {
        self.records.values().map(|r| r.heap_bytes()).sum::<usize>()
            + self.text.heap_bytes()
            + self.vectors.heap_bytes()
    }

    pub fn get(&self, id: &[u8]) -> Option<&MemoryRecord> {
        self.records
            .get(id)
            .filter(|r| r.is_live(SystemTime::now()))
    }

    pub fn vector_of(&self, record: &MemoryRecord) -> Option<&[f32]> {
        self.vectors.get(record.slot?)
    }

    /// Prepares a client-supplied query vector for comparison against
    /// stored ones - normalizing it under cosine, so both sides are
    /// unit length.
    pub fn prepare_query_vector(&self, values: &mut [f32]) {
        self.vectors.prepare(values);
    }

    /// Ids in sorted order. Used by `MEM.SCAN`, which pages through a
    /// stable snapshot the same way the keyspace `SCAN` does.
    fn sorted_ids(&self) -> Vec<&Bytes> {
        let now = SystemTime::now();
        let mut ids: Vec<&Bytes> = self
            .records
            .values()
            .filter(|r| r.is_live(now))
            .map(|r| &r.id)
            .collect();
        ids.sort();
        ids
    }

    /// Up to `count` live ids from `cursor`, plus the cursor to resume
    /// from, or `0` when the index has been walked.
    pub fn scan(&self, cursor: usize, count: usize, filter: &Filter) -> (Vec<Bytes>, usize) {
        let now = SystemTime::now();
        let ids = self.sorted_ids();
        if cursor >= ids.len() {
            return (Vec::new(), 0);
        }
        let end = (cursor + count.max(1)).min(ids.len());
        let batch = ids[cursor..end]
            .iter()
            .filter(|id| {
                self.records
                    .get(**id)
                    .is_some_and(|r| filter.matches(r, now))
            })
            .map(|id| (*id).clone())
            .collect();
        let next = if end >= ids.len() { 0 } else { end };
        (batch, next)
    }

    pub fn add(
        &mut self,
        request: AddRequest,
        max_terms: usize,
        max_records: usize,
    ) -> Result<Bytes, MemoryError> {
        let now = SystemTime::now();
        let id = match request.id {
            Some(id) => id,
            None => {
                let id = format!("m{}", self.next_id).into_bytes();
                self.next_id += 1;
                id
            }
        };

        let existing = self.records.get(&id).filter(|r| r.is_live(now)).is_some();
        if (request.require_new && existing) || (request.require_existing && !existing) {
            return Err(MemoryError::ExistenceUnmet);
        }
        if !existing && max_records > 0 && self.records.len() >= max_records {
            return Err(MemoryError::Full { limit: max_records });
        }

        match (&request.vector, self.config.mode.stores_vectors()) {
            (Some(values), true) => self.vectors.validate(values)?,
            (Some(_), false) => return Err(MemoryError::VectorsNotSupported),
            // A vector index whose records have no vectors could answer
            // nothing, so refusing here beats a silently useless store.
            (None, _) if self.config.mode == Mode::Vector && !existing => {
                return Err(MemoryError::VectorRequired)
            }
            _ => {}
        }

        // Everything is validated, so from here nothing can fail
        // partway and leave the indexes disagreeing with the records.
        let previous = self.records.get(&id).map(|r| r.text.clone());
        let mut updated = match self.records.remove(&id) {
            Some(mut record) => {
                record.text = request.text;
                record.updated_at = now;
                record
            }
            None => MemoryRecord::new(id.clone(), request.text, now),
        };
        for (field, value) in request.meta {
            updated.set_meta(field, value);
        }
        if let Some(importance) = request.importance {
            updated.importance = importance.clamp(0.0, 1.0);
        }
        if let Some(ttl) = request.ttl {
            updated.expire_at = Some(now + ttl);
        }
        if let Some(values) = request.vector {
            updated.slot = Some(match updated.slot {
                Some(slot) => {
                    self.vectors.overwrite(slot, &values);
                    slot
                }
                None => self.vectors.insert(&values),
            });
        }
        if self.config.mode.indexes_text() {
            self.text
                .index(&id, previous.as_deref(), &updated.text, max_terms);
        }
        self.records.insert(id.clone(), updated);
        Ok(id)
    }

    /// Drops one record and everything indexed under it.
    pub fn del(&mut self, id: &[u8], max_terms: usize) -> bool {
        let Some(record) = self.records.remove(id) else {
            return false;
        };
        if self.config.mode.indexes_text() {
            self.text.remove(id, &record.text, max_terms);
        }
        if let Some(slot) = record.slot {
            self.vectors.remove(slot);
        }
        true
    }

    pub fn set_meta(
        &mut self,
        id: &[u8],
        pairs: Vec<(Bytes, Bytes)>,
    ) -> Result<usize, MemoryError> {
        let now = SystemTime::now();
        let record = self
            .records
            .get_mut(id)
            .filter(|r| r.is_live(now))
            .ok_or(MemoryError::NoSuchRecord)?;
        let added = pairs
            .into_iter()
            .filter(|(field, value)| record.set_meta(field.clone(), value.clone()))
            .count();
        record.updated_at = now;
        Ok(added)
    }

    pub fn remove_meta(&mut self, id: &[u8], fields: &[Bytes]) -> Result<usize, MemoryError> {
        let now = SystemTime::now();
        let record = self
            .records
            .get_mut(id)
            .filter(|r| r.is_live(now))
            .ok_or(MemoryError::NoSuchRecord)?;
        let removed = fields.iter().filter(|f| record.remove_meta(f)).count();
        record.updated_at = now;
        Ok(removed)
    }

    /// Sets or clears one record's own deadline, independent of the TTL
    /// on the key holding the index.
    pub fn set_record_ttl(&mut self, id: &[u8], ttl: Option<Duration>) -> Result<(), MemoryError> {
        let now = SystemTime::now();
        let record = self
            .records
            .get_mut(id)
            .filter(|r| r.is_live(now))
            .ok_or(MemoryError::NoSuchRecord)?;
        record.expire_at = ttl.map(|d| now + d);
        Ok(())
    }

    /// Milliseconds until this record expires: -2 missing, -1 no TTL.
    pub fn record_pttl(&self, id: &[u8]) -> i64 {
        let now = SystemTime::now();
        match self.records.get(id).filter(|r| r.is_live(now)) {
            None => -2,
            Some(record) => match record.expire_at {
                None => -1,
                Some(deadline) => deadline
                    .duration_since(now)
                    .map_or(0, |left| left.as_millis() as i64),
            },
        }
    }

    /// Drops records whose own TTL has passed. Called from the server's
    /// periodic tick, alongside the keyspace sweep.
    pub fn sweep_expired(&mut self, max_terms: usize) -> usize {
        let now = SystemTime::now();
        let dead: Vec<Bytes> = self
            .records
            .values()
            .filter(|r| !r.is_live(now))
            .map(|r| r.id.clone())
            .collect();
        for id in &dead {
            self.del(id, max_terms);
        }
        dead.len()
    }

    /// Brute-force top-k over the vector index.
    ///
    /// Exact, not approximate. At 384 dimensions a hundred thousand
    /// records is a few milliseconds of contiguous scanning, which is
    /// the right trade before there is a workload to tune against - and
    /// an exact scan is the reference an approximate index gets
    /// checked against later.
    ///
    /// The cap matters more than it looks: Klyro is single-threaded, so
    /// an unbounded scan stalls every other client on the server.
    pub fn search_vector(
        &self,
        query: &[f32],
        limit: usize,
        filter: &Filter,
        max_scan: usize,
    ) -> Result<Vec<(Bytes, f32)>, MemoryError> {
        if !self.config.mode.stores_vectors() {
            return Err(MemoryError::VectorsNotSupported);
        }
        if query.len() != self.config.dim {
            return Err(MemoryError::Vector(VectorError::WrongDimension {
                expected: self.config.dim,
                got: query.len(),
            }));
        }
        if self.vectors.scan_cost() > max_scan {
            return Err(MemoryError::TooLargeToScan {
                records: self.vectors.scan_cost(),
                limit: max_scan,
            });
        }

        let now = SystemTime::now();
        let mut scored: Vec<(Bytes, f32)> = self
            .records
            .values()
            .filter(|record| filter.matches(record, now))
            .filter_map(|record| {
                let score = self.vectors.similarity(record.slot?, query)?;
                Some((record.id.clone(), score))
            })
            .collect();
        // Ties break on id, so a result page is stable across calls.
        scored.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        scored.truncate(limit);
        Ok(scored)
    }

    /// The hybrid query: whichever indexes the caller gave input for,
    /// fused into one ranking.
    ///
    /// Given only text this is a keyword search, given only a vector a
    /// semantic one, and given both a fusion of the two - so one
    /// command serves all three structures and an agent does not have
    /// to decide which to call.
    #[allow(clippy::too_many_arguments)]
    pub fn query(
        &self,
        text_query: Option<&[u8]>,
        vector_query: Option<&[f32]>,
        limit: usize,
        filter: &Filter,
        weights: Weights,
        fusion: Fusion,
        max_terms: usize,
        max_scan: usize,
    ) -> Result<Vec<Hit>, MemoryError> {
        let keyword = match text_query {
            Some(query) => self.search_text(query, limit, filter, max_terms)?,
            None => Vec::new(),
        };
        let vector = match vector_query {
            Some(query) => self.search_vector(query, limit, filter, max_scan)?,
            None => Vec::new(),
        };
        Ok(fuse(
            keyword,
            vector,
            |id| self.records.get(id),
            weights,
            fusion,
            self.config.half_life,
            SystemTime::now(),
        ))
    }

    /// BM25 over the keyword index, filtered and capped.
    pub fn search_text(
        &self,
        query: &[u8],
        limit: usize,
        filter: &Filter,
        max_terms: usize,
    ) -> Result<Vec<(Bytes, f32)>, MemoryError> {
        if !self.config.mode.indexes_text() {
            return Err(MemoryError::TextNotSupported);
        }
        let now = SystemTime::now();
        let terms = text::tokenize(query, max_terms);
        Ok(self.text.search(&terms, limit, |id| {
            self.records
                .get(id)
                .is_some_and(|record| filter.matches(record, now))
        }))
    }

    /// Every record the dump writer needs, in a stable order.
    pub fn records_for_dump(&self) -> Vec<(&MemoryRecord, Option<&[f32]>)> {
        let now = SystemTime::now();
        let mut ids = self.sorted_ids();
        ids.sort();
        ids.into_iter()
            .filter_map(|id| self.records.get(id))
            .filter(|r| r.is_live(now))
            .map(|r| (r, r.slot.and_then(|slot| self.vectors.get(slot))))
            .collect()
    }

    pub fn next_id(&self) -> u64 {
        self.next_id
    }

    /// Reinstates a record read back from a dump, rebuilding the
    /// indexes as it goes. Vectors come back already normalized, so
    /// this must not renormalize - `VectorIndex::insert` is idempotent
    /// on a unit vector, which is what makes that safe.
    pub fn load_record(
        &mut self,
        record: MemoryRecord,
        vector: Option<Vec<f32>>,
        max_terms: usize,
    ) {
        let mut record = record;
        if self.config.mode.indexes_text() {
            self.text.index(&record.id, None, &record.text, max_terms);
        }
        record.slot = match vector {
            Some(values)
                if self.config.mode.stores_vectors() && values.len() == self.config.dim =>
            {
                Some(self.vectors.insert(&values))
            }
            _ => None,
        };
        self.records.insert(record.id.clone(), record);
    }

    pub fn set_next_id(&mut self, next_id: u64) {
        self.next_id = next_id.max(1);
    }
}

impl Clone for Memory {
    /// `COPY` deep-copies a key, so a copied index is rebuilt from its
    /// records rather than sharing them. Rebuilding also compacts the
    /// vector array, dropping any slots the original had freed.
    fn clone(&self) -> Memory {
        let mut copy = Memory::new(self.config.clone());
        // The term cap only bounds indexing work; re-deriving it here
        // from what was already indexed would need the original cap,
        // and any document already in the index is by definition under
        // whatever cap admitted it.
        let max_terms = usize::MAX;
        for (record, vector) in self.records_for_dump() {
            copy.load_record(record.clone(), vector.map(|v| v.to_vec()), max_terms);
        }
        copy.set_next_id(self.next_id);
        copy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: usize = 1024;

    fn hybrid() -> Memory {
        Memory::new(MemoryConfig::new(Mode::Hybrid, 3, Metric::Cosine))
    }

    fn add(memory: &mut Memory, id: &str, text: &str, vector: Option<Vec<f32>>) -> Bytes {
        memory
            .add(
                AddRequest {
                    id: Some(id.as_bytes().to_vec()),
                    text: text.as_bytes().to_vec(),
                    vector,
                    ..AddRequest::default()
                },
                CAP,
                0,
            )
            .unwrap()
    }

    #[test]
    fn add_indexes_text_and_stores_the_vector() {
        let mut memory = hybrid();
        add(
            &mut memory,
            "m1",
            "User prefers PostgreSQL",
            Some(vec![1.0, 0.0, 0.0]),
        );
        assert_eq!(memory.len(), 1);
        assert_eq!(memory.vector_count(), 1);
        let hits = memory
            .search_text(b"postgresql", 10, &Filter::default(), CAP)
            .unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ids_are_assigned_when_the_client_gives_none() {
        let mut memory = hybrid();
        let first = memory
            .add(
                AddRequest {
                    text: b"one".to_vec(),
                    vector: Some(vec![1.0, 0.0, 0.0]),
                    ..AddRequest::default()
                },
                CAP,
                0,
            )
            .unwrap();
        assert_eq!(first, b"m1".to_vec());
        assert_eq!(memory.next_id(), 2);
    }

    #[test]
    fn updating_a_record_reindexes_its_text() {
        let mut memory = hybrid();
        add(&mut memory, "m1", "alpha", Some(vec![1.0, 0.0, 0.0]));
        add(&mut memory, "m1", "beta", Some(vec![0.0, 1.0, 0.0]));
        assert_eq!(memory.len(), 1);
        assert_eq!(memory.vector_count(), 1, "the slot should be reused");
        assert!(memory
            .search_text(b"alpha", 10, &Filter::default(), CAP)
            .unwrap()
            .is_empty());
        assert_eq!(
            memory
                .search_text(b"beta", 10, &Filter::default(), CAP)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn deleting_clears_both_indexes() {
        let mut memory = hybrid();
        add(&mut memory, "m1", "alpha", Some(vec![1.0, 0.0, 0.0]));
        assert!(memory.del(b"m1", CAP));
        assert!(!memory.del(b"m1", CAP));
        assert_eq!(memory.len(), 0);
        assert_eq!(memory.vector_count(), 0);
        assert_eq!(memory.term_count(), 0);
    }

    #[test]
    fn search_mode_refuses_vectors_and_vector_mode_refuses_keyword_queries() {
        let mut search = Memory::new(MemoryConfig::new(Mode::Search, 0, Metric::Cosine));
        let refused = search.add(
            AddRequest {
                text: b"x".to_vec(),
                vector: Some(vec![1.0]),
                ..AddRequest::default()
            },
            CAP,
            0,
        );
        assert_eq!(refused, Err(MemoryError::VectorsNotSupported));

        let vector = Memory::new(MemoryConfig::new(Mode::Vector, 3, Metric::Cosine));
        assert_eq!(
            vector.search_text(b"x", 10, &Filter::default(), CAP),
            Err(MemoryError::TextNotSupported)
        );
    }

    #[test]
    fn vector_mode_requires_a_vector_on_a_new_record() {
        let mut memory = Memory::new(MemoryConfig::new(Mode::Vector, 3, Metric::Cosine));
        let refused = memory.add(
            AddRequest {
                text: b"x".to_vec(),
                ..AddRequest::default()
            },
            CAP,
            0,
        );
        assert_eq!(refused, Err(MemoryError::VectorRequired));
    }

    #[test]
    fn nx_and_xx_gate_on_existence() {
        let mut memory = hybrid();
        let xx = memory.add(
            AddRequest {
                id: Some(b"m1".to_vec()),
                text: b"x".to_vec(),
                vector: Some(vec![1.0, 0.0, 0.0]),
                require_existing: true,
                ..AddRequest::default()
            },
            CAP,
            0,
        );
        assert_eq!(xx, Err(MemoryError::ExistenceUnmet));
        add(&mut memory, "m1", "x", Some(vec![1.0, 0.0, 0.0]));
        let nx = memory.add(
            AddRequest {
                id: Some(b"m1".to_vec()),
                text: b"y".to_vec(),
                vector: Some(vec![1.0, 0.0, 0.0]),
                require_new: true,
                ..AddRequest::default()
            },
            CAP,
            0,
        );
        assert_eq!(nx, Err(MemoryError::ExistenceUnmet));
    }

    #[test]
    fn a_refused_add_leaves_nothing_behind() {
        let mut memory = hybrid();
        let refused = memory.add(
            AddRequest {
                id: Some(b"m1".to_vec()),
                text: b"x".to_vec(),
                vector: Some(vec![1.0, 0.0]), // wrong dimension
                ..AddRequest::default()
            },
            CAP,
            0,
        );
        assert!(refused.is_err());
        assert_eq!(memory.len(), 0);
        assert_eq!(memory.vector_count(), 0);
        assert_eq!(memory.term_count(), 0);
    }

    #[test]
    fn max_records_stops_growth_but_still_allows_updates() {
        let mut memory = hybrid();
        memory
            .add(
                AddRequest {
                    id: Some(b"m1".to_vec()),
                    text: b"x".to_vec(),
                    vector: Some(vec![1.0, 0.0, 0.0]),
                    ..AddRequest::default()
                },
                CAP,
                1,
            )
            .unwrap();
        let full = memory.add(
            AddRequest {
                id: Some(b"m2".to_vec()),
                text: b"y".to_vec(),
                vector: Some(vec![1.0, 0.0, 0.0]),
                ..AddRequest::default()
            },
            CAP,
            1,
        );
        assert_eq!(full, Err(MemoryError::Full { limit: 1 }));
        // Updating the record already there is still allowed.
        assert!(memory
            .add(
                AddRequest {
                    id: Some(b"m1".to_vec()),
                    text: b"z".to_vec(),
                    vector: Some(vec![1.0, 0.0, 0.0]),
                    ..AddRequest::default()
                },
                CAP,
                1,
            )
            .is_ok());
    }

    #[test]
    fn expired_records_disappear_from_reads_before_the_sweep() {
        let mut memory = hybrid();
        add(&mut memory, "m1", "alpha", Some(vec![1.0, 0.0, 0.0]));
        memory
            .set_record_ttl(b"m1", Some(Duration::from_millis(1)))
            .unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(memory.get(b"m1").is_none());
        assert!(memory
            .search_text(b"alpha", 10, &Filter::default(), CAP)
            .unwrap()
            .is_empty());
        assert_eq!(memory.sweep_expired(CAP), 1);
        assert_eq!(memory.len(), 0);
    }

    #[test]
    fn record_ttl_reports_the_way_pttl_does() {
        let mut memory = hybrid();
        add(&mut memory, "m1", "alpha", Some(vec![1.0, 0.0, 0.0]));
        assert_eq!(memory.record_pttl(b"missing"), -2);
        assert_eq!(memory.record_pttl(b"m1"), -1);
        memory
            .set_record_ttl(b"m1", Some(Duration::from_secs(60)))
            .unwrap();
        assert!(memory.record_pttl(b"m1") > 59_000);
        memory.set_record_ttl(b"m1", None).unwrap();
        assert_eq!(memory.record_pttl(b"m1"), -1);
    }

    #[test]
    fn scan_walks_every_record_once() {
        let mut memory = hybrid();
        for i in 0..7 {
            add(
                &mut memory,
                &format!("m{i}"),
                "text",
                Some(vec![1.0, 0.0, 0.0]),
            );
        }
        let mut seen = Vec::new();
        let mut cursor = 0;
        loop {
            let (batch, next) = memory.scan(cursor, 3, &Filter::default());
            seen.extend(batch);
            cursor = next;
            if cursor == 0 {
                break;
            }
        }
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn cloning_rebuilds_the_indexes_and_compacts_freed_slots() {
        let mut memory = hybrid();
        add(&mut memory, "m1", "alpha", Some(vec![1.0, 0.0, 0.0]));
        add(&mut memory, "m2", "beta", Some(vec![0.0, 1.0, 0.0]));
        memory.del(b"m1", CAP);

        let copy = memory.clone();
        assert_eq!(copy.len(), 1);
        assert_eq!(copy.vector_count(), 1);
        assert_eq!(copy.next_id(), memory.next_id());
        assert_eq!(
            copy.search_text(b"beta", 10, &Filter::default(), CAP)
                .unwrap()
                .len(),
            1
        );
    }
}
