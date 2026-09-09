//! Save/kill/reload round-trip, TTL across a restart. Ported from the
//! old test_persistence.py. Each test needs its own dump-file lifecycle.

mod common;

use common::KlyroServer;
use std::collections::HashSet;
use std::time::Duration;

#[test]
fn fresh_dump_path_starts_empty() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("DBSIZE"), "COUNT 0\r\n");
    server.kill();
    server.cleanup_dump();
}

#[test]
fn round_trip_across_all_types_after_sigkill() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET greeting hello persistence");
    client.send("LPUSH mylist a b c");
    client.send("HSET user name Alice");
    client.send("SADD tags fast reliable");
    client.send("ZADD board 100 alice 50 bob");
    assert_eq!(client.send("SAVE"), "OK\r\n");
    server.kill(); // SIGKILL, not SHUTDOWN - proves the save already on disk survives

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    let mut c2 = reloaded.connect();
    assert_eq!(c2.send("GET greeting"), "VALUE hello persistence\r\n");
    assert_eq!(c2.send("TYPE mylist"), "LIST\r\n");
    assert_eq!(c2.send("LRANGE mylist 0 -1"), "c\r\nb\r\na\r\nEND\r\n");
    assert_eq!(c2.send("TYPE user"), "HASH\r\n");
    assert_eq!(c2.send("HGET user name"), "VALUE Alice\r\n");
    assert_eq!(c2.send("TYPE tags"), "SET\r\n");
    let resp = c2.send("SMEMBERS tags");
    let members: HashSet<&str> = common::lines_before_terminator(&resp, "END")
        .into_iter()
        .collect();
    assert_eq!(members, HashSet::from(["fast", "reliable"]));
    assert_eq!(c2.send("TYPE board"), "ZSET\r\n");
    assert_eq!(
        c2.send("ZRANGE board 0 -1"),
        "bob 50\r\nalice 100\r\nEND\r\n"
    );

    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn ttl_survives_a_restart() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET longlived v");
    client.send("EXPIRE longlived 300");
    client.send("SAVE");
    server.kill();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    let mut c2 = reloaded.connect();
    let resp = c2.send("TTL longlived");
    let ttl: i64 = resp.split_whitespace().nth(1).unwrap().parse().unwrap();
    assert!(ttl > 290, "expected ttl > 290, got {ttl}");
    assert!(ttl <= 300, "expected ttl <= 300, got {ttl}");

    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn key_expired_during_downtime_is_gone_on_reload() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET shortlived x");
    client.send("EXPIRE shortlived 1");
    client.send("SAVE");
    std::thread::sleep(Duration::from_millis(1200)); // let it actually expire before "restarting"
    server.kill();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    let mut c2 = reloaded.connect();
    assert_eq!(c2.send("GET shortlived"), "NOT_FOUND\r\n");
    assert_eq!(c2.send("TYPE shortlived"), "NONE\r\n");

    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn graceful_shutdown_also_saves() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET savedbyshutdown v");
    client.send("SHUTDOWN"); // no explicit SAVE - relies on shutdown's own save
    server.wait_for_exit(Duration::from_secs(3));

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    let mut c2 = reloaded.connect();
    assert_eq!(c2.send("GET savedbyshutdown"), "VALUE v\r\n");

    reloaded.kill();
    reloaded.cleanup_dump();
}
