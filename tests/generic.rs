//! Commands valid regardless of a key's type, plus keyspace-wide and
//! shutdown/save behavior. Ported from the old test_generic.py; each
//! test gets its own dedicated server (see tests/common/mod.rs).

mod common;

use common::{lines_before_terminator, KlyroServer};
use std::collections::HashSet;

#[test]
fn ping() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("PING"), "PONG\r\n");
}

#[test]
fn unknown_command() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("BOGUS"), "ERR unknown command\r\n");
}

#[test]
fn del() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET generic_del x");
    assert_eq!(client.send("DEL generic_del"), "OK\r\n");
    assert_eq!(client.send("DEL generic_del"), "NOT_FOUND\r\n");
}

#[test]
fn del_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("DEL generic_never_existed"), "NOT_FOUND\r\n");
}

#[test]
fn expire_and_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET generic_ek v");
    assert_eq!(client.send("EXPIRE generic_ek 100"), "OK\r\n");
    let resp = client.send("TTL generic_ek");
    assert!(
        resp == "TTL 100\r\n" || resp == "TTL 99\r\n",
        "got {resp:?}"
    );
}

#[test]
fn expire_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("EXPIRE generic_never_existed 5"),
        "NOT_FOUND\r\n"
    );
}

#[test]
fn ttl_no_expiry() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET generic_noexp v");
    assert_eq!(client.send("TTL generic_noexp"), "TTL -1\r\n");
}

#[test]
fn ttl_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("TTL generic_never_existed"), "TTL -2\r\n");
}

#[test]
fn type_string_and_missing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET generic_tk v");
    assert_eq!(client.send("TYPE generic_tk"), "STRING\r\n");
    assert_eq!(client.send("TYPE generic_never_existed"), "NONE\r\n");
}

#[test]
fn quit_closes_connection() {
    let server = KlyroServer::new();
    let mut c = server.connect();
    assert_eq!(c.send("QUIT"), "BYE\r\n");
}

#[test]
fn dbsize_and_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("DBSIZE"), "COUNT 0\r\n");
    client.send("SET a 1");
    client.send("SET b 2");
    assert_eq!(client.send("DBSIZE"), "COUNT 2\r\n");
    let resp = client.send("KEYS");
    let keys: HashSet<&str> = lines_before_terminator(&resp, "END").into_iter().collect();
    assert_eq!(keys, HashSet::from(["a", "b"]));
}

#[test]
fn save_replies_ok() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("SAVE"), "OK\r\n");
    server.kill();
    server.cleanup_dump();
}

#[test]
fn shutdown_stops_the_process() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SHUTDOWN"), "SHUTTING_DOWN\r\n");
    let status = server.wait_for_exit(std::time::Duration::from_secs(3));
    assert!(status.success());
    assert!(server.output().contains("ok"));
    server.cleanup_dump();
}
