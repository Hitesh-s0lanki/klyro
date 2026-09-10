//! String commands added on top of the original SET/GET pair: the SET
//! option flags, the SETNX/SETEX family, multi-key access, and the
//! INCRBY/STRLEN ergonomics.

mod common;

use common::{lines_before_terminator, KlyroServer};

#[test]
fn set_nx_only_writes_when_the_key_is_absent() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET lock mine NX"), "OK\r\n");
    assert_eq!(client.send("SET lock yours NX"), "NOT_SET\r\n");
    assert_eq!(client.send("GET lock"), "VALUE mine\r\n");
}

#[test]
fn set_xx_only_writes_when_the_key_is_present() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET k v XX"), "NOT_SET\r\n");
    client.send("SET k first");
    assert_eq!(client.send("SET k second XX"), "OK\r\n");
    assert_eq!(client.send("GET k"), "VALUE second\r\n");
}

#[test]
fn set_nx_with_ex_is_atomic_lock_acquisition() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SET lock token NX EX 30"), "OK\r\n");
    let ttl = client.send("TTL lock");
    assert!(ttl == "TTL 30\r\n" || ttl == "TTL 29\r\n", "got {ttl:?}");
    // A second holder is turned away while the lease is live.
    assert_eq!(client.send("SET lock other NX EX 30"), "NOT_SET\r\n");
}

#[test]
fn set_px_takes_milliseconds() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v PX 60000");
    let ttl = client.send("TTL k");
    assert!(ttl == "TTL 60\r\n" || ttl == "TTL 59\r\n", "got {ttl:?}");
}

#[test]
fn set_keepttl_preserves_an_existing_expiry() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v EX 100");
    client.send("SET k updated KEEPTTL");
    assert_eq!(client.send("GET k"), "VALUE updated\r\n");
    assert!(client.send("TTL k").starts_with("TTL 9"));
    // A plain SET still clears it, as it always has.
    client.send("SET k plain");
    assert_eq!(client.send("TTL k"), "TTL -1\r\n");
}

#[test]
fn set_values_keep_spaces_and_a_lone_flag_word_stays_a_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k hello big world");
    assert_eq!(client.send("GET k"), "VALUE hello big world\r\n");
    // There is always at least one token left for the value, so a value
    // that is only the word NX is stored literally rather than parsed
    // as a flag.
    client.send("SET flagish NX");
    assert_eq!(client.send("GET flagish"), "VALUE NX\r\n");
}

#[test]
fn set_rejects_contradictory_and_invalid_options() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("SET k v NX XX"),
        "ERR NX and XX are mutually exclusive\r\n"
    );
    assert_eq!(
        client.send("SET k v KEEPTTL EX 10"),
        "ERR KEEPTTL cannot be combined with EX or PX\r\n"
    );
    assert_eq!(client.send("SET k v EX 0"), "ERR invalid expire time\r\n");
}

#[test]
fn setnx_writes_only_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SETNX k first"), "OK\r\n");
    assert_eq!(client.send("SETNX k second"), "NOT_SET\r\n");
    assert_eq!(client.send("GET k"), "VALUE first\r\n");
}

#[test]
fn setex_and_psetex_set_a_value_with_its_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("SETEX k 60 value here"), "OK\r\n");
    assert_eq!(client.send("GET k"), "VALUE value here\r\n");
    assert!(client.send("TTL k").starts_with("TTL 5"));

    assert_eq!(client.send("PSETEX p 60000 v"), "OK\r\n");
    assert!(client.send("TTL p").starts_with("TTL 5"));
    assert_eq!(client.send("SETEX bad 0 v"), "ERR invalid expire time\r\n");
}

#[test]
fn getset_returns_the_previous_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("GETSET k first"), "NOT_FOUND\r\n");
    assert_eq!(client.send("GETSET k second"), "VALUE first\r\n");
    assert_eq!(client.send("GET k"), "VALUE second\r\n");
}

#[test]
fn getdel_reads_and_removes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");
    assert_eq!(client.send("GETDEL k"), "VALUE v\r\n");
    assert_eq!(client.send("EXISTS k"), "COUNT 0\r\n");
    assert_eq!(client.send("GETDEL k"), "NOT_FOUND\r\n");
}

#[test]
fn mset_and_mget_move_several_keys_at_once() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MSET a 1 b 2 c 3"), "OK\r\n");
    let reply = client.send("MGET a b c missing");
    let lines = lines_before_terminator(&reply, "END");
    assert_eq!(lines, vec!["VALUE 1", "VALUE 2", "VALUE 3", "NOT_FOUND"]);
}

#[test]
fn mget_reads_a_wrong_type_key_as_missing() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    client.send("RPUSH l x");
    let reply = client.send("MGET s l");
    let lines = lines_before_terminator(&reply, "END");
    assert_eq!(lines, vec!["VALUE v", "NOT_FOUND"]);
}

#[test]
fn mset_rejects_a_dangling_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("MSET a 1 b"),
        "ERR usage: MSET key value [key value ...]\r\n"
    );
}

#[test]
fn incrby_and_decrby_apply_an_amount() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("INCRBY n 10"), "VALUE 10\r\n");
    assert_eq!(client.send("INCRBY n 5"), "VALUE 15\r\n");
    assert_eq!(client.send("DECRBY n 20"), "VALUE -5\r\n");
    assert_eq!(
        client.send("INCRBY n notanumber"),
        "ERR usage: INCRBY key increment\r\n"
    );
}

#[test]
fn incrby_refuses_a_non_integer_value_and_overflow() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s hello");
    assert_eq!(client.send("INCRBY s 1"), "ERR value is not an integer\r\n");
    client.send(&format!("SET big {}", i64::MAX));
    assert_eq!(
        client.send("INCRBY big 1"),
        "ERR increment or decrement would overflow\r\n"
    );
}

#[test]
fn incrbyfloat_accumulates_fractions() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("INCRBYFLOAT f 1.5"), "VALUE 1.5\r\n");
    assert_eq!(client.send("INCRBYFLOAT f 2.25"), "VALUE 3.75\r\n");
    assert_eq!(client.send("INCRBYFLOAT f -3.75"), "VALUE 0\r\n");
    client.send("SET s hello");
    assert_eq!(
        client.send("INCRBYFLOAT s 1"),
        "ERR value is not a float\r\n"
    );
}

#[test]
fn incrby_preserves_an_existing_ttl() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET n 1");
    client.send("EXPIRE n 100");
    client.send("INCRBY n 5");
    assert!(client.send("TTL n").starts_with("TTL 9"));
}

#[test]
fn strlen_measures_the_stored_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("STRLEN missing"), "LEN 0\r\n");
    client.send("SET k hello world");
    assert_eq!(client.send("STRLEN k"), "LEN 11\r\n");
}

#[test]
fn new_string_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("RPUSH l x");
    let wrongtype = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";
    for cmd in [
        "STRLEN l",
        "INCRBY l 1",
        "INCRBYFLOAT l 1",
        "GETSET l v",
        "GETDEL l",
        "GETEX l",
    ] {
        assert_eq!(client.send(cmd), wrongtype, "for {cmd}");
    }
}
