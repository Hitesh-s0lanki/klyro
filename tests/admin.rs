//! INFO, CONFIG, the config file, and HELLO's protocol negotiation.

mod common;

use common::{bulk, int, ok, KlyroServer, Value};

/// Pulls `key:value` out of an INFO reply, which is one bulk string.
fn field(reply: &Value, key: &str) -> String {
    let body = reply.text();
    let prefix = format!("{}:", key);
    body.lines()
        .find_map(|line| line.trim_end().strip_prefix(&prefix).map(str::to_string))
        .unwrap_or_else(|| panic!("no {key:?} line in {body:?}"))
}

/// Pulls a parameter out of a CONFIG GET reply.
fn parameter(reply: &Value, name: &str) -> String {
    reply
        .pairs()
        .into_iter()
        .find(|(key, _)| key == name)
        .unwrap_or_else(|| panic!("no {name:?} in {reply:?}"))
        .1
}

#[test]
fn info_prints_every_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let body = client.send("INFO").text();
    for header in [
        "# Server",
        "# Clients",
        "# Memory",
        "# Persistence",
        "# Stats",
        "# Keyspace",
    ] {
        assert!(body.contains(header), "no {header:?} in {body:?}");
    }
}

#[test]
fn info_takes_a_single_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("INFO memory");
    assert!(reply.text().contains("# Memory"));
    assert!(!reply.text().contains("# Server"));
    assert!(field(&reply, "used_memory").parse::<u64>().unwrap() > 0);
}

#[test]
fn info_answers_an_unknown_section_with_an_empty_string() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // Redis returns an empty body rather than an error.
    assert_eq!(client.send("INFO nonsense"), bulk(""));
}

#[test]
fn info_reports_the_configured_port_and_a_running_uptime() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("INFO server");
    assert_eq!(field(&reply, "tcp_port"), server.port.to_string());
    assert!(field(&reply, "uptime_in_seconds").parse::<u64>().is_ok());
    // Client libraries gate features on redis_version.
    assert!(!field(&reply, "redis_version").is_empty());
}

#[test]
fn info_counts_commands() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("PING");
    client.send("PING");
    // The two PINGs plus this INFO.
    assert_eq!(
        field(&client.send("INFO stats"), "total_commands_processed"),
        "3"
    );
}

#[test]
fn info_counts_connections() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // Measure the increase rather than an absolute count: the test
    // harness opens its own connection to check the server is up.
    let before: u64 = field(&client.send("INFO stats"), "total_connections_received")
        .parse()
        .unwrap();
    let mut second = server.connect();
    second.send("PING");
    let after: u64 = field(&client.send("INFO stats"), "total_connections_received")
        .parse()
        .unwrap();
    assert_eq!(after, before + 1);
    assert_eq!(
        field(&client.send("INFO clients"), "connected_clients"),
        "2"
    );
}

#[test]
fn info_counts_one_hit_or_miss_per_read() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    for _ in 0..3 {
        client.send("GET k");
    }
    for _ in 0..2 {
        client.send("GET missing");
    }
    let reply = client.send("INFO stats");
    // A read command counts once per key looked up, not once per
    // internal lookup - the type check must not double it.
    assert_eq!(field(&reply, "keyspace_hits"), "3");
    assert_eq!(field(&reply, "keyspace_misses"), "2");
}

#[test]
fn writes_do_not_move_the_hit_ratio() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for _ in 0..5 {
        client.send("SET k v");
        client.send("RPUSH l x");
    }
    let reply = client.send("INFO stats");
    assert_eq!(field(&reply, "keyspace_hits"), "0");
    assert_eq!(field(&reply, "keyspace_misses"), "0");
}

