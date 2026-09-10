//! Keyspace-wide commands: EXISTS, RENAME, COPY, RANDOMKEY, FLUSH.

mod common;

use common::{bulk, int, nil, ok, KlyroServer};

#[test]
fn exists_counts_every_key_it_is_given() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("EXISTS a"), int(1));
    assert_eq!(client.send("EXISTS nope"), int(0));
    assert_eq!(client.send("EXISTS a b nope"), int(2));
    // Redis counts a repeated key once per repetition.
    assert_eq!(client.send("EXISTS a a"), int(2));
}

#[test]
fn exists_sees_every_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH l x");
    client.send("HSET h f v");
    client.send("SADD s m");
    client.send("ZADD z 1 m");
    assert_eq!(client.send("EXISTS l h s z"), int(4));
}

#[test]
fn rename_moves_the_value_and_its_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET old v");
    client.send("EXPIRE old 100");
    assert_eq!(client.send("RENAME old new"), ok());
    assert_eq!(client.send("GET new"), bulk("v"));
    assert_eq!(client.send("EXISTS old"), int(0));
    assert!(client.send("TTL new").integer() > 90);
}

#[test]
fn rename_overwrites_the_destination_but_renamenx_refuses() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("RENAMENX a b"), int(0));
    assert_eq!(client.send("RENAME a b"), ok());
    assert_eq!(client.send("GET b"), bulk("1"));
}

#[test]
fn rename_reports_a_missing_source() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("RENAME nope other").error(), "ERR no such key");
    assert_eq!(
        client.send("RENAMENX nope other").error(),
        "ERR no such key"
    );
}

#[test]
fn copy_duplicates_a_value_independently() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH src a b");
    assert_eq!(client.send("COPY src dst"), int(1));
    // Mutating the copy must not disturb the original.
    client.send("RPUSH dst c");
    assert_eq!(client.send("LLEN src"), int(2));
    assert_eq!(client.send("LLEN dst"), int(3));
}

#[test]
fn copy_refuses_an_existing_destination_without_replace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("COPY a b"), int(0));
    assert_eq!(client.send("GET b"), bulk("2"));
    assert_eq!(client.send("COPY a b REPLACE"), int(1));
    assert_eq!(client.send("GET b"), bulk("1"));
    assert_eq!(client.send("COPY nope b"), int(0));
    assert_eq!(client.send("COPY a c BOGUS").error(), "ERR syntax error");
}

#[test]
fn randomkey_returns_a_stored_key_or_nil() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("RANDOMKEY"), nil());
    client.send("SET only v");
    assert_eq!(client.send("RANDOMKEY"), bulk("only"));
}

#[test]
fn flushdb_and_flushall_empty_the_keyspace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("FLUSHDB"), ok());
    assert_eq!(client.send("DBSIZE"), int(0));

    client.send("MSET a 1 b 2");
    assert_eq!(client.send("FLUSHALL"), ok());
    assert_eq!(client.send("DBSIZE"), int(0));
}

#[test]
fn dbsize_ignores_expired_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET live v");
    client.send("SET dead v");
    client.send("EXPIREAT dead 1");
    assert_eq!(client.send("DBSIZE"), int(1));
}

#[test]
fn echo_returns_its_argument() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.call(&["ECHO", "hello there"]), bulk("hello there"));
    assert_eq!(
        client.send("ECHO").error(),
        "ERR wrong number of arguments for 'echo' command"
    );
}
