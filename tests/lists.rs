//! List commands.

mod common;

use common::{bulk, int, nil, ok, KlyroServer, Value, WRONGTYPE};

fn seeded(server: &KlyroServer, key: &str, values: &[&str]) -> common::KlyroClient {
    let mut client = server.connect();
    let mut args = vec!["RPUSH", key];
    args.extend_from_slice(values);
    client.call(&args);
    client
}

#[test]
fn push_pop_and_len() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("RPUSH l a"), int(1));
    assert_eq!(client.send("RPUSH l b c"), int(3));
    assert_eq!(client.send("LPUSH l z"), int(4));
    assert_eq!(client.send("LLEN l"), int(4));
    assert_eq!(
        client.send("LRANGE l 0 -1").list(),
        vec!["z", "a", "b", "c"]
    );
    assert_eq!(client.send("LPOP l"), bulk("z"));
    assert_eq!(client.send("RPOP l"), bulk("c"));
    assert_eq!(client.send("LPOP missing"), nil());
}

#[test]
fn push_applies_values_left_to_right() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // Each LPUSH value goes to the head in turn, so the list ends up
    // reversed relative to the arguments.
    assert_eq!(client.send("LPUSH l a b c"), int(3));
    assert_eq!(client.send("LRANGE l 0 -1").list(), vec!["c", "b", "a"]);
    assert_eq!(client.send("RPUSH r a b c"), int(3));
    assert_eq!(client.send("LRANGE r 0 -1").list(), vec!["a", "b", "c"]);
}

#[test]
fn push_requires_at_least_one_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.send("RPUSH l").error(),
        "ERR wrong number of arguments for 'rpush' command"
    );
}

#[test]
fn lrange_handles_negative_and_out_of_range_indices() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c", "d"]);
    assert_eq!(
        client.send("LRANGE l 0 -1").list(),
        vec!["a", "b", "c", "d"]
    );
    assert_eq!(client.send("LRANGE l 1 2").list(), vec!["b", "c"]);
    assert_eq!(client.send("LRANGE l -2 -1").list(), vec!["c", "d"]);
    assert_eq!(client.send("LRANGE l 10 20"), Value::Array(vec![]));
}

#[test]
fn lindex_reads_by_position() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c"]);
    assert_eq!(client.send("LINDEX l 0"), bulk("a"));
    assert_eq!(client.send("LINDEX l -1"), bulk("c"));
    assert_eq!(client.send("LINDEX l 9"), nil());
    assert_eq!(client.send("LINDEX missing 0"), nil());
}

#[test]
fn lset_replaces_in_place() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c"]);
    assert_eq!(client.call(&["LSET", "l", "1", "new value"]), ok());
    assert_eq!(client.send("LINDEX l 1"), bulk("new value"));
    assert_eq!(client.send("LSET l 9 x").error(), "ERR index out of range");
    assert_eq!(client.send("LSET missing 0 x").error(), "ERR no such key");
}

#[test]
fn linsert_places_a_value_around_a_pivot() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "c"]);
    assert_eq!(client.send("LINSERT l BEFORE c b"), int(3));
    assert_eq!(client.send("LINSERT l AFTER c d"), int(4));
    assert_eq!(
        client.send("LRANGE l 0 -1").list(),
        vec!["a", "b", "c", "d"]
    );
    // -1 for a missing pivot, 0 for a missing key.
    assert_eq!(client.send("LINSERT l BEFORE nope x"), int(-1));
    assert_eq!(client.send("LINSERT missing BEFORE a x"), int(0));
    assert_eq!(
        client.send("LINSERT l SIDEWAYS c x").error(),
        "ERR syntax error"
    );
}

#[test]
fn lrem_honours_the_count_direction() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["x", "a", "x", "b", "x"]);
    assert_eq!(client.send("LREM l 2 x"), int(2));
    assert_eq!(client.send("LRANGE l 0 -1").list(), vec!["a", "b", "x"]);

    let mut client = seeded(&server, "m", &["x", "a", "x", "b", "x"]);
    assert_eq!(client.send("LREM m -1 x"), int(1));
    assert_eq!(
        client.send("LRANGE m 0 -1").list(),
        vec!["x", "a", "x", "b"]
    );

    let mut client = seeded(&server, "n", &["x", "a", "x"]);
    assert_eq!(client.send("LREM n 0 x"), int(2));
    assert_eq!(client.send("LRANGE n 0 -1").list(), vec!["a"]);
}

