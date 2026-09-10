//! Keyspace-wide commands: EXISTS, the variadic DEL, RENAME, COPY,
//! RANDOMKEY, and FLUSHDB/FLUSHALL.

mod common;

use common::KlyroServer;

#[test]
fn exists_counts_every_key_it_is_given() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET a 1");
    client.send("SET b 2");
    assert_eq!(client.send("EXISTS a"), "COUNT 1\r\n");
    assert_eq!(client.send("EXISTS nope"), "COUNT 0\r\n");
    assert_eq!(client.send("EXISTS a b nope"), "COUNT 2\r\n");
    // Redis counts a repeated key once per repetition.
    assert_eq!(client.send("EXISTS a a"), "COUNT 2\r\n");
}

#[test]
fn exists_sees_every_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH l x");
    client.send("HSET h f v");
    client.send("SADD s m");
    client.send("ZADD z 1 m");
    assert_eq!(client.send("EXISTS l h s z"), "COUNT 4\r\n");
}

#[test]
fn del_keeps_its_single_key_reply_and_counts_multiples() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2 c 3");
    // One key: the original reply shape, unchanged.
    assert_eq!(client.send("DEL a"), "OK\r\n");
    assert_eq!(client.send("DEL a"), "NOT_FOUND\r\n");
    // Several keys: a count instead.
    assert_eq!(client.send("DEL b c nope"), "DELETED 2\r\n");
    assert_eq!(client.send("DBSIZE"), "COUNT 0\r\n");
}

#[test]
fn unlink_is_an_alias_for_del() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET a 1");
    assert_eq!(client.send("UNLINK a"), "OK\r\n");
    assert_eq!(client.send("EXISTS a"), "COUNT 0\r\n");
}

#[test]
fn rename_moves_the_value_and_its_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET old v");
    client.send("EXPIRE old 100");
    assert_eq!(client.send("RENAME old new"), "OK\r\n");
    assert_eq!(client.send("GET new"), "VALUE v\r\n");
    assert_eq!(client.send("EXISTS old"), "COUNT 0\r\n");
    assert!(client.send("TTL new").starts_with("TTL 9"));
}

#[test]
fn rename_overwrites_the_destination_but_renamenx_refuses() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET a 1");
    client.send("SET b 2");
    assert_eq!(client.send("RENAMENX a b"), "FALSE\r\n");
    assert_eq!(client.send("RENAME a b"), "OK\r\n");
    assert_eq!(client.send("GET b"), "VALUE 1\r\n");
}

#[test]
fn rename_reports_a_missing_source() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("RENAME nope other"), "NOT_FOUND\r\n");
    assert_eq!(client.send("RENAMENX nope other"), "NOT_FOUND\r\n");
}

#[test]
fn copy_duplicates_a_value_independently() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH src a b");
    assert_eq!(client.send("COPY src dst"), "OK\r\n");
    // Mutating the copy must not disturb the original.
    client.send("RPUSH dst c");
    assert_eq!(client.send("LLEN src"), "LEN 2\r\n");
    assert_eq!(client.send("LLEN dst"), "LEN 3\r\n");
}

#[test]
fn copy_refuses_an_existing_destination_without_replace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET a 1");
    client.send("SET b 2");
    assert_eq!(client.send("COPY a b"), "FALSE\r\n");
    assert_eq!(client.send("GET b"), "VALUE 2\r\n");
    assert_eq!(client.send("COPY a b REPLACE"), "OK\r\n");
    assert_eq!(client.send("GET b"), "VALUE 1\r\n");
    assert_eq!(client.send("COPY nope b"), "NOT_FOUND\r\n");
}

#[test]
fn randomkey_returns_a_stored_key_or_reports_empty() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("RANDOMKEY"), "NOT_FOUND\r\n");
    client.send("SET only v");
    assert_eq!(client.send("RANDOMKEY"), "VALUE only\r\n");
}

#[test]
fn flushdb_and_flushall_empty_the_keyspace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("FLUSHDB"), "OK\r\n");
    assert_eq!(client.send("DBSIZE"), "COUNT 0\r\n");

    client.send("MSET a 1 b 2");
    assert_eq!(client.send("FLUSHALL"), "OK\r\n");
    assert_eq!(client.send("DBSIZE"), "COUNT 0\r\n");
}

#[test]
fn dbsize_ignores_expired_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET live v");
    client.send("SET dead v");
    client.send("EXPIREAT dead 1");
    assert_eq!(client.send("DBSIZE"), "COUNT 1\r\n");
}

#[test]
fn echo_returns_its_argument() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("ECHO hello there"), "VALUE hello there\r\n");
    assert_eq!(client.send("ECHO"), "ERR usage: ECHO message\r\n");
}
