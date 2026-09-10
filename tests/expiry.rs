//! Expiry: the second- and millisecond-granularity commands, the
//! absolute-timestamp variants, PERSIST, and lazy expiration.

mod common;

use common::{bulk, int, nil, KlyroServer};

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[test]
fn expire_and_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("EXPIRE k 100"), int(1));
    assert!((95..=100).contains(&client.send("TTL k").integer()));
}

#[test]
fn ttl_reports_missing_and_persistent_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("TTL nope"), int(-2));
    assert_eq!(client.send("PTTL nope"), int(-2));
    client.send("SET k v");
    assert_eq!(client.send("TTL k"), int(-1));
    assert_eq!(client.send("PTTL k"), int(-1));
}

#[test]
fn pexpire_sets_a_millisecond_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("PEXPIRE k 100000"), int(1));

    let pttl = client.send("PTTL k").integer();
    assert!((90_000..=100_000).contains(&pttl), "got {pttl}");
    assert_eq!(client.send("TTL k").integer(), pttl / 1000);
}

#[test]
fn expireat_takes_an_absolute_unix_timestamp() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(
        client.send(&format!("EXPIREAT k {}", unix_now() + 100)),
        int(1)
    );
    assert!((95..=100).contains(&client.send("TTL k").integer()));
}

#[test]
fn pexpireat_takes_absolute_milliseconds() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(
        client.send(&format!("PEXPIREAT k {}", (unix_now() + 60) * 1000)),
        int(1)
    );
    assert!((55..=60).contains(&client.send("TTL k").integer()));
}

#[test]
fn an_expired_key_reads_as_missing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    client.send("EXPIREAT k 1");
    assert_eq!(client.send("GET k"), nil());
    assert_eq!(client.send("EXISTS k"), int(0));
    assert_eq!(client.send("TTL k"), int(-2));
}

#[test]
fn persist_removes_a_ttl_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    client.send("EXPIRE k 100");
    assert_eq!(client.send("PERSIST k"), int(1));
    assert_eq!(client.send("TTL k"), int(-1));
    // A key with no TTL left to remove reports the same as a missing one.
    assert_eq!(client.send("PERSIST k"), int(0));
    assert_eq!(client.send("PERSIST nope"), int(0));
}

#[test]
fn expire_variants_report_missing_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for command in [
        "EXPIRE k 5",
        "PEXPIRE k 5000",
        "EXPIREAT k 99999999999",
        "PEXPIREAT k 99999999999",
    ] {
        assert_eq!(client.send(command), int(0), "for {command}");
    }
}

#[test]
fn a_ttl_survives_the_key_being_read_and_updated_in_place() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k 1");
    client.send("EXPIRE k 100");
    client.send("INCR k");
    assert_eq!(client.send("GET k"), bulk("2"));
    assert!(client.send("TTL k").integer() > 0);
}
