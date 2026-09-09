//! Redis-style glob pattern matching, used by KEYS/SCAN's optional
//! pattern: `*` matches any run of characters (including none), `?`
//! matches exactly one character, `[...]` matches one character from a
//! set (a leading `^` or `!` negates it; `a-z` ranges are supported, and
//! a literal `]` is allowed as the class's first character), and `\`
//! escapes the next pattern character to match it literally.

pub fn glob_match(pattern: &str, s: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), s.as_bytes())
}

fn glob_match_bytes(pattern: &[u8], s: &[u8]) -> bool {
    if pattern.is_empty() {
        return s.is_empty();
    }

    match pattern[0] {
        b'*' => {
            let rest = &pattern[1..];
            if rest.is_empty() {
                return true; // trailing '*' matches the rest
            }
            for i in 0..=s.len() {
                if glob_match_bytes(rest, &s[i..]) {
                    return true;
                }
            }
            false
        }

        b'?' => !s.is_empty() && glob_match_bytes(&pattern[1..], &s[1..]),

        b'[' => {
            if s.is_empty() {
                return false;
            }
            let (matched, consumed) = match_class(&pattern[1..], s[0]);
            matched && glob_match_bytes(&pattern[1 + consumed..], &s[1..])
        }

        b'\\' if pattern.len() > 1 => {
            !s.is_empty() && s[0] == pattern[1] && glob_match_bytes(&pattern[2..], &s[1..])
        }

        c => !s.is_empty() && s[0] == c && glob_match_bytes(&pattern[1..], &s[1..]),
    }
}

/// `p` points just after the opening `[`. Returns whether `c` is a member
/// of the (possibly negated) class, plus how many bytes of `p` the class
/// consumed (including the closing `]`, if present).
fn match_class(p: &[u8], c: u8) -> (bool, usize) {
    let mut i = 0;
    let mut negate = false;
    if i < p.len() && (p[i] == b'^' || p[i] == b'!') {
        negate = true;
        i += 1;
    }

    let mut found = false;
    let mut first = true; // a ']' right after '[' (or '[^') is a literal member
    while i < p.len() && (p[i] != b']' || first) {
        first = false;
        if p[i] == b'\\' && i + 1 < p.len() {
            if p[i + 1] == c {
                found = true;
            }
            i += 2;
            continue;
        }
        if i + 2 < p.len() && p[i + 1] == b'-' && p[i + 2] != b']' {
            if c >= p[i] && c <= p[i + 2] {
                found = true;
            }
            i += 3;
            continue;
        }
        if p[i] == c {
            found = true;
        }
        i += 1;
    }
    if i < p.len() && p[i] == b']' {
        i += 1;
    }

    (if negate { !found } else { found }, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn star_matches_any_run() {
        assert!(glob_match("user:*", "user:1"));
        assert!(glob_match("*:1", "user:1"));
        assert!(glob_match("*", ""));
    }

    #[test]
    fn question_mark_matches_one_char() {
        assert!(glob_match("post:?", "post:1"));
        assert!(!glob_match("post:?", "post:12"));
    }

    #[test]
    fn character_class_and_negation() {
        assert!(glob_match("[us]*", "user:1"));
        assert!(glob_match("[us]*", "session:abc"));
        assert!(!glob_match("[^up]*", "user:1"));
        assert!(glob_match("[^up]*", "session:abc"));
    }

    #[test]
    fn exact_match_no_wildcards() {
        assert!(glob_match("user:1", "user:1"));
        assert!(!glob_match("user:1", "user:2"));
    }
}
