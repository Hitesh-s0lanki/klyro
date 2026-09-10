//! Sorted set commands.

mod common;

use common::{bulk, int, nil, KlyroServer, Value, WRONGTYPE};

fn board(server: &KlyroServer) -> common::KlyroClient {
    let mut client = server.connect();
    client.send("ZADD board 50 bob 75 carol 100 alice");
    client
}

#[test]
fn zadd_repositions_without_counting() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("ZADD z 1 a"), int(1));
    assert_eq!(client.send("ZADD z 9 a"), int(0));
    assert_eq!(client.send("ZSCORE z a"), bulk("9"));
    assert_eq!(client.send("ZCARD z"), int(1));
}

#[test]
fn zadd_rejects_a_dangling_pair_and_a_bad_score() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("ZADD z 1 a 2").error(), "ERR syntax error");
    assert_eq!(
        client.send("ZADD z notascore a").error(),
        "ERR value is not a valid float"
    );
    // A bad score anywhere leaves the whole call unapplied.
    assert_eq!(
        client.send("ZADD z 1 a bad b").error(),
        "ERR value is not a valid float"
    );
    assert_eq!(client.send("EXISTS z"), int(0));
}

#[test]
fn zrange_is_ascending_and_zrevrange_descending() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(
        client.send("ZRANGE board 0 -1").list(),
        vec!["bob", "carol", "alice"]
    );
    assert_eq!(
        client.send("ZREVRANGE board 0 -1").list(),
        vec!["alice", "carol", "bob"]
    );
    assert_eq!(
        client.send("ZREVRANGE board 0 1").list(),
        vec!["alice", "carol"]
    );
}

#[test]
fn withscores_interleaves_members_and_scores() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(
        client.send("ZRANGE board 0 -1 WITHSCORES").list(),
        vec!["bob", "50", "carol", "75", "alice", "100"]
    );
    assert_eq!(
        client.send("ZRANGE board 0 -1 BOGUS").error(),
        "ERR syntax error"
    );
}

#[test]
fn zscore_and_zmscore() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZSCORE board carol"), bulk("75"));
    assert_eq!(client.send("ZSCORE board nobody"), nil());
    assert_eq!(
        client.send("ZMSCORE board alice nobody bob"),
        Value::Array(vec![bulk("100"), nil(), bulk("50")])
    );
}

#[test]
fn zrank_and_zrevrank_count_from_each_end() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZRANK board bob"), int(0));
    assert_eq!(client.send("ZRANK board alice"), int(2));
    assert_eq!(client.send("ZREVRANK board alice"), int(0));
    assert_eq!(client.send("ZRANK board nobody"), nil());
    assert_eq!(client.send("ZRANK missing bob"), nil());
}

#[test]
fn zincrby_adjusts_a_score_and_reorders() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZINCRBY board 60 bob"), bulk("110"));
    assert_eq!(client.send("ZREVRANK board bob"), int(0));
    // A missing member starts from zero.
    assert_eq!(client.send("ZINCRBY board 5 newcomer"), bulk("5"));
    assert_eq!(client.send("ZINCRBY board -5 newcomer"), bulk("0"));
}

#[test]
fn zrangebyscore_selects_a_score_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(
        client.send("ZRANGEBYSCORE board 60 100").list(),
        vec!["carol", "alice"]
    );
    assert_eq!(
        client.send("ZRANGEBYSCORE board -inf +inf").items().len(),
        3
    );
    assert_eq!(
        client.send("ZRANGEBYSCORE board 60 100 WITHSCORES").list(),
        vec!["carol", "75", "alice", "100"]
    );
}

#[test]
fn zrangebyscore_honours_exclusive_bounds() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(
        client.send("ZRANGEBYSCORE board (75 +inf").list(),
        vec!["alice"]
    );
    assert_eq!(
        client.send("ZRANGEBYSCORE board 75 (100").list(),
        vec!["carol"]
    );
}

#[test]
fn zrevrangebyscore_takes_its_bounds_high_first() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(
        client.send("ZREVRANGEBYSCORE board 100 60").list(),
        vec!["alice", "carol"]
    );
}

#[test]
fn zcount_counts_the_same_window() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZCOUNT board -inf +inf"), int(3));
    assert_eq!(client.send("ZCOUNT board 50 75"), int(2));
    assert_eq!(client.send("ZCOUNT board (50 75"), int(1));
    assert_eq!(client.send("ZCOUNT missing 0 100"), int(0));
    assert_eq!(
        client.send("ZCOUNT board low high").error(),
        "ERR min or max is not a float"
    );
}

#[test]
fn zremrange_by_rank_and_by_score() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZREMRANGEBYRANK board 0 1"), int(2));
    assert_eq!(client.send("ZRANGE board 0 -1").list(), vec!["alice"]);

    let mut client = board(&server);
    client.send("DEL board");
    client.send("ZADD board 50 bob 75 carol 100 alice");
    assert_eq!(client.send("ZREMRANGEBYSCORE board 60 90"), int(1));
    assert_eq!(client.send("ZSCORE board carol"), nil());
    assert_eq!(client.send("ZCARD board"), int(2));
}

#[test]
fn removing_every_member_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    client.send("ZREMRANGEBYSCORE board -inf +inf");
    assert_eq!(client.send("EXISTS board"), int(0));
}

#[test]
fn zrem_counts_what_it_removed() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    assert_eq!(client.send("ZREM board bob"), int(1));
    assert_eq!(client.send("ZREM board bob"), int(0));
    assert_eq!(client.send("ZREM board carol alice nobody"), int(2));
    assert_eq!(client.send("EXISTS board"), int(0));
}

#[test]
fn zpopmin_and_zpopmax_take_from_each_end() {
    let server = KlyroServer::new();
    let mut client = board(&server);
    // Without a count the member and its score come back side by side.
    assert_eq!(client.send("ZPOPMIN board").list(), vec!["bob", "50"]);
    assert_eq!(
        client.send("ZPOPMAX board 2").list(),
        vec!["alice", "100", "carol", "75"]
    );
    assert_eq!(client.send("EXISTS board"), int(0));
    assert_eq!(client.send("ZPOPMIN missing"), Value::Array(vec![]));
}

#[test]
fn scores_print_without_a_trailing_decimal_point() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("ZADD z 1 whole 2.5 fractional -3 negative");
    assert_eq!(client.send("ZSCORE z whole"), bulk("1"));
    assert_eq!(client.send("ZSCORE z fractional"), bulk("2.5"));
    assert_eq!(client.send("ZSCORE z negative"), bulk("-3"));
}

#[test]
fn zset_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for command in [
        "ZADD s 1 m",
        "ZSCORE s m",
        "ZMSCORE s m",
        "ZREM s m",
        "ZCARD s",
        "ZRANK s m",
        "ZREVRANK s m",
        "ZRANGE s 0 -1",
        "ZREVRANGE s 0 -1",
        "ZRANGEBYSCORE s 0 1",
        "ZCOUNT s 0 1",
        "ZINCRBY s 1 m",
        "ZPOPMIN s",
        "ZREMRANGEBYRANK s 0 1",
        "ZREMRANGEBYSCORE s 0 1",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}
