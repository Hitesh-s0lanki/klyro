//! Set commands, including the SINTER/SUNION/SDIFF algebra.

mod common;

use common::{int, nil, KlyroServer, Value, WRONGTYPE};

fn two_sets(server: &KlyroServer) -> common::KlyroClient {
    let mut client = server.connect();
    client.send("SADD s1 a b c d");
    client.send("SADD s2 c d e");
    client
}

#[test]
fn sadd_counts_only_new_members() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SADD s a b c"), int(3));
    assert_eq!(client.send("SADD s c d"), int(1));
    assert_eq!(client.send("SCARD s"), int(4));
    assert_eq!(
        client.send("SADD s").error(),
        "ERR wrong number of arguments for 'sadd' command"
    );
}

#[test]
fn membership_and_members() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SISMEMBER s1 a"), int(1));
    assert_eq!(client.send("SISMEMBER s1 zz"), int(0));
    assert_eq!(
        client.send("SMEMBERS s1").sorted(),
        vec!["a", "b", "c", "d"]
    );
    assert_eq!(
        client.send("SMISMEMBER s1 a zz c"),
        Value::Array(vec![int(1), int(0), int(1)])
    );
}

#[test]
fn srem_counts_what_it_removed() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SREM s1 a"), int(1));
    assert_eq!(client.send("SREM s1 a"), int(0));
    assert_eq!(client.send("SREM s1 b c nope"), int(2));
    assert_eq!(client.send("SCARD s1"), int(1));
}

#[test]
fn sinter_sunion_and_sdiff() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTER s1 s2").sorted(), vec!["c", "d"]);
    assert_eq!(
        client.send("SUNION s1 s2").sorted(),
        vec!["a", "b", "c", "d", "e"]
    );
    assert_eq!(client.send("SDIFF s1 s2").sorted(), vec!["a", "b"]);
    // The difference is taken in the order given, so it is not
    // symmetric.
    assert_eq!(client.send("SDIFF s2 s1").sorted(), vec!["e"]);
}

#[test]
fn algebra_treats_a_missing_key_as_empty() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTER s1 nope"), Value::Array(vec![]));
    assert_eq!(
        client.send("SUNION s1 nope").sorted(),
        vec!["a", "b", "c", "d"]
    );
    assert_eq!(
        client.send("SDIFF s1 nope").sorted(),
        vec!["a", "b", "c", "d"]
    );
}

#[test]
fn algebra_accepts_more_than_two_sets() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    client.send("SADD s3 d z");
    assert_eq!(client.send("SINTER s1 s2 s3").sorted(), vec!["d"]);
}

#[test]
fn store_variants_write_the_result() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTERSTORE dst s1 s2"), int(2));
    assert_eq!(client.send("SMEMBERS dst").sorted(), vec!["c", "d"]);
    assert_eq!(client.send("SUNIONSTORE dst s1 s2"), int(5));
    assert_eq!(client.send("SDIFFSTORE dst s1 s2"), int(2));
    assert_eq!(client.send("SMEMBERS dst").sorted(), vec!["a", "b"]);
}

#[test]
fn storing_an_empty_result_removes_the_destination() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    client.send("SADD dst placeholder");
    assert_eq!(client.send("SDIFFSTORE dst s1 s1"), int(0));
    assert_eq!(client.send("EXISTS dst"), int(0));
}

#[test]
fn a_store_destination_may_also_be_a_source() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SINTERSTORE s1 s1 s2"), int(2));
    assert_eq!(client.send("SMEMBERS s1").sorted(), vec!["c", "d"]);
}

#[test]
fn spop_removes_what_it_returns() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    let member = client.send("SPOP s1").text();
    assert_eq!(client.send("SCARD s1"), int(3));
    assert_eq!(client.call(&["SISMEMBER", "s1", &member]), int(0));
}

#[test]
fn spop_with_a_count_empties_the_key() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SPOP s1 10").items().len(), 4);
    assert_eq!(client.send("EXISTS s1"), int(0));
}

#[test]
fn spop_on_a_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SPOP nope"), nil());
    assert_eq!(client.send("SPOP nope 3"), Value::Array(vec![]));
    assert_eq!(
        client.send("SPOP nope -1").error(),
        "ERR value is out of range, must be positive"
    );
}

#[test]
fn srandmember_leaves_the_set_alone() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert!(matches!(client.send("SRANDMEMBER s1"), Value::Bulk(_)));
    assert_eq!(client.send("SRANDMEMBER s1 2").items().len(), 2);
    assert_eq!(client.send("SCARD s1"), int(4));
    // Never more than the set holds, when the count is positive.
    assert_eq!(client.send("SRANDMEMBER s1 100").items().len(), 4);
}

#[test]
fn srandmember_with_a_negative_count_may_repeat() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SADD one only");
    assert_eq!(
        client.send("SRANDMEMBER one -3").list(),
        vec!["only", "only", "only"]
    );
}

#[test]
fn smove_transfers_one_member() {
    let server = KlyroServer::new();
    let mut client = two_sets(&server);
    assert_eq!(client.send("SMOVE s1 s2 a"), int(1));
    assert_eq!(client.send("SISMEMBER s1 a"), int(0));
    assert_eq!(client.send("SISMEMBER s2 a"), int(1));
    assert_eq!(client.send("SMOVE s1 s2 nope"), int(0));
}

#[test]
fn smove_emptying_the_source_deletes_it() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SADD src only");
    client.send("SMOVE src dst only");
    assert_eq!(client.send("EXISTS src"), int(0));
    assert_eq!(client.send("SCARD dst"), int(1));
}

#[test]
fn members_may_contain_spaces() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.call(&["SADD", "s", "two words", "three more"]),
        int(2)
    );
    assert_eq!(client.call(&["SISMEMBER", "s", "two words"]), int(1));
    assert_eq!(
        client.send("SMEMBERS s").sorted(),
        vec!["three more", "two words"]
    );
}

#[test]
fn set_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    client.send("SADD real m");
    for command in [
        "SADD s m",
        "SREM s m",
        "SCARD s",
        "SMEMBERS s",
        "SISMEMBER s m",
        "SMISMEMBER s m",
        "SINTER s real",
        "SUNION real s",
        "SDIFF s",
        "SINTERSTORE dst s real",
        "SPOP s",
        "SRANDMEMBER s",
        "SMOVE s real m",
        "SMOVE real s m",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}
