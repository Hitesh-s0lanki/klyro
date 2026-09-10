//! List commands added beyond push/pop/range: random access, in-place
//! edits, trimming, and the two-key moves.

mod common;

use common::{lines_before_terminator, KlyroServer};

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

fn seeded(server: &KlyroServer, key: &str, values: &str) -> common::KlyroClient {
    let mut client = server.connect();
    client.send(&format!("RPUSH {} {}", key, values));
    client
}

#[test]
fn lindex_reads_by_position() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b c");
    assert_eq!(client.send("LINDEX l 0"), "VALUE a\r\n");
    assert_eq!(client.send("LINDEX l 2"), "VALUE c\r\n");
    assert_eq!(client.send("LINDEX l -1"), "VALUE c\r\n");
    assert_eq!(client.send("LINDEX l 9"), "NOT_FOUND\r\n");
    assert_eq!(client.send("LINDEX missing 0"), "NOT_FOUND\r\n");
}

#[test]
fn lset_replaces_in_place() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b c");
    assert_eq!(client.send("LSET l 1 new value"), "OK\r\n");
    assert_eq!(client.send("LINDEX l 1"), "VALUE new value\r\n");
    assert_eq!(client.send("LSET l 9 x"), "NOT_FOUND\r\n");
    assert_eq!(client.send("LSET missing 0 x"), "NOT_FOUND\r\n");
}

#[test]
fn linsert_places_a_value_around_a_pivot() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a c");
    assert_eq!(client.send("LINSERT l BEFORE c b"), "LEN 3\r\n");
    assert_eq!(client.send("LINSERT l AFTER c d"), "LEN 4\r\n");
    let reply = client.send("LRANGE l 0 -1");
    assert_eq!(
        lines_before_terminator(&reply, "END"),
        vec!["a", "b", "c", "d"]
    );
    assert_eq!(client.send("LINSERT l BEFORE nope x"), "NOT_FOUND\r\n");
}

#[test]
fn lrem_honors_the_count_direction() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "x a x b x");
    assert_eq!(client.send("LREM l 2 x"), "REMOVED 2\r\n");
    let reply = client.send("LRANGE l 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["a", "b", "x"]);

    let mut client = seeded(&server, "m", "x a x");
    assert_eq!(client.send("LREM m 0 x"), "REMOVED 2\r\n");
    assert_eq!(client.send("LLEN m"), "LEN 1\r\n");
}

#[test]
fn lrem_emptying_the_list_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "x x");
    assert_eq!(client.send("LREM l 0 x"), "REMOVED 2\r\n");
    assert_eq!(client.send("EXISTS l"), "COUNT 0\r\n");
}

#[test]
fn ltrim_keeps_only_the_requested_window() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b c d e");
    assert_eq!(client.send("LTRIM l 1 3"), "OK\r\n");
    let reply = client.send("LRANGE l 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["b", "c", "d"]);
}

#[test]
fn ltrim_with_an_empty_range_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b");
    assert_eq!(client.send("LTRIM l 5 10"), "OK\r\n");
    assert_eq!(client.send("EXISTS l"), "COUNT 0\r\n");
}

#[test]
fn lpop_and_rpop_accept_a_count() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b c d");
    let reply = client.send("LPOP l 2");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["a", "b"]);
    let reply = client.send("RPOP l 5");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["d", "c"]);
    // A count on an empty list is an empty list, not NOT_FOUND.
    let reply = client.send("LPOP l 2");
    assert!(lines_before_terminator(&reply, "END").is_empty());
}

#[test]
fn lpop_without_a_count_keeps_its_single_value_reply() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b");
    assert_eq!(client.send("LPOP l"), "VALUE a\r\n");
    assert_eq!(client.send("RPOP l"), "VALUE b\r\n");
    assert_eq!(client.send("LPOP l"), "NOT_FOUND\r\n");
}

#[test]
fn pushx_only_extends_an_existing_list() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("LPUSHX l v"), "NOT_FOUND\r\n");
    assert_eq!(client.send("RPUSHX l v"), "NOT_FOUND\r\n");
    assert_eq!(client.send("EXISTS l"), "COUNT 0\r\n");

    client.send("RPUSH l a");
    assert_eq!(client.send("RPUSHX l b"), "LEN 2\r\n");
    assert_eq!(client.send("LPUSHX l z"), "LEN 3\r\n");
}

#[test]
fn rpoplpush_moves_between_lists() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", "1 2 3");
    assert_eq!(client.send("RPOPLPUSH src dst"), "VALUE 3\r\n");
    assert_eq!(client.send("RPOPLPUSH src dst"), "VALUE 2\r\n");
    let reply = client.send("LRANGE dst 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["2", "3"]);
    assert_eq!(client.send("LLEN src"), "LEN 1\r\n");
}

#[test]
fn rpoplpush_can_rotate_a_list_onto_itself() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "a b c");
    assert_eq!(client.send("RPOPLPUSH l l"), "VALUE c\r\n");
    let reply = client.send("LRANGE l 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["c", "a", "b"]);
}

#[test]
fn rotating_a_single_element_list_onto_itself_keeps_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", "only");
    assert_eq!(client.send("RPOPLPUSH l l"), "VALUE only\r\n");
    assert_eq!(client.send("LLEN l"), "LEN 1\r\n");
}

#[test]
fn lmove_chooses_both_ends() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", "1 2 3");
    assert_eq!(client.send("LMOVE src dst LEFT RIGHT"), "VALUE 1\r\n");
    assert_eq!(client.send("LMOVE src dst RIGHT RIGHT"), "VALUE 3\r\n");
    let reply = client.send("LRANGE dst 0 -1");
    assert_eq!(lines_before_terminator(&reply, "END"), vec!["1", "3"]);
    assert_eq!(client.send("LMOVE nope dst LEFT LEFT"), "NOT_FOUND\r\n");
    assert_eq!(
        client.send("LMOVE src dst SIDEWAYS LEFT"),
        "ERR usage: LMOVE source destination LEFT|RIGHT LEFT|RIGHT\r\n"
    );
}

#[test]
fn emptying_the_source_of_a_move_deletes_it() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", "only");
    client.send("RPOPLPUSH src dst");
    assert_eq!(client.send("EXISTS src"), "COUNT 0\r\n");
    assert_eq!(client.send("LLEN dst"), "LEN 1\r\n");
}

#[test]
fn new_list_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for cmd in [
        "LINDEX s 0",
        "LSET s 0 v",
        "LINSERT s BEFORE a b",
        "LREM s 0 v",
        "LTRIM s 0 1",
        "LPUSHX s v",
        "RPOPLPUSH s other",
    ] {
        assert_eq!(client.send(cmd), WRONGTYPE, "for {cmd}");
    }
}
