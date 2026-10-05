//! `maxmemory` and the eviction policies.
//!
//! These tests drive a real server against a real allocator, so they
//! never assert on an exact byte count. Each one reads what the process
//! is using, sets the limit a fixed distance above that, and then
//! asserts on behaviour - which keys survive, which command is refused,
//! which counter moved - rather than on the number itself.

mod common;

use common::{int, ok, KlyroClient, KlyroServer, Value};

/// Pulls `key:value` out of an INFO reply.
fn field(reply: &Value, key: &str) -> String {
    let body = reply.text();
    let prefix = format!("{}:", key);
    body.lines()
        .find_map(|line| line.trim_end().strip_prefix(&prefix).map(str::to_string))
        .unwrap_or_else(|| panic!("no {key:?} line in {body:?}"))
}

fn info_number(client: &mut KlyroClient, key: &str) -> usize {
    field(&client.send("INFO"), key)
        .parse()
        .unwrap_or_else(|_| panic!("{key} is not a number"))
}

/// Room to add before the limit bites. Big enough that the process's
/// own background drift cannot cross it on its own, small enough that
/// a handful of values does.
const HEADROOM: usize = 256 * 1024;

/// One value, sized so that a few dozen of them fill the headroom.
const VALUE_BYTES: usize = 16 * 1024;

/// Sets a policy and a limit `HEADROOM` bytes above what the server is
/// using right now.
fn cap(client: &mut KlyroClient, policy: &str) {
    let used = info_number(client, "used_memory");
    assert_eq!(
        client.call(&["CONFIG", "SET", "maxmemory-policy", policy]),
        ok()
    );
    assert_eq!(
        client.call(&["CONFIG", "SET", "maxmemory", &(used + HEADROOM).to_string()]),
        ok()
    );
}

/// Writes `key` with a value large enough to move the needle.
fn fill(client: &mut KlyroClient, key: &str) -> Value {
    client.call(&["SET", key, &"x".repeat(VALUE_BYTES)])
}

/// Writes values until one is refused, and returns that refusal. Fails
/// the test if the server accepted every write.
fn write_until_refused(client: &mut KlyroClient, prefix: &str) -> String {
    for i in 0..2_000 {
        if let Value::Error(message) = fill(client, &format!("{prefix}:{i}")) {
            return message;
        }
    }
    panic!("the server never refused a write");
}

/// Writes `count` values, asserting each one was accepted.
fn write_all(client: &mut KlyroClient, prefix: &str, count: usize) {
    for i in 0..count {
        assert_eq!(
            fill(client, &format!("{prefix}:{i}")),
            ok(),
            "write {i} was refused"
        );
    }
}

#[test]
fn no_limit_is_the_default() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let info = client.send("INFO memory");
    assert_eq!(field(&info, "maxmemory"), "0");
    assert_eq!(field(&info, "maxmemory_human"), "unlimited");
    assert_eq!(field(&info, "maxmemory_policy"), "noeviction");
}

#[test]
fn noeviction_refuses_a_write_that_would_grow_the_keyspace() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "noeviction");

    let refusal = write_until_refused(&mut client, "grow");
    assert!(
        refusal.starts_with("OOM command not allowed"),
        "unexpected refusal: {refusal:?}"
    );
    // Nothing was evicted to get there - that is what noeviction means.
    assert_eq!(info_number(&mut client, "evicted_keys"), 0);
}

#[test]
fn a_full_server_still_answers_reads_and_deletes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "noeviction");
    write_until_refused(&mut client, "full");

    // Reads are never refused, or a client could not find out what it
    // is holding.
    assert_eq!(client.call(&["GET", "full:0"]).text().len(), VALUE_BYTES);
    assert!(matches!(client.send("DBSIZE"), Value::Int(n) if n > 0));

    // Nor is anything that can only shrink the keyspace - otherwise the
    // one way out of the state would be closed.
    assert_eq!(client.call(&["DEL", "full:0"]), int(1));
    assert_eq!(client.call(&["EXPIRE", "full:1", "100"]), int(1));

    // And freeing room lets writes through again. How much has to go
    // is not fixed - the refusal came from the write that crossed the
    // limit, which says nothing about how far past it the server
    // already was - so this deletes until the write fits.
    let mut deleted = 1;
    loop {
        assert!(deleted < 100, "deleting keys never made room for a write");
        assert_eq!(client.call(&["DEL", &format!("full:{deleted}")]), int(1));
        deleted += 1;
        if fill(&mut client, "refilled") == ok() {
            break;
        }
    }
}

#[test]
fn allkeys_lru_evicts_instead_of_refusing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "allkeys-lru");

    // Twice as many values as the headroom can hold, so eviction has to
    // carry the second half.
    let writes = (HEADROOM / VALUE_BYTES) * 2;
    write_all(&mut client, "lru", writes);

    let evicted = info_number(&mut client, "evicted_keys");
    assert!(evicted > 0, "nothing was evicted");
    let held = match client.send("DBSIZE") {
        Value::Int(n) => n as usize,
        other => panic!("DBSIZE replied {other:?}"),
    };
    assert_eq!(
        held + evicted,
        writes,
        "every key is either held or evicted"
    );
}

#[test]
fn allkeys_lru_keeps_the_key_that_is_being_read() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.call(&["SET", "favourite", "v"]), ok());
    cap(&mut client, "allkeys-lru");

    // Read the favourite between every write, so it is never the least
    // recently used key in any sample.
    for i in 0..(HEADROOM / VALUE_BYTES) * 3 {
        assert_eq!(fill(&mut client, &format!("churn:{i}")), ok());
        assert_eq!(client.call(&["GET", "favourite"]).text(), "v");
    }

    assert!(info_number(&mut client, "evicted_keys") > 0);
    assert_eq!(client.call(&["EXISTS", "favourite"]), int(1));
}

