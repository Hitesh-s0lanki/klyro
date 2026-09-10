//! String commands, including SET's option flags.

mod common;

use common::{bulk, int, nil, ok, KlyroServer, Value, WRONGTYPE};

#[test]
fn set_and_get() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.call(&["SET", "k", "hello world"]), ok());
    assert_eq!(client.send("GET k"), bulk("hello world"));
    assert_eq!(client.send("GET missing"), nil());
    assert_eq!(client.send("SET k second"), ok());
    assert_eq!(client.send("GET k"), bulk("second"));
}

#[test]
fn set_clears_a_previous_expiry_unless_told_otherwise() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v EX 100");
    assert!(client.send("TTL k").integer() > 0);
    client.send("SET k v2");
    assert_eq!(client.send("TTL k"), int(-1));
}

#[test]
fn set_nx_only_writes_when_the_key_is_absent() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET lock mine NX"), ok());
    // A refused conditional SET replies nil, as Redis does.
    assert_eq!(client.send("SET lock yours NX"), nil());
    assert_eq!(client.send("GET lock"), bulk("mine"));
}

#[test]
fn set_xx_only_writes_when_the_key_is_present() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET k v XX"), nil());
    client.send("SET k first");
    assert_eq!(client.send("SET k second XX"), ok());
    assert_eq!(client.send("GET k"), bulk("second"));
}

#[test]
fn set_nx_with_ex_is_atomic_lock_acquisition() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET lock token NX EX 30"), ok());
    assert!((29..=30).contains(&client.send("TTL lock").integer()));
    assert_eq!(client.send("SET lock other NX EX 30"), nil());
}

#[test]
fn set_takes_px_exat_and_pxat() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v PX 60000");
    assert!((59..=60).contains(&client.send("TTL k").integer()));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    client.send(&format!("SET k v EXAT {}", now + 100));
    assert!((95..=100).contains(&client.send("TTL k").integer()));
    client.send(&format!("SET k v PXAT {}", (now + 200) * 1000));
    assert!((195..=200).contains(&client.send("TTL k").integer()));
}

#[test]
fn set_keepttl_preserves_an_existing_expiry() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v EX 100");
    client.send("SET k updated KEEPTTL");
    assert_eq!(client.send("GET k"), bulk("updated"));
    assert!(client.send("TTL k").integer() > 0);
}

#[test]
fn set_get_returns_the_previous_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET k first GET"), nil());
    assert_eq!(client.send("SET k second GET"), bulk("first"));
    assert_eq!(client.send("GET k"), bulk("second"));
    // Even when NX declines the write, GET still reports what is there.
    assert_eq!(client.send("SET k third NX GET"), bulk("second"));
    assert_eq!(client.send("GET k"), bulk("second"));
}

#[test]
fn set_rejects_contradictory_and_invalid_options() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET k v NX XX").error(), "ERR syntax error");
    assert_eq!(client.send("SET k v BOGUS").error(), "ERR syntax error");
    assert_eq!(
        client.send("SET k v EX 0").error(),
        "ERR invalid expire time in 'set' command"
    );
    assert_eq!(client.send("SET k v EX").error(), "ERR syntax error");
}

#[test]
fn setnx_setex_and_psetex() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SETNX k first"), int(1));
    assert_eq!(client.send("SETNX k second"), int(0));
    assert_eq!(client.send("GET k"), bulk("first"));

    assert_eq!(client.call(&["SETEX", "e", "60", "value here"]), ok());
    assert_eq!(client.send("GET e"), bulk("value here"));
    assert!((59..=60).contains(&client.send("TTL e").integer()));

    assert_eq!(client.send("PSETEX p 60000 v"), ok());
    assert!((59..=60).contains(&client.send("TTL p").integer()));
    assert_eq!(
        client.send("SETEX bad 0 v").error(),
        "ERR invalid expire time in 'setex' command"
    );
}

#[test]
fn getset_getdel_and_getex() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("GETSET k first"), nil());
    assert_eq!(client.send("GETSET k second"), bulk("first"));

    assert_eq!(client.send("GETDEL k"), bulk("second"));
    assert_eq!(client.send("EXISTS k"), int(0));
    assert_eq!(client.send("GETDEL k"), nil());

    client.send("SET g v");
    assert_eq!(client.send("GETEX g EX 100"), bulk("v"));
    assert!(client.send("TTL g").integer() > 0);
    assert_eq!(client.send("GETEX g"), bulk("v"));
    assert!(client.send("TTL g").integer() > 0);
    assert_eq!(client.send("GETEX g PERSIST"), bulk("v"));
    assert_eq!(client.send("TTL g"), int(-1));
    assert_eq!(client.send("GETEX missing EX 10"), nil());
}

