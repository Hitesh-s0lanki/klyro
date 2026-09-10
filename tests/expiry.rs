//! Expiry commands beyond the original second-granularity EXPIRE/TTL:
//! the millisecond and absolute-timestamp variants, PERSIST, and GETEX.

mod common;

use common::KlyroServer;

/// Parses a `TTL 100\r\n` / `PTTL 99950\r\n` style reply.
fn number(reply: &str, prefix: &str) -> i64 {
    let body = reply
        .strip_prefix(prefix)
        .and_then(|r| r.strip_suffix("\r\n"))
        .unwrap_or_else(|| panic!("expected a {prefix:?} reply, got {reply:?}"));
    body.parse().expect("numeric reply body")
}

#[test]
fn pexpire_sets_a_millisecond_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("PEXPIRE k 100000"), "OK\r\n");

    let pttl = number(&client.send("PTTL k"), "PTTL ");
    assert!((90_000..=100_000).contains(&pttl), "got {pttl}");
    assert_eq!(number(&client.send("TTL k"), "TTL "), pttl / 1000);
}

#[test]
fn pttl_reports_missing_and_persistent_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("PTTL nope"), "PTTL -2\r\n");
    client.send("SET k v");
    assert_eq!(client.send("PTTL k"), "PTTL -1\r\n");
}

#[test]
fn expireat_takes_an_absolute_unix_timestamp() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    assert_eq!(client.send(&format!("EXPIREAT k {}", now + 100)), "OK\r\n");
    let ttl = number(&client.send("TTL k"), "TTL ");
    assert!((95..=100).contains(&ttl), "got {ttl}");
}

#[test]
fn expireat_in_the_past_expires_the_key_immediately() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("EXPIREAT k 1"), "OK\r\n");
    assert_eq!(client.send("GET k"), "NOT_FOUND\r\n");
}

#[test]
fn pexpireat_takes_absolute_milliseconds() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    assert_eq!(
        client.send(&format!("PEXPIREAT k {}", now_ms + 60_000)),
        "OK\r\n"
    );
    let ttl = number(&client.send("TTL k"), "TTL ");
    assert!((55..=60).contains(&ttl), "got {ttl}");
}

#[test]
fn persist_removes_a_ttl_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    client.send("EXPIRE k 100");
    assert_eq!(client.send("PERSIST k"), "OK\r\n");
    assert_eq!(client.send("TTL k"), "TTL -1\r\n");
    // A key with no TTL left to remove reports the same as a missing one.
    assert_eq!(client.send("PERSIST k"), "NOT_FOUND\r\n");
    assert_eq!(client.send("PERSIST nope"), "NOT_FOUND\r\n");
}

#[test]
fn expire_variants_report_missing_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for cmd in [
        "EXPIRE k 5",
        "PEXPIRE k 5000",
        "EXPIREAT k 99999999999",
        "PEXPIREAT k 99999999999",
    ] {
        assert_eq!(client.send(cmd), "NOT_FOUND\r\n", "for {cmd}");
    }
}

#[test]
fn getex_sets_clears_and_keeps_a_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");

    assert_eq!(client.send("GETEX k EX 100"), "VALUE v\r\n");
    let ttl = number(&client.send("TTL k"), "TTL ");
    assert!((95..=100).contains(&ttl), "got {ttl}");

    // No option at all leaves the TTL alone.
    assert_eq!(client.send("GETEX k"), "VALUE v\r\n");
    assert!(number(&client.send("TTL k"), "TTL ") > 0);

    assert_eq!(client.send("GETEX k PERSIST"), "VALUE v\r\n");
    assert_eq!(client.send("TTL k"), "TTL -1\r\n");
}

#[test]
fn getex_on_a_missing_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("GETEX nope EX 10"), "NOT_FOUND\r\n");
}
