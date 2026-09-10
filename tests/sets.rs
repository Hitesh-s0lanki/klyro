//! Set commands added beyond SADD/SREM/SISMEMBER/SCARD/SMEMBERS: the
//! algebra, the random-member picks, and SMOVE.

mod common;

use common::{lines_before_terminator, KlyroServer};

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

/// Set iteration order is unspecified, so compare sorted.
fn sorted(reply: &str) -> Vec<&str> {
    let mut lines = lines_before_terminator(reply, "END");
    lines.sort_unstable();
    lines
}

fn two_sets(server: &KlyroServer) -> common::KlyroClient {
    let mut client = server.connect();
    client.send("SADD s1 a b c d");
    client.send("SADD s2 c d e");
    client
}

#[test]
fn sinter_sunion_and_sdiff() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(sorted(&client.send("SINTER s1 s2")), vec!["c", "d"]);
    assert_eq!(
        sorted(&client.send("SUNION s1 s2")),
        vec!["a", "b", "c", "d", "e"]
    );
    assert_eq!(sorted(&client.send("SDIFF s1 s2")), vec!["a", "b"]);
    // The difference is taken in the order given, so it is not symmetric.
    assert_eq!(sorted(&client.send("SDIFF s2 s1")), vec!["e"]);
}

#[test]
fn algebra_treats_a_missing_key_as_empty() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert!(sorted(&client.send("SINTER s1 nope")).is_empty());
    assert_eq!(
        sorted(&client.send("SUNION s1 nope")),
        vec!["a", "b", "c", "d"]
    );
    assert_eq!(
        sorted(&client.send("SDIFF s1 nope")),
        vec!["a", "b", "c", "d"]
    );
}

#[test]
fn algebra_accepts_more_than_two_sets() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    client.send("SADD s3 d z");
    assert_eq!(sorted(&client.send("SINTER s1 s2 s3")), vec!["d"]);
}

#[test]
fn a_single_key_is_returned_as_is() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(sorted(&client.send("SINTER s1")), vec!["a", "b", "c", "d"]);
}

#[test]
fn store_variants_write_the_result() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTERSTORE dst s1 s2"), "LEN 2\r\n");
    assert_eq!(sorted(&client.send("SMEMBERS dst")), vec!["c", "d"]);

    assert_eq!(client.send("SUNIONSTORE dst s1 s2"), "LEN 5\r\n");
    assert_eq!(client.send("SDIFFSTORE dst s1 s2"), "LEN 2\r\n");
    assert_eq!(sorted(&client.send("SMEMBERS dst")), vec!["a", "b"]);
}

#[test]
fn storing_an_empty_result_removes_the_destination() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    client.send("SADD dst placeholder");
    assert_eq!(client.send("SDIFFSTORE dst s1 s1"), "LEN 0\r\n");
    assert_eq!(client.send("EXISTS dst"), "COUNT 0\r\n");
}

#[test]
fn a_store_destination_may_also_be_a_source() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTERSTORE s1 s1 s2"), "LEN 2\r\n");
    assert_eq!(sorted(&client.send("SMEMBERS s1")), vec!["c", "d"]);
}

#[test]
fn smismember_answers_per_member() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    let reply = client.send("SMISMEMBER s1 a zz c");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["TRUE", "FALSE", "TRUE"]
    );
}

#[test]
fn spop_removes_what_it_returns() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    let popped = client.send("SPOP s1");
    let member = popped
        .strip_prefix("VALUE ")
        .and_then(|r| r.strip_suffix("\r\n"))
        .expect("a VALUE reply");
    assert_eq!(client.send("SCARD s1"), "LEN 3\r\n");
    assert_eq!(client.send(&format!("SISMEMBER s1 {member}")), "FALSE\r\n");
}

#[test]
fn spop_with_a_count_empties_the_key() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    let reply = client.send("SPOP s1 10");
    assert_eq!(lines_before_terminator(&reply, "END").len(), 4);
    assert_eq!(client.send("EXISTS s1"), "COUNT 0\r\n");
}

#[test]
fn spop_on_a_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SPOP nope"), "NOT_FOUND\r\n");
    let reply = client.send("SPOP nope 3");
    assert!(lines_before_terminator(&reply, "END").is_empty());
}

#[test]
fn srandmember_leaves_the_set_alone() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert!(client.send("SRANDMEMBER s1").starts_with("VALUE "));
    let reply = client.send("SRANDMEMBER s1 2");
    assert_eq!(lines_before_terminator(&reply, "END").len(), 2);
    assert_eq!(client.send("SCARD s1"), "LEN 4\r\n");
}

#[test]
fn srandmember_with_a_negative_count_may_repeat() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SADD one only");
    let reply = client.send("SRANDMEMBER one -3");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["only", "only", "only"]
    );
}

#[test]
fn srandmember_never_returns_more_than_the_set_holds() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    let reply = client.send("SRANDMEMBER s1 100");
    assert_eq!(lines_before_terminator(&reply, "END").len(), 4);
}

#[test]
fn smove_transfers_one_member() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SMOVE s1 s2 a"), "OK\r\n");
    assert_eq!(client.send("SISMEMBER s1 a"), "FALSE\r\n");
    assert_eq!(client.send("SISMEMBER s2 a"), "TRUE\r\n");
    assert_eq!(client.send("SMOVE s1 s2 nope"), "NOT_FOUND\r\n");
}

#[test]
fn smove_emptying_the_source_deletes_it() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SADD src only");
    client.send("SMOVE src dst only");
    assert_eq!(client.send("EXISTS src"), "COUNT 0\r\n");
    assert_eq!(client.send("SCARD dst"), "LEN 1\r\n");
}

#[test]
fn srem_keeps_its_single_member_reply_and_counts_multiples() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SREM s1 a"), "OK\r\n");
    assert_eq!(client.send("SREM s1 a"), "NOT_FOUND\r\n");
    assert_eq!(client.send("SREM s1 b c nope"), "DELETED 2\r\n");
    assert_eq!(client.send("SCARD s1"), "LEN 1\r\n");
}

#[test]
fn new_set_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    client.send("SADD real m");
    for cmd in [
        "SINTER s real",
        "SUNION real s",
        "SDIFF s",
        "SINTERSTORE dst s real",
        "SPOP s",
        "SRANDMEMBER s",
        "SMOVE s real m",
        "SMOVE real s m",
        "SMISMEMBER s m",
    ] {
        assert_eq!(client.send(cmd), WRONGTYPE, "for {cmd}");
    }
}
