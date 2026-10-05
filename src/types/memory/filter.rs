//! Metadata filters.
//!
//! A filter is a list of `field op value` triples, ANDed. That is
//! deliberately not a query language: it covers every filter an agent
//! actually asks for - a namespace's own sub-scoping, a memory type, a
//! recency floor, an importance floor - without a parser or a grammar.
//!
//! Filtering matters for more than correctness. An agent's namespace
//! can hold tens of thousands of memories, and a filter that runs
//! before scoring is what keeps a query from touching all of them.

use std::time::SystemTime;

use super::record::MemoryRecord;
use super::unix_millis;
use crate::util::bytes::{eq_ignore_case, parse_f64, Bytes};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Eq,
    Ne,
    Gt,
    Gte,
    Lt,
    Lte,
    /// Value is one of a comma-separated list.
    In,
    /// Value contains this substring.
    Contains,
}

impl Op {
    pub fn parse(word: &[u8]) -> Option<Op> {
        for (name, op) in [
            ("EQ", Op::Eq),
            ("NE", Op::Ne),
            ("GT", Op::Gt),
            ("GTE", Op::Gte),
            ("LT", Op::Lt),
            ("LTE", Op::Lte),
            ("IN", Op::In),
            ("CONTAINS", Op::Contains),
        ] {
            if eq_ignore_case(word, name) {
                return Some(op);
            }
        }
        None
    }
}

/// Which value a clause reads. Fields beginning with `@` name the
/// record itself; everything else names a metadata field. The prefix
/// keeps the two namespaces apart, so a metadata field called `text`
/// stays reachable.
#[derive(Clone, PartialEq, Debug)]
pub enum Field {
    Meta(Bytes),
    Id,
    Text,
    Importance,
    CreatedAt,
    UpdatedAt,
}

impl Field {
    pub fn parse(name: &[u8]) -> Option<Field> {
        let Some(reserved) = name.strip_prefix(b"@") else {
            return Some(Field::Meta(name.to_vec()));
        };
        for (word, field) in [
            ("id", Field::Id),
            ("text", Field::Text),
            ("importance", Field::Importance),
            ("created_at", Field::CreatedAt),
            ("updated_at", Field::UpdatedAt),
        ] {
            if eq_ignore_case(reserved, word) {
                return Some(field);
            }
        }
        None
    }

    /// The record's value for this field, or `None` when the record
    /// doesn't carry it. Timestamps read as unix milliseconds, which is
    /// what a client filtering on `created_after` already has.
    fn read(&self, record: &MemoryRecord) -> Option<Bytes> {
        Some(match self {
            Field::Meta(name) => record.get_meta(name)?.to_vec(),
            Field::Id => record.id.clone(),
            Field::Text => record.text.clone(),
            Field::Importance => format!("{}", record.importance).into_bytes(),
            Field::CreatedAt => unix_millis(record.created_at).to_string().into_bytes(),
            Field::UpdatedAt => unix_millis(record.updated_at).to_string().into_bytes(),
        })
    }
}

#[derive(Clone, Debug)]
pub struct Clause {
    pub field: Field,
    pub op: Op,
    pub value: Bytes,
}

/// Compares two values numerically when both parse as numbers, and as
/// raw bytes otherwise. This is what lets `@importance GTE 0.8` and
/// `type EQ preference` share one code path without the client
/// declaring a schema.
fn compare(left: &[u8], right: &[u8]) -> std::cmp::Ordering {
    match (parse_f64(left), parse_f64(right)) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal),
        _ => left.cmp(right),
    }
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

impl Clause {
    fn matches(&self, record: &MemoryRecord) -> bool {
        let Some(actual) = self.field.read(record) else {
            // A record missing the field fails every test except "is
            // not equal to", which it trivially passes.
            return self.op == Op::Ne;
        };
        let ordering = compare(&actual, &self.value);
        match self.op {
            Op::Eq => ordering.is_eq(),
            Op::Ne => !ordering.is_eq(),
            Op::Gt => ordering.is_gt(),
            Op::Gte => ordering.is_ge(),
            Op::Lt => ordering.is_lt(),
            Op::Lte => ordering.is_le(),
            Op::In => self
                .value
                .split(|b| *b == b',')
                .any(|option| compare(&actual, option).is_eq()),
            Op::Contains => contains(&actual, &self.value),
        }
    }
}