#[test]
fn a_volatile_policy_leaves_keys_without_a_ttl_alone() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for i in 0..4 {
        assert_eq!(fill(&mut client, &format!("keep:{i}")), ok());
    }
    cap(&mut client, "volatile-lru");

    // Every new key carries a TTL, so only these are eligible.
    for i in 0..(HEADROOM / VALUE_BYTES) * 3 {
        let key = format!("drop:{i}");
        assert_eq!(
            client.call(&["SET", &key, &"x".repeat(VALUE_BYTES), "EX", "600"]),
            ok()
        );
    }

    assert!(info_number(&mut client, "evicted_keys") > 0);
    for i in 0..4 {
        assert_eq!(
            client.call(&["EXISTS", &format!("keep:{i}")]),
            int(1),
            "keep:{i} had no TTL and should have survived"
        );
    }
}

#[test]
fn a_volatile_policy_with_nothing_to_take_refuses_the_write() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "volatile-ttl");

    // No key carries an expiry, so volatile-ttl has no candidates and
    // behaves exactly as noeviction does.
    let refusal = write_until_refused(&mut client, "novolatile");
    assert!(
        refusal.starts_with("OOM"),
        "unexpected refusal: {refusal:?}"
    );
    assert_eq!(info_number(&mut client, "evicted_keys"), 0);
}

#[test]
fn clearing_the_limit_lets_the_writes_through_again() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "noeviction");
    write_until_refused(&mut client, "again");

    assert_eq!(client.call(&["CONFIG", "SET", "maxmemory", "0"]), ok());
    assert_eq!(fill(&mut client, "again:free"), ok());
}

#[test]
fn every_policy_is_settable_and_reported_back() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for policy in [
        "noeviction",
        "allkeys-lru",
        "allkeys-lfu",
        "allkeys-random",
        "volatile-lru",
        "volatile-lfu",
        "volatile-random",
        "volatile-ttl",
    ] {
        assert_eq!(
            client.call(&["CONFIG", "SET", "maxmemory-policy", policy]),
            ok()
        );
        assert_eq!(
            field(&client.send("INFO memory"), "maxmemory_policy"),
            policy
        );
    }
    // Spelling is not case-sensitive, and an unknown one is refused.
    assert_eq!(
        client.call(&["CONFIG", "SET", "maxmemory-policy", "ALLKEYS-LFU"]),
        ok()
    );
    assert!(matches!(
        client.call(&["CONFIG", "SET", "maxmemory-policy", "allkeys-mru"]),
        Value::Error(_)
    ));
}

#[test]
fn maxmemory_accepts_the_size_suffixes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // kb/mb/gb are powers of 1024; k/m/g are powers of a thousand.
    for (written, bytes) in [
        ("1024", "1024"),
        ("1kb", "1024"),
        ("1k", "1000"),
        ("8mb", "8388608"),
        ("2m", "2000000"),
        ("1gb", "1073741824"),
        ("0", "0"),
    ] {
        assert_eq!(client.call(&["CONFIG", "SET", "maxmemory", written]), ok());
        assert_eq!(
            client.call(&["CONFIG", "GET", "maxmemory"]).pairs(),
            vec![("maxmemory".to_string(), bytes.to_string())],
            "for {written}"
        );
    }
    for junk in ["1tb", "mb", "-1", "1.5mb"] {
        assert!(
            matches!(
                client.call(&["CONFIG", "SET", "maxmemory", junk]),
                Value::Error(_)
            ),
            "{junk} should be refused"
        );
    }
    // Back to unlimited, so the server does not evict during teardown.
    assert_eq!(client.call(&["CONFIG", "SET", "maxmemory", "0"]), ok());
}

#[test]
fn a_config_file_can_set_the_limit_and_the_policy() {
    let server = KlyroServer::with_config("maxmemory 64mb\nmaxmemory-policy allkeys-lfu\n");
    let mut client = server.connect();
    let info = client.send("INFO memory");
    assert_eq!(field(&info, "maxmemory"), (64 * 1024 * 1024).to_string());
    assert_eq!(field(&info, "maxmemory_human"), "64.00M");
    assert_eq!(field(&info, "maxmemory_policy"), "allkeys-lfu");
}

#[test]
fn an_eviction_aborts_a_transaction_watching_the_key() {
    let server = KlyroServer::new();
    let mut watcher = server.connect();
    let mut writer = server.connect();

    assert_eq!(watcher.call(&["SET", "watched", "v"]), ok());
    assert_eq!(watcher.call(&["WATCH", "watched"]), ok());
    assert_eq!(watcher.send("MULTI"), ok());
    assert_eq!(watcher.send("GET watched"), Value::Simple("QUEUED".into()));

    // A second connection fills the server until the watched key,
    // which nobody has touched since, is chosen as a victim.
    cap(&mut writer, "allkeys-lru");
    write_all(&mut writer, "pressure", (HEADROOM / VALUE_BYTES) * 4);
    assert_eq!(writer.call(&["EXISTS", "watched"]), int(0));

    assert_eq!(watcher.send("EXEC"), Value::NilArray);
}

#[test]
fn resetstat_clears_the_eviction_count() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    cap(&mut client, "allkeys-random");
    write_all(&mut client, "reset", (HEADROOM / VALUE_BYTES) * 2);
    assert!(info_number(&mut client, "evicted_keys") > 0);

    assert_eq!(client.send("CONFIG RESETSTAT"), ok());
    assert_eq!(info_number(&mut client, "evicted_keys"), 0);
}