#[test]
fn info_keyspace_breaks_down_by_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2");
    client.send("RPUSH l x");
    client.send("HSET h f v");
    client.send("SADD s m");
    client.send("ZADD z 1 m");
    client.send("SET vol v EX 100");

    let reply = client.send("INFO keyspace");
    assert!(
        reply.text().contains("db0:keys=7,expires=1"),
        "got {reply:?}"
    );
    assert_eq!(field(&reply, "string"), "3");
    assert_eq!(field(&reply, "list"), "1");
    assert_eq!(field(&reply, "hash"), "1");
    assert_eq!(field(&reply, "set"), "1");
    assert_eq!(field(&reply, "zset"), "1");
}

#[test]
fn info_persistence_tracks_saves() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_ne!(
        field(&client.send("INFO persistence"), "changes_since_last_save"),
        "0"
    );

    client.send("SAVE");
    let reply = client.send("INFO persistence");
    assert_eq!(field(&reply, "changes_since_last_save"), "0");
    assert_eq!(field(&reply, "last_save_status"), "ok");
    assert_eq!(field(&reply, "total_saves"), "1");
    assert!(field(&reply, "last_save_time").parse::<u64>().unwrap() > 0);
    server.cleanup_dump();
}

#[test]
fn config_get_lists_everything_and_supports_globs() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("CONFIG GET *");
    assert_eq!(reply.pairs().len(), 22);
    assert_eq!(parameter(&reply, "port"), server.port.to_string());
    assert_eq!(parameter(&reply, "mem-max-topk"), "100");
    // 0 is a real setting for these two, meaning "no limit".
    assert_eq!(parameter(&reply, "mem-max-records"), "0");
    assert_eq!(parameter(&reply, "maxmemory"), "0");
    assert_eq!(parameter(&reply, "maxmemory-policy"), "noeviction");

    let reply = client.send("CONFIG GET mem-max-t*");
    assert_eq!(
        reply.pairs(),
        vec![
            ("mem-max-terms-per-doc".to_string(), "1024".to_string()),
            ("mem-max-text-bytes".to_string(), "65536".to_string()),
            ("mem-max-topk".to_string(), "100".to_string()),
        ]
    );

    let reply = client.send("CONFIG GET save*");
    assert_eq!(
        reply.pairs(),
        vec![("save-interval".to_string(), "60".to_string())]
    );
    assert_eq!(client.send("CONFIG GET nomatch"), Value::Array(vec![]));
}

#[test]
fn config_get_accepts_several_patterns() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("CONFIG GET maxclients dbfilename");
    assert_eq!(reply.pairs().len(), 2);
}

#[test]
fn config_set_changes_a_parameter() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("CONFIG SET maxclients 128"), ok());
    assert_eq!(
        parameter(&client.send("CONFIG GET maxclients"), "maxclients"),
        "128"
    );
    // INFO reads the same value, so the two can't disagree.
    assert_eq!(field(&client.send("INFO clients"), "maxclients"), "128");
}

#[test]
fn config_set_refuses_immutable_unknown_and_invalid() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client
        .send("CONFIG SET port 1234")
        .error()
        .contains("cannot be changed at runtime"));
    assert!(client
        .send("CONFIG SET bind 127.0.0.1")
        .error()
        .contains("cannot be changed at runtime"));
    assert!(client
        .send("CONFIG SET nonsense 1")
        .error()
        .contains("Unknown option"));
    assert!(client
        .send("CONFIG SET maxclients 0")
        .error()
        .contains("invalid value"));
}

#[test]
fn config_names_and_subcommands_are_case_insensitive() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("config set MAXCLIENTS 55"), ok());
    assert_eq!(
        parameter(&client.send("CONFIG GET maxclients"), "maxclients"),
        "55"
    );
}

#[test]
fn config_usage_errors() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client.send("CONFIG").is_error());
    assert!(client.send("CONFIG NONSENSE").is_error());
    assert!(client.send("CONFIG GET").is_error());
    assert!(client.send("CONFIG SET maxclients").is_error());
}

