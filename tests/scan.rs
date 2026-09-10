//! KEYS glob matching and SCAN's resumable cursor.

mod common;

use common::{int, KlyroServer, Value};
use std::collections::HashSet;

const SEED_KEYS: &[&str] = &[
    "user:1",
    "user:2",
    "user:10",
    "post:1",
    "post:abc",
    "session:xyz",
];

fn seeded_server() -> (KlyroServer, common::KlyroClient) {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for key in SEED_KEYS {
        client.call(&["SET", key, "v"]);
    }
    (server, client)
}

fn keys_matching(client: &mut common::KlyroClient, pattern: &str) -> HashSet<String> {
    client.call(&["KEYS", pattern]).list().into_iter().collect()
}

#[test]
fn keys_star_matches_everything() {
    let (_server, mut client) = seeded_server();
    assert_eq!(
        keys_matching(&mut client, "*"),
        SEED_KEYS.iter().map(|k| k.to_string()).collect()
    );
}

#[test]
fn keys_prefix_and_suffix_patterns() {
    let (_server, mut client) = seeded_server();
    assert_eq!(
        keys_matching(&mut client, "user:*"),
        ["user:1", "user:2", "user:10"]
            .iter()
            .map(|k| k.to_string())
            .collect()
    );
    assert_eq!(
        keys_matching(&mut client, "*:1"),
        ["user:1", "post:1"].iter().map(|k| k.to_string()).collect()
    );
}

#[test]
fn keys_question_mark_matches_one_character() {
    let (_server, mut client) = seeded_server();
    assert_eq!(
        keys_matching(&mut client, "user:?"),
        ["user:1", "user:2"].iter().map(|k| k.to_string()).collect()
    );
}

#[test]
fn keys_character_class_and_negation() {
    let (_server, mut client) = seeded_server();
    assert_eq!(
        keys_matching(&mut client, "[up]*"),
        ["user:1", "user:2", "user:10", "post:1", "post:abc"]
            .iter()
            .map(|k| k.to_string())
            .collect()
    );
    assert_eq!(
        keys_matching(&mut client, "[^up]*"),
        ["session:xyz"].iter().map(|k| k.to_string()).collect()
    );
}

#[test]
fn keys_exact_match_and_no_match() {
    let (_server, mut client) = seeded_server();
    assert_eq!(
        keys_matching(&mut client, "user:1"),
        ["user:1"].iter().map(|k| k.to_string()).collect()
    );
    assert!(keys_matching(&mut client, "nothing:*").is_empty());
}

/// One SCAN call, as (cursor, keys).
fn scan_batch(client: &mut common::KlyroClient, args: &[&str]) -> (String, Vec<String>) {
    let reply = client.call(args);
    let items = reply.items();
    assert_eq!(items.len(), 2, "SCAN replies with a cursor and a batch");
    (items[0].text(), items[1].list())
}

#[test]
fn scan_covers_everything_in_one_call_with_a_generous_count() {
    let (_server, mut client) = seeded_server();
    let (cursor, keys) = scan_batch(&mut client, &["SCAN", "0", "COUNT", "100"]);
    assert_eq!(cursor, "0");
    assert_eq!(keys.len(), SEED_KEYS.len());
}

#[test]
fn scan_with_a_small_count_needs_several_calls_but_covers_everything() {
    let (_server, mut client) = seeded_server();
    let mut seen: HashSet<String> = HashSet::new();
    let mut cursor = "0".to_string();
    let mut rounds = 0;

    loop {
        let (next, keys) = scan_batch(&mut client, &["SCAN", &cursor, "COUNT", "2"]);
        for key in keys {
            assert!(seen.insert(key.clone()), "SCAN re-emitted {key}");
        }
        cursor = next;
        rounds += 1;
        assert!(rounds < 20, "SCAN did not terminate");
        if cursor == "0" {
            break;
        }
    }

    assert!(rounds > 1, "COUNT 2 should have needed several rounds");
    assert_eq!(seen.len(), SEED_KEYS.len());
}

#[test]
fn scan_match_filters_the_batch() {
    let (_server, mut client) = seeded_server();
    let (cursor, keys) = scan_batch(
        &mut client,
        &["SCAN", "0", "MATCH", "user:*", "COUNT", "100"],
    );
    assert_eq!(cursor, "0");
    let found: HashSet<String> = keys.into_iter().collect();
    assert_eq!(
        found,
        ["user:1", "user:2", "user:10"]
            .iter()
            .map(|k| k.to_string())
            .collect()
    );
}

#[test]
fn scan_rejects_a_bad_cursor_or_option() {
    let (_server, mut client) = seeded_server();
    assert_eq!(client.send("SCAN notanumber").error(), "ERR invalid cursor");
    assert_eq!(client.send("SCAN -1").error(), "ERR invalid cursor");
    assert_eq!(client.send("SCAN 0 BOGUS x").error(), "ERR syntax error");
    assert_eq!(client.send("SCAN 0 MATCH").error(), "ERR syntax error");
    assert_eq!(client.send("SCAN 0 COUNT 0").error(), "ERR syntax error");
}

#[test]
fn scan_on_an_empty_keyspace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let (cursor, keys) = scan_batch(&mut client, &["SCAN", "0"]);
    assert_eq!(cursor, "0");
    assert!(keys.is_empty());
    assert_eq!(client.send("KEYS *"), Value::Array(vec![]));
    assert_eq!(client.send("DBSIZE"), int(0));
}
