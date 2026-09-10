//! Byte-string helpers.
//!
//! Keys and values are `Vec<u8>`, not `String`: RESP length-prefixes
//! every argument, so a value may hold spaces, newlines, NUL bytes, or
//! anything else a client cares to send. Nothing in the keyspace may
//! assume valid UTF-8, which is why the parsing and formatting helpers
//! commands need live here rather than being `str` methods.

/// A key, a value, a field, a member - every payload in the keyspace.
pub type Bytes = Vec<u8>;

/// Renders bytes for a human-facing message, replacing anything that
/// isn't valid UTF-8. Never use this to build a reply payload.
pub fn to_display(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// An ASCII-uppercased copy, for matching command and option names.
pub fn to_upper(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| b.to_ascii_uppercase() as char)
        .collect()
}

/// Whether `bytes` equals `word` ignoring ASCII case - the test every
/// option keyword needs.
pub fn eq_ignore_case(bytes: &[u8], word: &str) -> bool {
    bytes.len() == word.len() && bytes.eq_ignore_ascii_case(word.as_bytes())
}

/// Parses a 64-bit integer. Rejects leading/trailing space, unlike
/// `str::parse`, because Redis does: `INCR` on `" 1"` is an error.
pub fn parse_i64(bytes: &[u8]) -> Option<i64> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || text.trim() != text {
        return None;
    }
    text.parse().ok()
}

/// Parses a float, accepting `inf`, `-inf`, and `+inf` as Redis does.
/// Rejects NaN, which has no ordering and so can't be a score.
pub fn parse_f64(bytes: &[u8]) -> Option<f64> {
    let text = std::str::from_utf8(bytes).ok()?;
    if text.is_empty() || text.trim() != text {
        return None;
    }
    let value: f64 = text.parse().ok()?;
    if value.is_nan() {
        None
    } else {
        Some(value)
    }
}

/// Formats a score the way Redis does: the shortest decimal text that
/// parses back to exactly this value, with no trailing `.0` on an
/// integral one. Rust's `Display` for `f64` is already that shortest
/// round-trip form, which is what Redis switched to as well.
pub fn format_f64(value: f64) -> Bytes {
    if value.is_infinite() {
        return if value > 0.0 {
            b"inf".to_vec()
        } else {
            b"-inf".to_vec()
        };
    }
    format!("{}", value).into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_and_case_insensitive_compare() {
        assert_eq!(to_upper(b"get"), "GET");
        assert!(eq_ignore_case(b"nx", "NX"));
        assert!(!eq_ignore_case(b"nxx", "NX"));
    }

    #[test]
    fn integers_reject_surrounding_space() {
        assert_eq!(parse_i64(b"42"), Some(42));
        assert_eq!(parse_i64(b"-42"), Some(-42));
        assert_eq!(parse_i64(b" 42"), None);
        assert_eq!(parse_i64(b"42 "), None);
        assert_eq!(parse_i64(b""), None);
        assert_eq!(parse_i64(b"4.2"), None);
    }

    #[test]
    fn floats_accept_infinity_but_not_nan() {
        assert_eq!(parse_f64(b"1.5"), Some(1.5));
        assert_eq!(parse_f64(b"-inf"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_f64(b"+inf"), Some(f64::INFINITY));
        assert_eq!(parse_f64(b"nan"), None);
        assert_eq!(parse_f64(b"abc"), None);
    }

    #[test]
    fn scores_print_the_way_redis_prints_them() {
        assert_eq!(format_f64(100.0), b"100".to_vec());
        assert_eq!(format_f64(-5.0), b"-5".to_vec());
        assert_eq!(format_f64(2.75), b"2.75".to_vec());
        assert_eq!(format_f64(0.1), b"0.1".to_vec());
        assert_eq!(format_f64(f64::INFINITY), b"inf".to_vec());
    }

    #[test]
    fn score_formatting_round_trips() {
        for value in [
            1.0 / 3.0,
            1e-9,
            1.7976931348623157e308,
            -0.30000000000000004,
        ] {
            let text = String::from_utf8(format_f64(value)).unwrap();
            assert_eq!(text.parse::<f64>().unwrap(), value, "for {value}");
        }
    }
}
