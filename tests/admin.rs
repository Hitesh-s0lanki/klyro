//! INFO, CONFIG, and the config file.

mod common;

use common::{lines_before_terminator, KlyroServer};

/// Pulls `key:value` out of an INFO reply.
fn field<'a>(reply: &'a str, key: &str) -> &'a str {
    let prefix = format!("{}:", key);
    lines_before_terminator(reply, "END")
        .into_iter()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {key:?} line in {reply:?}"))
}

/// Pulls a parameter's value out of a CONFIG GET reply.
fn parameter<'a>(reply: &'a str, name: &str) -> &'a str {
    let prefix = format!("{} ", name);
    lines_before_terminator(reply, "END")
        .into_iter()
        .find_map(|line| line.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("no {name:?} in {reply:?}"))
}

#[test]
fn info_prints_every_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("INFO");
    for header in [
        "# Server",
        "# Clients",
        "# Memory",
        "# Persistence",
        "# Stats",
        "# Keyspace",
    ] {
        assert!(reply.contains(header), "no {header:?} in {reply:?}");
    }
}

#[test]
fn info_takes_a_single_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("INFO memory");
    assert!(reply.contains("# Memory"));
    assert!(!reply.contains("# Server"));
    assert!(field(&reply, "used_memory").parse::<u64>().unwrap() > 0);
}

#[test]
fn info_rejects_an_unknown_section() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client.send("INFO nonsense").starts_with("ERR usage:"));
}

#[test]
fn info_reports_the_configured_port_and_a_running_uptime() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("INFO server");
    assert_eq!(field(&reply, "tcp_port"), server.port.to_string());
    assert!(field(&reply, "uptime_in_seconds").parse::<u64>().is_ok());
}

#[test]
fn info_counts_commands() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("PING");
    client.send("PING");
    let reply = client.send("INFO stats");
    // The two PINGs plus this INFO.
    assert_eq!(field(&reply, "total_commands_processed"), "3");
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
    assert!(reply.contains("db0:keys=7,expires=1"), "got {reply:?}");
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
    assert_eq!(lines_before_terminator(&reply, "END").len(), 9);
    assert_eq!(parameter(&reply, "port"), server.port.to_string());

    let reply = client.send("CONFIG GET save*");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["save-interval 60"]
    );

    let reply = client.send("CONFIG GET nomatch");
    assert!(lines_before_terminator(&reply, "END").is_empty());
}

#[test]
fn config_set_changes_a_parameter() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("CONFIG SET maxclients 128"), "OK\r\n");
    let reply = client.send("CONFIG GET maxclients");
    assert_eq!(parameter(&reply, "maxclients"), "128");
    // INFO reads the same value, so the two can't disagree.
    assert_eq!(field(&client.send("INFO clients"), "maxclients"), "128");
}

#[test]
fn config_set_refuses_immutable_unknown_and_invalid() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("CONFIG SET port 1234"),
        "ERR parameter cannot be changed at runtime `port`\r\n"
    );
    assert_eq!(
        client.send("CONFIG SET bind 127.0.0.1"),
        "ERR parameter cannot be changed at runtime `bind`\r\n"
    );
    assert_eq!(
        client.send("CONFIG SET nonsense 1"),
        "ERR unknown parameter `nonsense`\r\n"
    );
    assert_eq!(
        client.send("CONFIG SET maxclients 0"),
        "ERR invalid value for parameter `maxclients`\r\n"
    );
}

#[test]
fn config_names_are_case_insensitive() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("CONFIG SET MAXCLIENTS 55"), "OK\r\n");
    assert_eq!(
        parameter(&client.send("CONFIG GET maxclients"), "maxclients"),
        "55"
    );
}

#[test]
fn config_usage_errors() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for cmd in [
        "CONFIG",
        "CONFIG NONSENSE",
        "CONFIG GET",
        "CONFIG SET maxclients",
    ] {
        assert!(client.send(cmd).starts_with("ERR usage:"), "for {cmd}");
    }
}

#[test]
fn config_resetstat_clears_the_counters() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    client.send("GET k");
    assert_eq!(client.send("CONFIG RESETSTAT"), "OK\r\n");
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
    assert_eq!(
        client.send("APPEND k 0123456789"),
        "ERR resulting string too long\r\n"
    );
    assert_eq!(client.send("APPEND k 12345"), "LEN 15\r\n");
}

#[test]
fn scan_default_count_is_configurable() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MSET a 1 b 2 c 3 d 4 e 5");
    client.send("CONFIG SET scan-default-count 2");
    let reply = client.send("SCAN 0");
    assert_eq!(reply.lines().count(), 3); // two keys plus the CURSOR line
    assert!(reply.ends_with("CURSOR 2\r\n"), "got {reply:?}");
}

#[test]
fn zadd_max_pairs_is_configurable() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("CONFIG SET zadd-max-pairs 2");
    assert_eq!(client.send("ZADD z 1 a 2 b"), "ADDED 2\r\n");
    assert_eq!(
        client.send("ZADD z 1 a 2 b 3 c"),
        "ERR too many score/member pairs\r\n"
    );
}

#[test]
fn dbfilename_redirects_the_next_save() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let redirected = std::env::temp_dir().join(format!("klyro_redirect_{}.dump", server.port));
    let _ = std::fs::remove_file(&redirected);

    client.send("SET k v");
    client.send(&format!("CONFIG SET dbfilename {}", redirected.display()));
    assert_eq!(client.send("SAVE"), "OK\r\n");

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
    // The whole existing test suite relies on this form, but assert it
    // directly so the compatibility is not just implied.
    let server = KlyroServer::new();
    let mut client = server.connect();
    let reply = client.send("CONFIG GET port");
    assert_eq!(parameter(&reply, "port"), server.port.to_string());
    let reply = client.send("CONFIG GET dbfilename");
    assert_eq!(
        parameter(&reply, "dbfilename"),
        server.dump_path.to_string_lossy()
    );
}