/// Every clause a query carried, ANDed. An empty filter accepts
/// everything.
#[derive(Clone, Default, Debug)]
pub struct Filter {
    clauses: Vec<Clause>,
}

impl Filter {
    pub fn push(&mut self, clause: Clause) {
        self.clauses.push(clause);
    }

    pub fn matches(&self, record: &MemoryRecord, now: SystemTime) -> bool {
        record.is_live(now) && self.clauses.iter().all(|c| c.matches(record))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn record() -> MemoryRecord {
        let mut r = MemoryRecord::new(
            b"m1".to_vec(),
            b"User prefers PostgreSQL".to_vec(),
            SystemTime::now(),
        );
        r.set_meta(b"type".to_vec(), b"preference".to_vec());
        r.set_meta(b"score".to_vec(), b"42".to_vec());
        r.importance = 0.85;
        r
    }

    fn filter(field: &[u8], op: &str, value: &[u8]) -> Filter {
        let mut f = Filter::default();
        f.push(Clause {
            field: Field::parse(field).unwrap(),
            op: Op::parse(op.as_bytes()).unwrap(),
            value: value.to_vec(),
        });
        f
    }

    fn matches(field: &[u8], op: &str, value: &[u8]) -> bool {
        filter(field, op, value).matches(&record(), SystemTime::now())
    }

    #[test]
    fn equality_on_a_metadata_field() {
        assert!(matches(b"type", "EQ", b"preference"));
        assert!(!matches(b"type", "EQ", b"fact"));
        assert!(matches(b"type", "NE", b"fact"));
    }

    #[test]
    fn numbers_compare_numerically_not_lexically() {
        // As bytes, "42" sorts after "100"; as numbers it does not.
        assert!(!matches(b"score", "GT", b"100"));
        assert!(matches(b"score", "LT", b"100"));
        assert!(matches(b"score", "GTE", b"42"));
    }

    #[test]
    fn reserved_fields_read_the_record_itself() {
        assert!(matches(b"@importance", "GTE", b"0.8"));
        assert!(!matches(b"@importance", "GT", b"0.9"));
        assert!(matches(b"@id", "EQ", b"m1"));
        assert!(matches(b"@text", "CONTAINS", b"PostgreSQL"));
        assert!(!matches(b"@text", "CONTAINS", b"MySQL"));
    }

    #[test]
    fn created_at_compares_as_unix_millis() {
        let past = (unix_millis(SystemTime::now()) - 60_000).to_string();
        assert!(matches(b"@created_at", "GTE", past.as_bytes()));
        let future = (unix_millis(SystemTime::now()) + 60_000).to_string();
        assert!(!matches(b"@created_at", "GTE", future.as_bytes()));
    }

    #[test]
    fn in_takes_a_comma_separated_list() {
        assert!(matches(b"type", "IN", b"fact,preference,event"));
        assert!(!matches(b"type", "IN", b"fact,event"));
    }

    #[test]
    fn a_missing_field_fails_everything_except_ne() {
        assert!(!matches(b"absent", "EQ", b"x"));
        assert!(!matches(b"absent", "GT", b"0"));
        assert!(matches(b"absent", "NE", b"x"));
    }

    #[test]
    fn clauses_are_anded() {
        let mut f = filter(b"type", "EQ", b"preference");
        f.push(Clause {
            field: Field::parse(b"score").unwrap(),
            op: Op::Gt,
            value: b"100".to_vec(),
        });
        assert!(!f.matches(&record(), SystemTime::now()));
    }

    #[test]
    fn an_empty_filter_accepts_any_live_record() {
        let f = Filter::default();
        assert!(f.matches(&record(), SystemTime::now()));
        let mut expired = record();
        expired.expire_at = Some(SystemTime::now() - Duration::from_secs(1));
        assert!(!f.matches(&expired, SystemTime::now()));
    }

    #[test]
    fn an_unknown_reserved_field_is_refused() {
        assert_eq!(Field::parse(b"@nonsense"), None);
        assert_eq!(Op::parse(b"LIKE"), None);
    }
}