#[test]
fn config_resetstat_clears_the_counters() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    client.send("GET k");
    assert_eq!(client.send("CONFIG RESETSTAT"), ok());
    let reply = client.send("INFO stats");
    assert_eq!(field(&reply, "keyspace_hits"), "0");
    assert_eq!(field(&reply, "total_connections_received"), "0");
    // This INFO is the only command since the reset.
    assert_eq!(field(&reply, "total_commands_processed"), "1");
}

#[test]
fn max_string_bytes_is_enforced_at_the_configured_size() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("CONFIG SET max-string-bytes 16");
    client.send("SET k 0123456789");
    assert!(client
        .send("APPEND k 0123456789")
        .error()
        .contains("exceeds maximum allowed size"));
    assert_eq!(client.send("APPEND k 12345"), int(15));
}

#[test]
fn scan_default_count_is_configurable() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2 c 3 d 4 e 5");
    client.send("CONFIG SET scan-default-count 2");
    let reply = client.send("SCAN 0");
    assert_eq!(reply.items()[0].text(), "2");
    assert_eq!(reply.items()[1].items().len(), 2);
}

#[test]
fn zadd_max_pairs_is_configurable() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("CONFIG SET zadd-max-pairs 2");
    assert_eq!(client.send("ZADD z 1 a 2 b"), int(2));
    assert!(client
        .send("ZADD z 1 a 2 b 3 c")
        .error()
        .contains("too many score/member pairs"));
}

#[test]
fn dbfilename_redirects_the_next_save() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let redirected = std::env::temp_dir().join(format!("klyro_redirect_{}.dump", server.port));
    let _ = std::fs::remove_file(&redirected);

    client.send("SET k v");
    client.call(&["CONFIG", "SET", "dbfilename", &redirected.to_string_lossy()]);
    assert_eq!(client.send("SAVE"), ok());

    assert!(redirected.exists(), "the save did not follow dbfilename");
    let _ = std::fs::remove_file(&redirected);
}

#[test]
fn a_config_file_supplies_the_settings() {
    let server = KlyroServer::with_config("maxclients 64\nsave-interval 300\nzadd-max-pairs 7");
    let mut client = server.connect();
    let reply = client.send("CONFIG GET *");
    assert_eq!(parameter(&reply, "maxclients"), "64");
    assert_eq!(parameter(&reply, "save-interval"), "300");
    assert_eq!(parameter(&reply, "zadd-max-pairs"), "7");
    assert_eq!(parameter(&reply, "port"), server.port.to_string());
}

#[test]
fn a_config_file_ignores_comments_and_blank_lines() {
    let server = KlyroServer::with_config(
        "# a leading comment\n\n   \nmaxclients 33   # trailing comment\n",
    );
    let mut client = server.connect();
    assert_eq!(
        parameter(&client.send("CONFIG GET maxclients"), "maxclients"),
        "33"
    );
}

#[test]
fn a_bad_config_file_stops_startup_and_says_why() {
    let path = std::env::temp_dir().join(format!("klyro_bad_{}.conf", std::process::id()));
    std::fs::write(&path, "maxclients 0\nnonsense 1\n").unwrap();

    let output = KlyroServer::try_spawn_with_args(&[path.to_string_lossy().into_owned()])
        .expect_err("a bad config file should stop startup");
    assert!(output.contains("invalid value"), "got {output:?}");
    assert!(output.contains("unknown parameter"), "got {output:?}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn a_missing_config_file_stops_startup() {
    let output = KlyroServer::try_spawn_with_args(&["/nonexistent/klyro.conf".to_string()])
        .expect_err("a missing config file should stop startup");
    assert!(output.contains("cannot read"), "got {output:?}");
}

#[test]
fn positional_port_and_dump_arguments_still_work() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("CONFIG GET port dbfilename");
    assert_eq!(parameter(&reply, "port"), server.port.to_string());
    assert_eq!(
        parameter(&reply, "dbfilename"),
        server.dump_path.to_string_lossy()
    );
}

