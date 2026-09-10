//! Saving and reloading the keyspace: the round trip, TTLs across a
//! restart, and the version 1 dump format.

mod common;

use common::{bulk, int, nil, ok, KlyroServer};
use std::time::Duration;

#[test]
fn every_type_survives_a_save_and_reload() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        assert_eq!(client.send("DBSIZE"), int(0));
        client.call(&["SET", "greeting", "hello persistence"]);
        client.send("RPUSH mylist a b c");
        client.send("HSET user name Alice age 30");
        client.send("SADD tags fast small");
        client.send("ZADD board 100 alice 50 bob");
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        assert_eq!(client.send("GET greeting"), bulk("hello persistence"));
        assert_eq!(
            client.send("LRANGE mylist 0 -1").list(),
            vec!["a", "b", "c"]
        );
        assert_eq!(client.send("HGET user name"), bulk("Alice"));
        assert_eq!(client.send("HGET user age"), bulk("30"));
        assert_eq!(client.send("SMEMBERS tags").sorted(), vec!["fast", "small"]);
        assert_eq!(
            client.send("ZRANGE board 0 -1").list(),
            vec!["bob", "alice"]
        );
        assert_eq!(client.send("ZSCORE board alice"), bulk("100"));
        assert_eq!(client.send("DBSIZE"), int(5));
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn binary_values_survive_a_save_and_reload() {
    let mut server = KlyroServer::new();
    let key = b"awkward\r\nkey\0here".to_vec();
    let value = b"line one\nline two\0with a nul\xff".to_vec();
    {
        let mut client = server.connect();
        client.call_bytes(&[b"SET".to_vec(), key.clone(), value.clone()]);
        client.call_bytes(&[b"RPUSH".to_vec(), b"l".to_vec(), value.clone()]);
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        assert_eq!(
            client.call_bytes(&[b"GET".to_vec(), key.clone()]).bytes(),
            value
        );
        assert_eq!(client.send("LINDEX l 0").bytes(), value);
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn a_ttl_survives_a_restart_as_an_absolute_deadline() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("SET k v");
        client.send("EXPIRE k 300");
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        let ttl = client.send("TTL k").integer();
        assert!((280..=300).contains(&ttl), "got {ttl}");
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn an_expired_key_does_not_come_back() {
    let mut server = KlyroServer::new();
    {
        let mut client = server.connect();
        client.send("SET gone v");
        client.send("SET stays v");
        client.send("PEXPIRE gone 50");
        std::thread::sleep(Duration::from_millis(120));
    }
    server.shutdown();

    let mut reloaded = KlyroServer::reload(server.dump_path.clone());
    {
        let mut client = reloaded.connect();
        assert_eq!(client.send("GET gone"), nil());
        assert_eq!(client.send("GET stays"), bulk("v"));
    }
    reloaded.kill();
    reloaded.cleanup_dump();
}

#[test]
fn save_writes_immediately_without_stopping_the_server() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("SAVE"), ok());
    assert!(server.dump_path.exists());

    // The server keeps serving after a save.
    assert_eq!(client.send("GET k"), bulk("v"));
    server.cleanup_dump();
}

#[test]
fn a_version_1_dump_still_loads() {
    // The original text format, written by the pre-RESP server.
    let path = std::env::temp_dir().join(format!("klyro_v1_{}.dump", std::process::id()));
    std::fs::write(
        &path,
        "KLYRO-DUMP 1\n\
         STRING greeting hello there\n\
         LIST mylist 2\n\
         a\n\
         b\n\
         HASH user 1\n\
         name Alice\n\
         SET tags 1\n\
         fast\n\
         ZSET board 1\n\
         alice 100\n",
    )
    .unwrap();

    let mut server = KlyroServer::with_dump(0, path.clone());
    {
        let mut client = server.connect();
        assert_eq!(client.send("GET greeting"), bulk("hello there"));
        assert_eq!(client.send("LRANGE mylist 0 -1").list(), vec!["a", "b"]);
        assert_eq!(client.send("HGET user name"), bulk("Alice"));
        assert_eq!(client.send("SISMEMBER tags fast"), int(1));
        assert_eq!(client.send("ZSCORE board alice"), bulk("100"));
    }
    server.kill();
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_file_that_is_not_a_dump_is_ignored() {
    let path = std::env::temp_dir().join(format!("klyro_junk_{}.dump", std::process::id()));
    std::fs::write(&path, "this is not a dump file\n").unwrap();

    let mut server = KlyroServer::with_dump(0, path.clone());
    {
        let mut client = server.connect();
        assert_eq!(client.send("DBSIZE"), int(0));
    }
    server.kill();
    let _ = std::fs::remove_file(&path);
}
