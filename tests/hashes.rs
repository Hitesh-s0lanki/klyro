//! Hash commands added beyond HSET/HGET/HDEL/HLEN/HGETALL.

mod common;

use common::{lines_before_terminator, KlyroServer};

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

/// Hash iteration order is unspecified, so compare sorted.
fn sorted(reply: &str) -> Vec<&str> {
    let mut lines = lines_before_terminator(reply, "END");
    lines.sort_unstable();
    lines
}

#[test]
fn hmset_writes_several_fields_at_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HMSET u name Alice age 30"), "OK\r\n");
    assert_eq!(client.send("HGET u name"), "VALUE Alice\r\n");
    assert_eq!(client.send("HLEN u"), "LEN 2\r\n");
    assert_eq!(
        client.send("HMSET u dangling"),
        "ERR usage: HMSET key field value [field value ...]\r\n"
    );
}

#[test]
fn hset_still_takes_a_value_with_spaces() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HSET u name Alice Smith");
    assert_eq!(client.send("HGET u name"), "VALUE Alice Smith\r\n");
}

#[test]
fn hsetnx_writes_only_a_missing_field() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HSETNX u f first"), "OK\r\n");
    assert_eq!(client.send("HSETNX u f second"), "FALSE\r\n");
    assert_eq!(client.send("HGET u f"), "VALUE first\r\n");
}

#[test]
fn hmget_reports_each_field_in_order() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2");
    let reply = client.send("HMGET u a b missing");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["VALUE 1", "VALUE 2", "NOT_FOUND"]
    );
}

#[test]
fn hmget_on_a_missing_key_reports_every_field_missing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("HMGET nope a b");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["NOT_FOUND", "NOT_FOUND"]
    );
}

#[test]
fn hexists_and_hstrlen_inspect_a_field() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HSET u name Alice");
    assert_eq!(client.send("HEXISTS u name"), "TRUE\r\n");
    assert_eq!(client.send("HEXISTS u nope"), "FALSE\r\n");
    assert_eq!(client.send("HSTRLEN u name"), "LEN 5\r\n");
    assert_eq!(client.send("HSTRLEN u nope"), "LEN 0\r\n");
}

#[test]
fn hkeys_and_hvals_split_the_hash() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2");
    assert_eq!(sorted(&client.send("HKEYS u")), vec!["a", "b"]);
    assert_eq!(sorted(&client.send("HVALS u")), vec!["1", "2"]);
    assert!(sorted(&client.send("HKEYS missing")).is_empty());
}

#[test]
fn hdel_keeps_its_single_field_reply_and_counts_multiples() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2 c 3");
    assert_eq!(client.send("HDEL u a"), "OK\r\n");
    assert_eq!(client.send("HDEL u a"), "NOT_FOUND\r\n");
    assert_eq!(client.send("HDEL u b c nope"), "DELETED 2\r\n");
    assert_eq!(client.send("EXISTS u"), "COUNT 0\r\n");
}

#[test]
fn hincrby_counts_within_a_hash() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HINCRBY u hits 1"), "VALUE 1\r\n");
    assert_eq!(client.send("HINCRBY u hits 9"), "VALUE 10\r\n");
    assert_eq!(client.send("HINCRBY u hits -4"), "VALUE 6\r\n");
    client.send("HSET u name Alice");
    assert_eq!(
        client.send("HINCRBY u name 1"),
        "ERR hash value is not an integer\r\n"
    );
}

#[test]
fn hincrbyfloat_accumulates_fractions() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HINCRBYFLOAT u score 1.5"), "VALUE 1.5\r\n");
    assert_eq!(client.send("HINCRBYFLOAT u score 2.25"), "VALUE 3.75\r\n");
    client.send("HSET u name Alice");
    assert_eq!(
        client.send("HINCRBYFLOAT u name 1"),
        "ERR hash value is not a float\r\n"
    );
}

#[test]
fn new_hash_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for cmd in [
        "HMSET s a 1",
        "HMGET s a",
        "HSETNX s a 1",
        "HEXISTS s a",
        "HKEYS s",
        "HVALS s",
        "HSTRLEN s a",
        "HINCRBY s a 1",
    ] {
        assert_eq!(client.send(cmd), WRONGTYPE, "for {cmd}");
    }
}
