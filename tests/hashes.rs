//! Hash commands.

mod common;

use common::{bulk, int, nil, ok, KlyroServer, Value, WRONGTYPE};

#[test]
fn hset_is_variadic_and_counts_new_fields() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HSET u name Alice age 30"), int(2));
    // Overwriting an existing field adds nothing.
    assert_eq!(client.send("HSET u name Bob"), int(0));
    assert_eq!(client.send("HGET u name"), bulk("Bob"));
    assert_eq!(client.send("HLEN u"), int(2));
    assert_eq!(
        client.send("HSET u dangling").error(),
        "ERR wrong number of arguments for 'hset' command"
    );
}

#[test]
fn hmset_is_hset_with_the_older_reply() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HMSET u name Alice age 30"), ok());
    assert_eq!(client.send("HLEN u"), int(2));
}

#[test]
fn fields_and_values_may_contain_spaces() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.call(&["HSET", "u", "full name", "Alice Smith"]),
        int(1)
    );
    assert_eq!(
        client.call(&["HGET", "u", "full name"]),
        bulk("Alice Smith")
    );
}

#[test]
fn hsetnx_writes_only_a_missing_field() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HSETNX u f first"), int(1));
    assert_eq!(client.send("HSETNX u f second"), int(0));
    assert_eq!(client.send("HGET u f"), bulk("first"));
}

#[test]
fn hsetnx_on_a_missing_key_leaves_nothing_behind_when_it_declines() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HSET u f v");
    client.send("HDEL u f");
    // The key is gone; a declined HSETNX must not resurrect it empty.
    assert_eq!(client.send("HSETNX u f x"), int(1));
    assert_eq!(client.send("HLEN u"), int(1));
}

#[test]
fn hget_and_hmget() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2");
    assert_eq!(client.send("HGET u a"), bulk("1"));
    assert_eq!(client.send("HGET u missing"), nil());
    assert_eq!(
        client.send("HMGET u a b missing"),
        Value::Array(vec![bulk("1"), bulk("2"), nil()])
    );
    assert_eq!(
        client.send("HMGET nope a b"),
        Value::Array(vec![nil(), nil()])
    );
}

#[test]
fn hdel_counts_what_it_removed() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2 c 3");
    assert_eq!(client.send("HDEL u a"), int(1));
    assert_eq!(client.send("HDEL u a"), int(0));
    assert_eq!(client.send("HDEL u b c nope"), int(2));
    assert_eq!(client.send("EXISTS u"), int(0));
}

#[test]
fn hexists_and_hstrlen() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HSET u name Alice");
    assert_eq!(client.send("HEXISTS u name"), int(1));
    assert_eq!(client.send("HEXISTS u nope"), int(0));
    assert_eq!(client.send("HSTRLEN u name"), int(5));
    assert_eq!(client.send("HSTRLEN u nope"), int(0));
}

#[test]
fn hkeys_hvals_and_hgetall() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HMSET u a 1 b 2");
    assert_eq!(client.send("HKEYS u").sorted(), vec!["a", "b"]);
    assert_eq!(client.send("HVALS u").sorted(), vec!["1", "2"]);
    assert_eq!(
        client.send("HGETALL u").pairs(),
        vec![("a".to_string(), "1".to_string()), ("b".into(), "2".into())]
    );
    assert_eq!(client.send("HKEYS missing"), Value::Array(vec![]));
}

#[test]
fn hincrby_counts_within_a_hash() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HINCRBY u hits 1"), int(1));
    assert_eq!(client.send("HINCRBY u hits 9"), int(10));
    assert_eq!(client.send("HINCRBY u hits -4"), int(6));
    client.send("HSET u name Alice");
    assert_eq!(
        client.send("HINCRBY u name 1").error(),
        "ERR hash value is not an integer"
    );
}

#[test]
fn hincrbyfloat_accumulates_fractions() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("HINCRBYFLOAT u score 1.5"), bulk("1.5"));
    assert_eq!(client.send("HINCRBYFLOAT u score 2.25"), bulk("3.75"));
    client.send("HSET u name Alice");
    assert_eq!(
        client.send("HINCRBYFLOAT u name 1").error(),
        "ERR value is not a valid float"
    );
}

#[test]
fn hash_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for command in [
        "HSET s a 1",
        "HGET s a",
        "HMGET s a",
        "HDEL s a",
        "HLEN s",
        "HSETNX s a 1",
        "HEXISTS s a",
        "HKEYS s",
        "HVALS s",
        "HGETALL s",
        "HSTRLEN s a",
        "HINCRBY s a 1",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}
