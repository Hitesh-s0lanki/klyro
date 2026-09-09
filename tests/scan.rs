//! KEYS's glob pattern and SCAN's resumable cursor. Ported from the old
//! test_scan.py.

mod common;

use common::KlyroServer;
use std::collections::HashSet;

const ALL_KEYS: [&str; 6] = [
    "user:1",
    "user:2",
    "user:3",
    "post:1",
    "post:2",
    "session:abc",
];

fn seeded_server() -> (KlyroServer, common::KlyroClient) {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for k in ALL_KEYS {
        client.send(&format!("SET {k} v"));
    }
    (server, client)
}

fn keys_matching(client: &mut common::KlyroClient, pattern: Option<&str>) -> HashSet<String> {
    let resp = match pattern {
        Some(p) => client.send(&format!("KEYS {p}")),
        None => client.send("KEYS"),
    };
    assert!(resp.ends_with("END\r\n"));
    common::lines_before_terminator(&resp, "END")
        .into_iter()
        .map(String::from)
        .collect()
}

#[test]
fn keys_no_pattern_matches_everything() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, None),
        ALL_KEYS.iter().map(|s| s.to_string()).collect()
    );
}

#[test]
fn keys_star_prefix() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("user:*")),
        HashSet::from([
            "user:1".to_string(),
            "user:2".to_string(),
            "user:3".to_string()
        ])
    );
}

#[test]
fn keys_question_mark_matches_one_char() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("post:?")),
        HashSet::from(["post:1".to_string(), "post:2".to_string()])
    );
}

#[test]
fn keys_star_suffix() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("*:1")),
        HashSet::from(["user:1".to_string(), "post:1".to_string()])
    );
}

#[test]
fn keys_character_class() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("[us]*")),
        HashSet::from([
            "user:1".to_string(),
            "user:2".to_string(),
            "user:3".to_string(),
            "session:abc".to_string()
        ])
    );
}

#[test]
fn keys_negated_character_class() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("[^up]*")),
        HashSet::from(["session:abc".to_string()])
    );
}

#[test]
fn keys_no_match() {
    let (_server, mut c) = seeded_server();
    assert_eq!(keys_matching(&mut c, Some("nomatch*")), HashSet::new());
}

#[test]
fn keys_exact_match_no_wildcards() {
    let (_server, mut c) = seeded_server();
    assert_eq!(
        keys_matching(&mut c, Some("user:1")),
        HashSet::from(["user:1".to_string()])
    );
}

fn scan_batch(client: &mut common::KlyroClient, cmd: &str) -> (Vec<String>, String) {
    let resp = client.send(cmd);
    let mut lines: Vec<&str> = resp.trim_end_matches("\r\n").split("\r\n").collect();
    let cursor_line = lines.pop().expect("at least a cursor line");
    assert!(cursor_line.starts_with("CURSOR "), "got {cursor_line:?}");
    let next_cursor = cursor_line.split_whitespace().nth(1).unwrap().to_string();
    (lines.into_iter().map(String::from).collect(), next_cursor)
}

#[test]
fn scan_single_call_covers_everything_with_generous_count() {
    let (_server, mut c) = seeded_server();
    let (keys, next_cursor) = scan_batch(&mut c, "SCAN 0 COUNT 100");
    let keys: HashSet<String> = keys.into_iter().collect();
    assert_eq!(keys, ALL_KEYS.iter().map(|s| s.to_string()).collect());
    assert_eq!(next_cursor, "0");
}

#[test]
fn scan_small_count_requires_multiple_calls_but_covers_everything() {
    let (_server, mut c) = seeded_server();
    let mut cursor = "0".to_string();
    let mut seen = HashSet::new();
    let mut rounds = 0;
    loop {
        rounds += 1;
        assert!(
            rounds < 20,
            "SCAN should terminate well before this many rounds"
        );
        let (keys, next_cursor) = scan_batch(&mut c, &format!("SCAN {cursor} COUNT 2"));
        for k in &keys {
            assert!(
                seen.insert(k.clone()),
                "SCAN re-emitted a key mid-iteration"
            );
        }
        cursor = next_cursor;
        if cursor == "0" {
            break;
        }
    }
    assert_eq!(seen, ALL_KEYS.iter().map(|s| s.to_string()).collect());
    assert!(
        rounds > 1,
        "COUNT 2 over 6 keys should take more than one round"
    );
}

#[test]
fn scan_match_filters_results() {
    let (_server, mut c) = seeded_server();
    let (keys, cursor) = scan_batch(&mut c, "SCAN 0 MATCH user:* COUNT 100");
    let keys: HashSet<String> = keys.into_iter().collect();
    assert_eq!(
        keys,
        HashSet::from([
            "user:1".to_string(),
            "user:2".to_string(),
            "user:3".to_string()
        ])
    );
    assert_eq!(cursor, "0");
}

#[test]
fn scan_match_and_count_together() {
    let (_server, mut c) = seeded_server();
    let (keys, _cursor) = scan_batch(&mut c, "SCAN 0 COUNT 100 MATCH post:*");
    let keys: HashSet<String> = keys.into_iter().collect();
    assert_eq!(
        keys,
        HashSet::from(["post:1".to_string(), "post:2".to_string()])
    );
}

#[test]
fn scan_bad_cursor_is_rejected() {
    let (_server, mut c) = seeded_server();
    let resp = c.send("SCAN notanumber");
    assert_eq!(
        resp,
        "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n"
    );
}

#[test]
fn scan_negative_cursor_is_rejected() {
    let (_server, mut c) = seeded_server();
    let resp = c.send("SCAN -1");
    assert_eq!(
        resp,
        "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n"
    );
}

#[test]
fn scan_unknown_option_is_rejected() {
    let (_server, mut c) = seeded_server();
    let resp = c.send("SCAN 0 BOGUS x");
    assert_eq!(
        resp,
        "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n"
    );
}

#[test]
fn scan_match_without_pattern_is_rejected() {
    let (_server, mut c) = seeded_server();
    let resp = c.send("SCAN 0 MATCH");
    assert_eq!(
        resp,
        "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n"
    );
}

#[test]
fn scan_count_zero_is_rejected() {
    let (_server, mut c) = seeded_server();
    let resp = c.send("SCAN 0 COUNT 0");
    assert_eq!(
        resp,
        "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n"
    );
}
