//! Commands valid regardless of a key's type, plus connection and
//! shutdown behaviour and the WRONGTYPE matrix.

mod common;

use common::{bulk, int, nil, ok, KlyroServer, Value, WRONGTYPE};

#[test]
fn ping() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("PING"), Value::Simple("PONG".into()));
    // With an argument, PING echoes it.
    assert_eq!(client.send("PING hello"), bulk("hello"));
}

#[test]
fn unknown_command() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let message = client.send("BOGUS a b").error();
    assert!(
        message.starts_with("ERR unknown command 'BOGUS'"),
        "{message}"
    );
}

#[test]
fn del_counts_what_it_removed() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("DEL a"), int(1));
    assert_eq!(client.send("DEL a"), int(0));
    assert_eq!(client.send("DEL b nope"), int(1));
}

#[test]
fn unlink_is_an_alias_for_del() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET a 1");
    assert_eq!(client.send("UNLINK a"), int(1));
    assert_eq!(client.send("EXISTS a"), int(0));
}

#[test]
fn type_reports_every_kind() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    client.send("RPUSH l x");
    client.send("HSET h f v");
    client.send("SADD st m");
    client.send("ZADD z 1 m");

    for (key, kind) in [
        ("s", "string"),
        ("l", "list"),
        ("h", "hash"),
        ("st", "set"),
        ("z", "zset"),
    ] {
        assert_eq!(
            client.send(&format!("TYPE {key}")),
            Value::Simple(kind.into()),
            "for {key}"
        );
    }
    assert_eq!(client.send("TYPE missing"), Value::Simple("none".into()));
}

#[test]
fn dbsize_and_keys() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("DBSIZE"), int(0));
    client.send("MSET a 1 b 2");
    assert_eq!(client.send("DBSIZE"), int(2));
    assert_eq!(client.send("KEYS *").sorted(), vec!["a", "b"]);
}

#[test]
fn quit_closes_the_connection() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("QUIT"), ok());
    assert!(client.closed(), "QUIT should close the connection");
}

#[test]
fn save_writes_the_dump_file() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("SAVE"), ok());
    assert!(server.dump_path.exists());
    server.cleanup_dump();
}

#[test]
fn shutdown_stops_the_process() {
    let mut server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SHUTDOWN"), ok());
    let status = server.wait_for_exit(std::time::Duration::from_secs(5));
    assert!(status.success(), "klyro exited with {status:?}");
    server.cleanup_dump();
}

#[test]
fn a_command_against_the_wrong_type_is_refused() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    client.send("RPUSH l x");
    client.send("HSET h f v");
    client.send("SADD st m");
    client.send("ZADD z 1 m");

    for command in [
        "LPUSH s x",
        "HSET s f v",
        "SADD s m",
        "ZADD s 1 m",
        "GET l",
        "HGET l f",
        "SMEMBERS h",
        "ZSCORE h m",
        "LRANGE st 0 -1",
        "GET z",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}

#[test]
fn set_overwrites_a_key_of_any_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH k a b");
    assert_eq!(client.send("SET k now-a-string"), ok());
    assert_eq!(client.send("TYPE k"), Value::Simple("string".into()));
    assert_eq!(client.send("GET k"), bulk("now-a-string"));
}

#[test]
fn emptying_a_collection_deletes_its_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("RPUSH l only");
    assert_eq!(client.send("LPOP l"), bulk("only"));
    assert_eq!(client.send("EXISTS l"), int(0));

    client.send("HSET h f v");
    assert_eq!(client.send("HDEL h f"), int(1));
    assert_eq!(client.send("EXISTS h"), int(0));

    client.send("SADD s m");
    assert_eq!(client.send("SREM s m"), int(1));
    assert_eq!(client.send("EXISTS s"), int(0));

    client.send("ZADD z 1 m");
    assert_eq!(client.send("ZREM z m"), int(1));
    assert_eq!(client.send("EXISTS z"), int(0));
}

#[test]
fn reads_of_a_missing_key_are_empty_not_errors() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("GET missing"), nil());
    assert_eq!(client.send("LLEN missing"), int(0));
    assert_eq!(client.send("LRANGE missing 0 -1"), Value::Array(vec![]));
    assert_eq!(client.send("HLEN missing"), int(0));
    assert_eq!(client.send("HGETALL missing"), Value::Array(vec![]));
    assert_eq!(client.send("SCARD missing"), int(0));
    assert_eq!(client.send("SMEMBERS missing"), Value::Array(vec![]));
    assert_eq!(client.send("ZCARD missing"), int(0));
    assert_eq!(client.send("ZRANGE missing 0 -1"), Value::Array(vec![]));
    assert_eq!(client.send("LPOP missing"), nil());
}