#[test]
fn lrem_emptying_the_list_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["x", "x"]);
    assert_eq!(client.send("LREM l 0 x"), int(2));
    assert_eq!(client.send("EXISTS l"), int(0));
}

#[test]
fn ltrim_keeps_only_the_requested_window() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c", "d", "e"]);
    assert_eq!(client.send("LTRIM l 1 3"), ok());
    assert_eq!(client.send("LRANGE l 0 -1").list(), vec!["b", "c", "d"]);
}

#[test]
fn ltrim_with_an_empty_range_deletes_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b"]);
    assert_eq!(client.send("LTRIM l 5 10"), ok());
    assert_eq!(client.send("EXISTS l"), int(0));
}

#[test]
fn pop_accepts_a_count() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c", "d"]);
    assert_eq!(client.send("LPOP l 2").list(), vec!["a", "b"]);
    assert_eq!(client.send("RPOP l 5").list(), vec!["d", "c"]);
    // An empty list with a count replies with a null array, not an
    // empty one, which is how Redis distinguishes "no such key".
    assert_eq!(client.send("LPOP l 2"), Value::NilArray);
}

#[test]
fn pushx_only_extends_an_existing_list() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("LPUSHX l v"), int(0));
    assert_eq!(client.send("RPUSHX l v"), int(0));
    assert_eq!(client.send("EXISTS l"), int(0));

    client.send("RPUSH l a");
    assert_eq!(client.send("RPUSHX l b"), int(2));
    assert_eq!(client.send("LPUSHX l z"), int(3));
}

#[test]
fn rpoplpush_moves_between_lists() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", &["1", "2", "3"]);
    assert_eq!(client.send("RPOPLPUSH src dst"), bulk("3"));
    assert_eq!(client.send("RPOPLPUSH src dst"), bulk("2"));
    assert_eq!(client.send("LRANGE dst 0 -1").list(), vec!["2", "3"]);
    assert_eq!(client.send("LLEN src"), int(1));
    assert_eq!(client.send("RPOPLPUSH missing dst"), nil());
}

#[test]
fn rpoplpush_can_rotate_a_list_onto_itself() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["a", "b", "c"]);
    assert_eq!(client.send("RPOPLPUSH l l"), bulk("c"));
    assert_eq!(client.send("LRANGE l 0 -1").list(), vec!["c", "a", "b"]);
}

#[test]
fn rotating_a_single_element_list_onto_itself_keeps_the_key() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "l", &["only"]);
    assert_eq!(client.send("RPOPLPUSH l l"), bulk("only"));
    assert_eq!(client.send("LLEN l"), int(1));
}

#[test]
fn lmove_chooses_both_ends() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", &["1", "2", "3"]);
    assert_eq!(client.send("LMOVE src dst LEFT RIGHT"), bulk("1"));
    assert_eq!(client.send("LMOVE src dst RIGHT RIGHT"), bulk("3"));
    assert_eq!(client.send("LRANGE dst 0 -1").list(), vec!["1", "3"]);
    assert_eq!(
        client.send("LMOVE src dst SIDEWAYS LEFT").error(),
        "ERR syntax error"
    );
}

#[test]
fn emptying_the_source_of_a_move_deletes_it() {
    let server = KlyroServer::new();
    let mut client = seeded(&server, "src", &["only"]);
    client.send("RPOPLPUSH src dst");
    assert_eq!(client.send("EXISTS src"), int(0));
    assert_eq!(client.send("LLEN dst"), int(1));
}

#[test]
fn list_commands_reject_wrong_types() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET s v");
    for command in [
        "LPUSH s x",
        "LPOP s",
        "LLEN s",
        "LRANGE s 0 -1",
        "LINDEX s 0",
        "LSET s 0 v",
        "LINSERT s BEFORE a b",
        "LREM s 0 v",
        "LTRIM s 0 1",
        "LPUSHX s v",
        "RPOPLPUSH s other",
    ] {
        assert_eq!(client.send(command).error(), WRONGTYPE, "for {command}");
    }
}
