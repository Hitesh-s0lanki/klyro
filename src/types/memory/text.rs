//! Keyword retrieval: a tokenizer, an inverted index, and BM25.
//!
//! BM25 rather than plain TF-IDF because memories vary wildly in
//! length - a five-word stated preference sits next to a paragraph of
//! conversation context - and BM25's length normalization is what stops
//! the paragraph from winning on term count alone.
//!
//! There is deliberately no stemmer. The Search structure exists for
//! exact terms: identifiers, product names, error strings, technology
//! names. Stemming "PostgreSQL" or "mem_001" costs precision on
//! precisely the queries this index is for.

use std::collections::HashMap;

use crate::util::bytes::Bytes;

/// Term-frequency saturation. 1.2 is the usual default; past it, extra
/// repetitions of a term add almost nothing.
const K1: f32 = 1.2;
/// How much document length is normalized away. 0.75 is the usual
/// default: mostly normalized, not entirely.
const B: f32 = 0.75;

/// Words carrying no retrieval signal. Short on purpose - an
/// aggressive list would swallow terms that matter in a technical
/// memory ("can", "will", "should" all appear in real preferences).
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "for", "from", "had", "has", "have",
    "he", "her", "his", "in", "is", "it", "its", "of", "on", "or", "she", "that", "the", "their",
    "them", "then", "there", "these", "they", "this", "to", "was", "were", "with",
];

fn is_stopword(term: &[u8]) -> bool {
    // Only ASCII words can be stopwords, and the list is small enough
    // that a linear scan beats building a set per index.
    STOPWORDS.iter().any(|w| w.as_bytes() == term)
}

/// Whether this byte belongs inside a word. Bytes at or above 0x80 are
/// UTF-8 continuation or lead bytes, and are kept so a non-English word
/// survives tokenization as one term rather than being shredded.
/// Underscore is a word byte because identifiers - `mem_001`,
/// `user_id`, `MAX_RETRIES` - are exactly what this index is for, and
/// unlike `.` or `-` it never doubles as sentence punctuation, so no
/// trimming is needed to keep it.
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

/// Lowercases, splits on anything that isn't a word byte, and drops
/// single-byte and stopword terms. Stops after `max_terms`, so one
/// enormous document can't blow up the index.
pub fn tokenize(text: &[u8], max_terms: usize) -> Vec<Bytes> {
    let mut terms = Vec::new();
    let mut current = Vec::new();
    for &byte in text {
        if is_word_byte(byte) {
            current.push(byte.to_ascii_lowercase());
            continue;
        }
        if push_term(&mut terms, &mut current, max_terms) {
            return terms;
        }
    }
    push_term(&mut terms, &mut current, max_terms);
    terms
}

/// Moves `current` into `terms` if it survives filtering. Returns
/// whether the cap has been reached.
fn push_term(terms: &mut Vec<Bytes>, current: &mut Vec<u8>, max_terms: usize) -> bool {
    if current.len() > 1 && !is_stopword(current) {
        terms.push(std::mem::take(current));
    } else {
        current.clear();
    }
    terms.len() >= max_terms
}

/// Term counts for one document, so a term repeated in a document is
/// one posting with a frequency rather than several postings.
fn term_frequencies(terms: Vec<Bytes>) -> Vec<(Bytes, u32)> {
    let mut counts: HashMap<Bytes, u32> = HashMap::new();
    for term in terms {
        *counts.entry(term).or_insert(0) += 1;
    }
    counts.into_iter().collect()
}

#[derive(Default)]
pub struct TextIndex {
    /// term -> [(record id, times the term appears in that record)]
    postings: HashMap<Bytes, Vec<(Bytes, u32)>>,
    doc_len: HashMap<Bytes, u32>,
    total_len: u64,
}

impl TextIndex {
    pub fn new() -> TextIndex {
        TextIndex::default()
    }

    pub fn doc_count(&self) -> usize {
        self.doc_len.len()
    }

    pub fn term_count(&self) -> usize {
        self.postings.len()
    }

    pub fn avg_doc_len(&self) -> f32 {
        if self.doc_len.is_empty() {
            return 0.0;
        }
        self.total_len as f32 / self.doc_len.len() as f32
    }

    pub fn heap_bytes(&self) -> usize {
        let postings: usize = self
            .postings
            .iter()
            .map(|(term, list)| {
                term.len()
                    + list.capacity() * std::mem::size_of::<(Bytes, u32)>()
                    + list.iter().map(|(id, _)| id.len()).sum::<usize>()
            })
            .sum();
        let lengths: usize = self.doc_len.keys().map(|id| id.len() + 4).sum();
        postings + lengths
    }

