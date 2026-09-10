//! The blocking pops, and the LMPOP/ZMPOP family they share their
//! argument shape with.

mod common;

use std::time::{Duration, Instant};

use common::{array, bulk, int, nil, KlyroServer, Value};

/// Long enough that a parked command is unmistakably parked, short
/// enough not to slow the suite down.
const SETTLE: Duration = Duration::from_millis(150);

#[test]
fn blpop_answers_at_once_when_the_list_has_a_value() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("RPUSH q first second");
    assert_eq!(
        client.send("BLPOP q 0"),
        array(vec![bulk("q"), bulk("first")])
    );
    assert_eq!(
        client.send("BRPOP q 0"),
        array(vec![bulk("q"), bulk("second")])
    );
}

#[test]
fn blpop_waits_for_a_push_from_another_client() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(SETTLE), "BLPOP answered an empty list");

    pusher.send("RPUSH q value");
    assert_eq!(waiter.read(), array(vec![bulk("q"), bulk("value")]));
    // The push was consumed by the waiter, not left behind.
    assert_eq!(pusher.send("LLEN q"), int(0));
}

#[test]
fn blpop_gives_up_at_its_timeout() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    let started = Instant::now();
    assert_eq!(client.send("BLPOP q 0.2"), Value::NilArray);
    assert!(started.elapsed() >= Duration::from_millis(180));
    // The connection is usable again straight away.
    assert_eq!(client.send("PING"), Value::Simple("PONG".into()));
}

#[test]
fn a_waiting_client_takes_the_first_key_that_gets_a_value() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BLPOP", "first", "second", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    pusher.send("RPUSH second value");
    assert_eq!(waiter.read(), array(vec![bulk("second"), bulk("value")]));
}

#[test]
fn keys_are_tried_in_the_order_they_were_given() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("RPUSH second later");
    client.send("RPUSH first sooner");
    // Both have values, so the priority order decides.
    assert_eq!(
        client.send("BLPOP first second 0"),
        array(vec![bulk("first"), bulk("sooner")])
    );
}

#[test]
fn the_longest_waiting_client_is_served_first() {
    let server = KlyroServer::new();
    let (mut first, mut second) = (server.connect(), server.connect());
    let mut pusher = server.connect();

    first.send_only(&["BLPOP", "q", "0"]);
    assert!(first.quiet_for(SETTLE));
    second.send_only(&["BLPOP", "q", "0"]);
    assert!(second.quiet_for(SETTLE));

    pusher.send("RPUSH q one");
    assert_eq!(first.read(), array(vec![bulk("q"), bulk("one")]));
    assert!(second.quiet_for(SETTLE), "one push served two waiters");

    pusher.send("RPUSH q two");
    assert_eq!(second.read(), array(vec![bulk("q"), bulk("two")]));
}

#[test]
fn a_wrong_type_is_reported_rather_than_waited_on() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("SET k string");
    assert!(client.send("BLPOP k 0").error().starts_with("WRONGTYPE"));
}

#[test]
fn a_negative_or_unparsable_timeout_is_refused() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert!(client.send("BLPOP q -1").error().contains("negative"));
    assert!(client.send("BLPOP q soon").error().contains("not a float"));
}

#[test]
fn blmove_waits_and_then_moves() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BLMOVE", "src", "dst", "LEFT", "RIGHT", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    pusher.send("RPUSH src value");
    assert_eq!(waiter.read(), bulk("value"));
    assert_eq!(pusher.send("LRANGE dst 0 -1").list(), ["value"]);
    assert_eq!(pusher.send("EXISTS src"), int(0));
}

#[test]
fn blmove_times_out_with_a_null_rather_than_a_null_array() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("BLMOVE src dst LEFT RIGHT 0.2"), nil());
}

#[test]
fn brpoplpush_is_blmove_right_to_left() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BRPOPLPUSH", "src", "dst", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    pusher.send("RPUSH src a b");
    assert_eq!(waiter.read(), bulk("b"));
    assert_eq!(pusher.send("LRANGE src 0 -1").list(), ["a"]);
}

#[test]
fn a_move_can_wake_a_client_waiting_on_its_destination() {
    let server = KlyroServer::new();
    let (mut mover, mut waiter) = (server.connect(), server.connect());
    let mut pusher = server.connect();

    waiter.send_only(&["BLPOP", "dst", "0"]);
    assert!(waiter.quiet_for(SETTLE));
    mover.send_only(&["BLMOVE", "src", "dst", "LEFT", "RIGHT", "0"]);
    assert!(mover.quiet_for(SETTLE));

    // One push feeds the move, whose own write feeds the pop.
    pusher.send("RPUSH src value");
    assert_eq!(mover.read(), bulk("value"));
    assert_eq!(waiter.read(), array(vec![bulk("dst"), bulk("value")]));
}

#[test]
fn bzpopmin_and_bzpopmax_wait_on_a_sorted_set() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BZPOPMIN", "z", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    pusher.send("ZADD z 1 low 2 high");
    assert_eq!(
        waiter.read(),
        array(vec![bulk("z"), bulk("low"), bulk("1")])
    );
    assert_eq!(
        pusher.send("BZPOPMAX z 0"),
        array(vec![bulk("z"), bulk("high"), bulk("2")])
    );
}

#[test]
fn lmpop_takes_a_batch_from_the_first_non_empty_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("RPUSH q a b c");
    assert_eq!(
        client.send("LMPOP 2 empty q LEFT COUNT 2"),
        array(vec![bulk("q"), array(vec![bulk("a"), bulk("b")])])
    );
    assert_eq!(
        client.send("LMPOP 1 q RIGHT"),
        array(vec![bulk("q"), array(vec![bulk("c")])])
    );
    assert_eq!(client.send("LMPOP 1 q LEFT"), Value::NilArray);
}

