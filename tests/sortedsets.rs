//! Sorted set commands added beyond ZADD/ZSCORE/ZREM/ZCARD/ZRANGE:
//! ranks, score-range queries, incremental scoring, and the pops.

mod common;

use common::{lines_before_terminator, KlyroServer};

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

fn board(server: &KlyroServer) -> common::KlyroClient {
    let mut client = server.connect();
    client.send("ZADD board 50 bob 75 carol 100 alice");
    client
}

#[test]
fn zrevrange_reads_from_the_top() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZREVRANGE board 0 -1");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["alice 100", "carol 75", "bob 50"]
    );
    let reply = client.send("ZREVRANGE board 0 1");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["alice 100", "carol 75"]
    );
}

#[test]
fn zrank_and_zrevrank_count_from_each_end() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZRANK board bob"), "RANK 0\r\n");
    assert_eq!(client.send("ZRANK board alice"), "RANK 2\r\n");
    assert_eq!(client.send("ZREVRANK board alice"), "RANK 0\r\n");
    assert_eq!(client.send("ZREVRANK board bob"), "RANK 2\r\n");
    assert_eq!(client.send("ZRANK board nobody"), "NOT_FOUND\r\n");
    assert_eq!(client.send("ZRANK missing bob"), "NOT_FOUND\r\n");
}

#[test]
fn zincrby_adjusts_a_score_and_reorders() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZINCRBY board 60 bob"), "VALUE 110\r\n");
    assert_eq!(client.send("ZREVRANK board bob"), "RANK 0\r\n");
    // A missing member starts from zero.
    assert_eq!(client.send("ZINCRBY board 5 newcomer"), "VALUE 5\r\n");
    assert_eq!(client.send("ZINCRBY board -5 newcomer"), "VALUE 0\r\n");
}

#[test]
fn zmscore_reports_each_member() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZMSCORE board alice nobody bob");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["VALUE 100", "NOT_FOUND", "VALUE 50"]
    );
}

#[test]
fn zrangebyscore_selects_a_score_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZRANGEBYSCORE board 60 100");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["carol 75", "alice 100"]
    );
    let reply = client.send("ZRANGEBYSCORE board -inf +inf");
    assert_eq!(lines_before_terminator(&reply, "END").len(), 3);
}

#[test]
fn zrangebyscore_honors_exclusive_bounds() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZRANGEBYSCORE board (75 +inf");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["alice 100"]);
    let reply = client.send("ZRANGEBYSCORE board 75 (100");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["carol 75"]);
}

#[test]
fn zrevrangebyscore_takes_its_bounds_high_first() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZREVRANGEBYSCORE board 100 60");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["alice 100", "carol 75"]
    );
}

#[test]
fn zcount_counts_the_same_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZCOUNT board -inf +inf"), "COUNT 3\r\n");
    assert_eq!(client.send("ZCOUNT board 50 75"), "COUNT 2\r\n");
    assert_eq!(client.send("ZCOUNT board (50 75"), "COUNT 1\r\n");
    assert_eq!(client.send("ZCOUNT missing 0 100"), "COUNT 0\r\n");
}

#[test]
fn score_range_commands_reject_a_non_numeric_bound() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert!(client
        .send("ZCOUNT board low high")
        .starts_with("ERR usage:"));
}

#[test]
fn zremrangebyrank_drops_a_rank_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZREMRANGEBYRANK board 0 1"), "REMOVED 2\r\n");
    let reply = client.send("ZRANGE board 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["alice 100"]);
}

#[test]
fn zremrangebyscore_drops_a_score_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZREMRANGEBYSCORE board 60 90"), "REMOVED 1\r\n");
    assert_eq!(client.send("ZSCORE board carol"), "NOT_FOUND\r\n");
    assert_eq!(client.send("ZCARD board"), "LEN 2\r\n");
}

#[test]
fn removing_every_member_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    client.send("ZREMRANGEBYSCORE board -inf +inf");
    assert_eq!(client.send("EXISTS board"), "COUNT 0\r\n");
}

#[test]
fn zpopmin_and_zpopmax_take_from_each_end() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    let reply = client.send("ZPOPMIN board");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["bob 50"]);
    let reply = client.send("ZPOPMAX board 2");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["alice 100", "carol 75"]
    );
    assert_eq!(client.send("EXISTS board"), "COUNT 0\r\n");
}

#[test]
fn popping_a_missing_key_is_an_empty_reply() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("ZPOPMIN nope");
    assert!(lines_before_terminator(&reply, "END").is_empty());
}

#[test]
fn zrem_keeps_its_single_member_reply_and_counts_multiples() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZREM board bob"), "OK\r\n");
    assert_eq!(client.send("ZREM board bob"), "NOT_FOUND\r\n");
    assert_eq!(
        client.send("ZREM board carol alice nobody"),
        "DELETED 2\r\n"
    );
    assert_eq!(client.send("EXISTS board"), "COUNT 0\r\n");
}

#[test]
fn new_zset_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for cmd in [
        "ZRANK s m",
        "ZREVRANK s m",
        "ZREVRANGE s 0 -1",
        "ZRANGEBYSCORE s 0 1",
        "ZCOUNT s 0 1",
        "ZINCRBY s 1 m",
        "ZMSCORE s m",
        "ZPOPMIN s",
        "ZREMRANGEBYRANK s 0 1",
        "ZREMRANGEBYSCORE s 0 1",
    ] {
        assert_eq!(client.send(cmd), WRONGTYPE, "for {cmd}");
    }
}