    /// Indexes `id` under `text`, first un-indexing `previous` - the
    /// text it was last indexed under, or `None` if it is new.
    ///
    /// The previous text is a parameter rather than something the index
    /// remembers because the caller (a `Memory`) already holds the
    /// record. Keeping a term list per document here instead would
    /// roughly double the index's memory to save a tokenizer pass.
    pub fn index(&mut self, id: &[u8], previous: Option<&[u8]>, text: &[u8], max_terms: usize) {
        if let Some(previous) = previous {
            self.remove(id, previous, max_terms);
        }
        let frequencies = term_frequencies(tokenize(text, max_terms));
        let length: u32 = frequencies.iter().map(|(_, tf)| tf).sum();
        for (term, tf) in frequencies {
            self.postings
                .entry(term)
                .or_default()
                .push((id.to_vec(), tf));
        }
        // A document with no indexable terms is still a document: it
        // has to be counted, or IDF would divide by the wrong
        // collection size.
        self.doc_len.insert(id.to_vec(), length);
        self.total_len += length as u64;
    }

    /// Un-indexes `id`, which was indexed under `text`. Touches only
    /// the posting lists for that document's own terms, so the cost is
    /// proportional to the document rather than to the index.
    pub fn remove(&mut self, id: &[u8], text: &[u8], max_terms: usize) {
        let Some(length) = self.doc_len.remove(id) else {
            return;
        };
        self.total_len -= length as u64;
        for (term, _) in term_frequencies(tokenize(text, max_terms)) {
            let Some(list) = self.postings.get_mut(&term) else {
                continue;
            };
            list.retain(|(posting_id, _)| posting_id != id);
            if list.is_empty() {
                self.postings.remove(&term);
            }
        }
    }

    /// Inverse document frequency, in the form BM25 uses. The `1 +`
    /// keeps it non-negative for a term that appears in most documents,
    /// which the textbook formula does not.
    fn idf(&self, term: &[u8]) -> f32 {
        let n = self.doc_count() as f32;
        let df = self.postings.get(term).map_or(0, |list| list.len()) as f32;
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    /// BM25 over the union of the query's terms, best first.
    ///
    /// `keep` decides whether a candidate is eligible, and is applied
    /// *before* truncation so a metadata filter can't empty out a
    /// result set that had matches further down. `limit` caps how many
    /// candidates come back, not how many are scored.
    pub fn search<F>(&self, query_terms: &[Bytes], limit: usize, keep: F) -> Vec<(Bytes, f32)>
    where
        F: Fn(&[u8]) -> bool,
    {
        let avg = self.avg_doc_len();
        if avg == 0.0 {
            return Vec::new();
        }
        let mut scores: HashMap<&[u8], f32> = HashMap::new();
        for term in query_terms {
            let Some(list) = self.postings.get(term) else {
                continue;
            };
            let idf = self.idf(term);
            for (id, tf) in list {
                if !keep(id) {
                    continue;
                }
                let length = *self.doc_len.get(id).unwrap_or(&0) as f32;
                let tf = *tf as f32;
                let saturated = tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * length / avg));
                *scores.entry(id.as_slice()).or_insert(0.0) += idf * saturated;
            }
        }