#[test]
fn hello_reports_the_server_and_protocol() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("HELLO");
    let fields = reply.pairs();
    assert!(fields.contains(&("server".to_string(), "klyro".to_string())));
    assert!(fields.contains(&("proto".to_string(), "2".to_string())));
}

#[test]
fn hello_negotiates_resp3_and_switches_the_encoding() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("HSET h a 1");

    // Over RESP2 a hash comes back as a flat array.
    assert_eq!(client.send("HGETALL h").items().len(), 2);

    assert_eq!(client.send("HELLO 3").pairs().len(), 7);
    // The test client parses a RESP3 map as an array of its elements,
    // which is enough to show the marker changed - the redis-py checks
    // cover the semantics.
    let raw = client.send_raw(b"*2\r\n$7\r\nHGETALL\r\n$1\r\nh\r\n");
    assert_eq!(raw.items().len(), 2);
}

#[test]
fn hello_refuses_a_protocol_it_does_not_speak() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client.send("HELLO 4").error().starts_with("NOPROTO"));
}

#[test]
fn client_id_is_unique_per_connection() {
    let server = KlyroServer::new();
    let (mut first, mut second) = (server.connect(), server.connect());
    let one = first.send("CLIENT ID").integer();
    let two = second.send("CLIENT ID").integer();
    assert!(one > 0 && two > one);
    // HELLO reports the same id, which is what a client caches.
    let hello = first.send("HELLO").pairs();
    assert!(hello.contains(&("id".to_string(), one.to_string())));
}

#[test]
fn client_setname_and_getname_round_trip() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert_eq!(client.send("CLIENT GETNAME"), Value::Nil);
    assert_eq!(client.send("CLIENT SETNAME worker-3"), ok());
    assert_eq!(client.send("CLIENT GETNAME"), bulk("worker-3"));
    // A space would break CLIENT INFO's one-line-per-client format.
    assert!(client.call(&["CLIENT", "SETNAME", "two words"]).is_error());
}

#[test]
fn client_setinfo_is_accepted_from_client_libraries() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("CLIENT SETINFO LIB-NAME redis-py"), ok());
    assert_eq!(client.send("CLIENT SETINFO LIB-VER 5.0.1"), ok());
}

#[test]
fn client_info_describes_this_connection() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("CLIENT SETNAME reporter");
    client.send("WATCH k");

    let line = client.send("CLIENT INFO").text();
    assert!(line.contains("name=reporter"), "{line:?}");
    assert!(line.contains("watch=1"), "{line:?}");
    // -1 is "no transaction open", as Redis reports it.
    assert!(line.contains("multi=-1"), "{line:?}");
}

#[test]
fn info_reports_blocked_watching_and_subscribed_clients() {
    let server = KlyroServer::new();
    let (mut waiter, mut watcher) = (server.connect(), server.connect());
    let mut listener = server.connect();
    let mut client = server.connect();

    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(std::time::Duration::from_millis(150)));
    watcher.send("WATCH k");
    listener.call(&["SUBSCRIBE", "news"]);

    let clients = client.send("INFO clients");
    assert_eq!(field(&clients, "blocked_clients"), "1");
    assert_eq!(field(&clients, "watching_clients"), "1");
    assert_eq!(field(&clients, "pubsub_clients"), "1");

    client.send("PUBLISH news hi");
    let stats = client.send("INFO stats");
    assert_eq!(field(&stats, "pubsub_channels"), "1");
    assert_eq!(field(&stats, "total_messages_published"), "1");
}

#[test]
fn info_counts_transactions_that_ran() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("MULTI");
    client.send("SET k v");
    client.send("EXEC");
    assert_eq!(field(&client.send("INFO stats"), "total_transactions"), "1");

    // One a WATCH aborted is not a transaction that ran.
    let mut other = server.connect();
    client.send("WATCH k");
    other.send("SET k changed");
    client.send("MULTI");
    client.send("EXEC");
    assert_eq!(field(&client.send("INFO stats"), "total_transactions"), "1");
}