#[test]
fn zmpop_pairs_each_member_with_its_score() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("ZADD z 1 one 2 two 3 three");
    assert_eq!(
        client.send("ZMPOP 1 z MIN COUNT 2"),
        array(vec![
            bulk("z"),
            array(vec![
                array(vec![bulk("one"), bulk("1")]),
                array(vec![bulk("two"), bulk("2")]),
            ])
        ])
    );
    assert_eq!(client.send("ZMPOP 1 nothing MAX"), Value::NilArray);
}

#[test]
fn blmpop_and_bzmpop_wait_for_their_keys() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BLMPOP", "0", "2", "a", "b", "LEFT", "COUNT", "10"]);
    assert!(waiter.quiet_for(SETTLE));
    pusher.send("RPUSH b one two");
    assert_eq!(
        waiter.read(),
        array(vec![bulk("b"), array(vec![bulk("one"), bulk("two")])])
    );

    waiter.send_only(&["BZMPOP", "0", "1", "z", "MAX"]);
    assert!(waiter.quiet_for(SETTLE));
    pusher.send("ZADD z 5 top");
    assert_eq!(
        waiter.read(),
        array(vec![
            bulk("z"),
            array(vec![array(vec![bulk("top"), bulk("5")])])
        ])
    );
}

#[test]
fn a_multi_key_pop_needs_a_sane_numkeys() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert!(client
        .send("LMPOP 0 q LEFT")
        .error()
        .contains("greater than 0"));
    assert!(client
        .send("LMPOP 2 only LEFT")
        .error()
        .contains("wrong number"));
    assert!(client.send("LMPOP 1 q").error().contains("wrong number"));
    assert!(client.send("LMPOP 1 q SIDEWAYS").error().contains("syntax"));
    assert!(client
        .send("LMPOP 1 q LEFT COUNT 0")
        .error()
        .contains("greater than 0"));
}

#[test]
fn a_blocking_command_inside_a_transaction_answers_instead_of_waiting() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("MULTI");
    client.send("BLPOP q 0");
    client.send("SET after 1");
    // A transaction cannot wait: nothing could feed it while it holds
    // the server.
    assert_eq!(
        client.send("EXEC"),
        array(vec![Value::NilArray, common::ok()])
    );
}

#[test]
fn a_parked_client_that_disconnects_leaves_nothing_behind() {
    let server = KlyroServer::new();
    let mut waiter = server.connect();
    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(SETTLE));
    drop(waiter);

    let mut pusher = server.connect();
    pusher.send("RPUSH q value");
    // With the waiter gone, the value stays in the list.
    let mut attempts = 0;
    while pusher.send("LLEN q") != int(1) && attempts < 50 {
        std::thread::sleep(Duration::from_millis(20));
        attempts += 1;
    }
    assert_eq!(pusher.send("LRANGE q 0 -1").list(), ["value"]);
}

#[test]
fn commands_pipelined_behind_a_blocking_one_run_once_it_is_answered() {
    let server = KlyroServer::new();
    let (mut waiter, mut pusher) = (server.connect(), server.connect());

    waiter.send_only(&["BLPOP", "q", "0"]);
    waiter.send_only(&["SET", "after", "1"]);
    assert!(waiter.quiet_for(SETTLE));

    pusher.send("RPUSH q value");
    assert_eq!(waiter.read(), array(vec![bulk("q"), bulk("value")]));
    assert_eq!(waiter.read(), common::ok());
    assert_eq!(pusher.send("GET after"), bulk("1"));
}

#[test]
fn a_blocked_client_does_not_hold_up_the_server() {
    let server = KlyroServer::new();
    let (mut waiter, mut other) = (server.connect(), server.connect());

    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(SETTLE));
    // Everything else carries on as normal.
    assert_eq!(other.send("SET k v"), common::ok());
    assert_eq!(other.send("GET k"), bulk("v"));
}

#[test]
fn a_string_written_to_a_waited_on_key_does_not_wake_the_waiter() {
    let server = KlyroServer::new();
    let (mut waiter, mut other) = (server.connect(), server.connect());

    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    // Nothing blocks on a string, so this is not the value it waits
    // for - it keeps waiting rather than being woken to an error.
    other.send("SET q string");
    assert!(waiter.quiet_for(SETTLE));

    other.send("DEL q");
    other.send("RPUSH q value");
    assert_eq!(waiter.read(), array(vec![bulk("q"), bulk("value")]));
}

#[test]
fn waiting_does_not_inflate_the_command_count() {
    let server = KlyroServer::new();
    let (mut waiter, mut other) = (server.connect(), server.connect());

    let before = command_count(&mut other);
    waiter.send_only(&["BLPOP", "q", "0"]);
    assert!(waiter.quiet_for(SETTLE));

    for value in ["a", "b", "c"] {
        other.call(&["SADD", "unrelated", value]);
    }
    other.send("RPUSH q value");
    waiter.read();

    // BLPOP, three SADDs, the RPUSH, and the closing INFO. The RPUSH
    // re-ran the BLPOP to serve it, and that retry is the same command
    // finishing rather than another one.
    assert_eq!(command_count(&mut other) - before, 6);
}

/// INFO's count of commands processed since startup.
fn command_count(client: &mut common::KlyroClient) -> i64 {
    let body = client.send("INFO stats").text();
    body.lines()
        .find_map(|line| {
            line.trim_end()
                .strip_prefix("total_commands_processed:")
                .and_then(|n| n.parse().ok())
        })
        .expect("no total_commands_processed line")
}