        let mut hits: Vec<(Bytes, f32)> = scores
            .into_iter()
            .map(|(id, score)| (id.to_vec(), score))
            .collect();
        // Ties break on id so a result page is stable across calls.
        hits.sort_by(|a, b| {
            b.1.partial_cmp(&a.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });
        hits.truncate(limit);
        hits
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAP: usize = 1024;

    fn terms(text: &str) -> Vec<String> {
        tokenize(text.as_bytes(), CAP)
            .into_iter()
            .map(|t| String::from_utf8(t).unwrap())
            .collect()
    }

    #[test]
    fn tokenizer_lowercases_and_splits_on_punctuation() {
        assert_eq!(
            terms("User prefers PostgreSQL for backend projects."),
            vec!["user", "prefers", "postgresql", "backend", "projects"]
        );
    }

    #[test]
    fn tokenizer_keeps_identifiers_intact() {
        assert_eq!(
            terms("error code E42 in mem_001"),
            vec!["error", "code", "e42", "mem_001"]
        );
    }

    #[test]
    fn tokenizer_drops_stopwords_and_single_characters() {
        assert_eq!(terms("it is a b cd"), vec!["cd"]);
    }

    #[test]
    fn tokenizer_keeps_non_ascii_words_whole() {
        assert_eq!(terms("café münchen"), vec!["café", "münchen"]);
    }

    #[test]
    fn tokenizer_honours_the_term_cap() {
        assert_eq!(tokenize(b"aa bb cc dd", 2).len(), 2);
    }

    fn indexed() -> TextIndex {
        let mut index = TextIndex::new();
        index.index(
            b"m1",
            None,
            b"User prefers PostgreSQL for backend projects.",
            CAP,
        );
        index.index(
            b"m2",
            None,
            b"User is currently building a database administration tool.",
            CAP,
        );
        index.index(b"m3", None, b"User likes modern developer tools.", CAP);
        index.index(b"m4", None, b"User previously worked with MySQL.", CAP);
        index.index(
            b"m5",
            None,
            b"User is building Basora, a PostgreSQL developer application.",
            CAP,
        );
        index
    }

    #[test]
    fn search_finds_only_documents_containing_the_term() {
        let index = indexed();
        let hits = index.search(&[b"postgresql".to_vec()], 10, |_| true);
        let ids: Vec<&[u8]> = hits.iter().map(|(id, _)| id.as_slice()).collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&b"m1".as_slice()));
        assert!(ids.contains(&b"m5".as_slice()));
    }

    #[test]
    fn a_term_in_every_document_scores_near_zero() {
        let index = indexed();
        let hits = index.search(&[b"user".to_vec()], 10, |_| true);
        assert_eq!(hits.len(), 5);
        for (_, score) in hits {
            assert!(score < 0.2, "a term in every document should barely score");
        }
    }

    #[test]
    fn shorter_documents_outrank_longer_ones_on_the_same_term() {
        let mut index = TextIndex::new();
        index.index(b"short", None, b"PostgreSQL", CAP);
        index.index(
            b"long",
            None,
            b"PostgreSQL is one of many things discussed at length across this rather wordy memory record",
            CAP,
        );
        index.index(b"other", None, b"unrelated content entirely", CAP);
        let hits = index.search(&[b"postgresql".to_vec()], 10, |_| true);
        assert_eq!(hits[0].0, b"short".to_vec());
    }

    #[test]
    fn the_filter_runs_before_truncation() {
        let index = indexed();
        // Ask for one hit, but exclude the one that would have won.
        let unfiltered = index.search(&[b"postgresql".to_vec()], 1, |_| true);
        let excluded = unfiltered[0].0.clone();
        let hits = index.search(&[b"postgresql".to_vec()], 1, |id| id != excluded);
        assert_eq!(
            hits.len(),
            1,
            "a filter must not empty a page that had matches"
        );
        assert_ne!(hits[0].0, excluded);
    }

    #[test]
    fn removing_a_document_removes_its_postings() {
        let mut index = indexed();
        index.remove(b"m1", b"User prefers PostgreSQL for backend projects.", CAP);
        assert_eq!(index.doc_count(), 4);
        let hits = index.search(&[b"postgresql".to_vec()], 10, |_| true);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, b"m5".to_vec());
        assert!(index
            .search(&[b"prefers".to_vec()], 10, |_| true)
            .is_empty());
    }

    #[test]
    fn reindexing_replaces_rather_than_duplicates() {
        let mut index = TextIndex::new();
        index.index(b"m1", None, b"alpha beta", CAP);
        index.index(b"m1", Some(b"alpha beta"), b"gamma delta", CAP);
        assert_eq!(index.doc_count(), 1);
        assert!(index.search(&[b"alpha".to_vec()], 10, |_| true).is_empty());
        assert_eq!(index.search(&[b"gamma".to_vec()], 10, |_| true).len(), 1);
    }

    #[test]
    fn removal_leaves_no_empty_posting_lists_behind() {
        let mut index = TextIndex::new();
        index.index(b"m1", None, b"solitary term", CAP);
        assert_eq!(index.term_count(), 2);
        index.remove(b"m1", b"solitary term", CAP);
        assert_eq!(index.term_count(), 0);
    }

    #[test]
    fn a_document_with_no_indexable_terms_still_counts() {
        let mut index = TextIndex::new();
        index.index(b"m1", None, b"a b it is", CAP);
        assert_eq!(index.doc_count(), 1);
        assert_eq!(index.avg_doc_len(), 0.0);
    }

    #[test]
    fn searching_an_empty_index_returns_nothing() {
        let index = TextIndex::new();
        assert!(index
            .search(&[b"anything".to_vec()], 10, |_| true)
            .is_empty());
    }
}
