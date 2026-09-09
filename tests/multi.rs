//! LPUSH/RPUSH/SADD/ZADD taking multiple values/pairs in one call.
//! Ported from the old test_multi.py.

mod common;

use common::KlyroServer;
use std::collections::HashSet;

fn server_and_client() -> (KlyroServer, common::KlyroClient) {
    let server = KlyroServer::new();
    let client = server.connect();
    (server, client)
}

#[test]
fn lpush_multi_pushes_each_to_head_in_turn() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("LPUSH k a b c"), "LEN 3\r\n");
    // Redis semantics: LPUSH k a b c -> list ends up [c, b, a]
    assert_eq!(c.send("LRANGE k 0 -1"), "c\r\nb\r\na\r\nEND\r\n");
}

#[test]
fn rpush_multi_pushes_in_order() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("RPUSH k a b c"), "LEN 3\r\n");
    assert_eq!(c.send("LRANGE k 0 -1"), "a\r\nb\r\nc\r\nEND\r\n");
}

#[test]
fn push_requires_at_least_one_value() {
    let (_server, mut c) = server_and_client();
    assert_eq!(
        c.send("LPUSH k"),
        "ERR usage: LPUSH key value [value ...]\r\n"
    );
}

#[test]
fn sadd_multi_counts_only_new_members() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("SADD k x y z"), "ADDED 3\r\n");
    assert_eq!(c.send("SADD k x y w"), "ADDED 1\r\n");
    let resp = c.send("SMEMBERS k");
    let members: HashSet<&str> = common::lines_before_terminator(&resp, "END")
        .into_iter()
        .collect();
    assert_eq!(members, HashSet::from(["x", "y", "z", "w"]));
}

#[test]
fn sadd_requires_at_least_one_member() {
    let (_server, mut c) = server_and_client();
    assert_eq!(
        c.send("SADD k"),
        "ERR usage: SADD key member [member ...]\r\n"
    );
}

#[test]
fn zadd_multi_pairs() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("ZADD k 100 alice 50 bob 75 carol"), "ADDED 3\r\n");
    assert_eq!(
        c.send("ZRANGE k 0 -1"),
        "bob 50\r\ncarol 75\r\nalice 100\r\nEND\r\n"
    );
}

#[test]
fn zadd_multi_pairs_counts_only_new_members() {
    let (_server, mut c) = server_and_client();
    c.send("ZADD k 100 alice");
    assert_eq!(c.send("ZADD k 10 alice 200 dave"), "ADDED 1\r\n");
}

#[test]
fn zadd_dangling_score_is_rejected() {
    let (_server, mut c) = server_and_client();
    assert_eq!(
        c.send("ZADD k 5"),
        "ERR usage: ZADD key score member [score member ...]\r\n"
    );
}

#[test]
fn zadd_non_numeric_score_is_rejected() {
    let (_server, mut c) = server_and_client();
    assert_eq!(
        c.send("ZADD k notanumber alice"),
        "ERR usage: ZADD key score member [score member ...]\r\n"
    );
}

#[test]
fn zadd_too_many_pairs_is_rejected() {
    let (_server, mut c) = server_and_client();
    let pairs: Vec<String> = (0..130).map(|i| format!("{i} m{i}")).collect();
    let cmd = format!("ZADD k {}", pairs.join(" "));
    assert_eq!(c.send(&cmd), "ERR too many score/member pairs\r\n");
}

#[test]
fn single_value_set_and_hset_still_allow_spaces() {
    let (_server, mut c) = server_and_client();
    c.send("SET k hello world");
    assert_eq!(c.send("GET k"), "VALUE hello world\r\n");
    c.send("HSET k_h name Alice Smith");
    assert_eq!(c.send("HGET k_h name"), "VALUE Alice Smith\r\n");
}