#[test]
fn mget_and_mset() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MSET a 1 b 2 c 3"), ok());
    assert_eq!(
        client.send("MGET a b c missing"),
        Value::Array(vec![bulk("1"), bulk("2"), bulk("3"), nil()])
    );
    // A wrong-type key reads as nil rather than failing the whole call.
    client.send("RPUSH l x");
    assert_eq!(
        client.send("MGET a l"),
        Value::Array(vec![bulk("1"), nil()])
    );
    assert_eq!(
        client.send("MSET a 1 b").error(),
        "ERR wrong number of arguments for 'mset' command"
    );
}

#[test]
fn msetnx_is_all_or_nothing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MSETNX a 1 b 2"), int(1));
    // `b` already exists, so neither key is written.
    assert_eq!(client.send("MSETNX b 9 c 3"), int(0));
    assert_eq!(client.send("GET b"), bulk("2"));
    assert_eq!(client.send("EXISTS c"), int(0));
}

#[test]
fn incr_and_decr() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("INCR n"), int(1));
    assert_eq!(client.send("INCR n"), int(2));
    assert_eq!(client.send("DECR n"), int(1));
    assert_eq!(client.send("INCRBY n 10"), int(11));
    assert_eq!(client.send("DECRBY n 20"), int(-9));
}

#[test]
fn incr_rejects_a_non_integer_and_overflow() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s hello");
    assert_eq!(
        client.send("INCR s").error(),
        "ERR value is not an integer or out of range"
    );
    assert_eq!(
        client.send("INCRBY n notanumber").error(),
        "ERR value is not an integer or out of range"
    );
    client.send(&format!("SET big {}", i64::MAX));
    assert_eq!(
        client.send("INCR big").error(),
        "ERR increment or decrement would overflow"
    );
}

#[test]
fn incrbyfloat_accumulates_fractions() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("INCRBYFLOAT f 1.5"), bulk("1.5"));
    assert_eq!(client.send("INCRBYFLOAT f 2.25"), bulk("3.75"));
    assert_eq!(client.send("INCRBYFLOAT f -3.75"), bulk("0"));
    client.send("SET s hello");
    assert_eq!(
        client.send("INCRBYFLOAT s 1").error(),
        "ERR value is not a valid float"
    );
}

#[test]
fn counters_preserve_an_existing_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    for (setup, command) in [
        ("SET n 1", "INCR n"),
        ("SET n 1", "INCRBY n 5"),
        ("SET n 1", "APPEND n 2"),
        ("SET n 1", "SETRANGE n 0 9"),
    ] {
        client.send(setup);
        client.send("EXPIRE n 100");
        client.send(command);
        assert!(client.send("TTL n").integer() > 0, "after {command}");
    }
}

#[test]
fn append_and_strlen() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("STRLEN missing"), int(0));
    assert_eq!(client.send("APPEND k hello"), int(5));
    assert_eq!(client.call(&["APPEND", "k", " world"]), int(11));
    assert_eq!(client.send("GET k"), bulk("hello world"));
    assert_eq!(client.send("STRLEN k"), int(11));
}

#[test]
fn getrange_handles_negative_and_out_of_range_indices() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k HelloWorld");
    assert_eq!(client.send("GETRANGE k 0 4"), bulk("Hello"));
    assert_eq!(client.send("GETRANGE k -5 -1"), bulk("World"));
    assert_eq!(client.send("GETRANGE k 0 -1"), bulk("HelloWorld"));
    assert_eq!(client.send("GETRANGE k 100 200"), bulk(""));
    assert_eq!(client.send("GETRANGE missing 0 -1"), bulk(""));
    // SUBSTR is the old name for the same command.
    assert_eq!(client.send("SUBSTR k 0 4"), bulk("Hello"));
}

#[test]
fn setrange_writes_in_place_and_pads_with_nul_bytes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.call(&["SET", "k", "Hello World"]);
    assert_eq!(client.send("SETRANGE k 6 Redis"), int(11));
    assert_eq!(client.send("GET k"), bulk("Hello Redis"));

    // Redis pads a gap with NUL bytes, which the byte-oriented store
    // can now represent exactly.
    assert_eq!(client.send("SETRANGE pad 3 abc"), int(6));
    assert_eq!(client.send("GET pad").bytes(), b"\0\0\0abc".to_vec());
    // An empty patch is a no-op that still reports the length.
    assert_eq!(client.call(&["SETRANGE", "pad", "0", ""]), int(6));
}

#[test]
fn string_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH l x");
    for command in [
        "GET l",
        "STRLEN l",
        "INCR l",
        "INCRBYFLOAT l 1",
        "APPEND l x",
        "GETRANGE l 0 -1",
        "SETRANGE l 0 x",
        "GETSET l v",
        "GETDEL l",
        "GETEX l",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}
