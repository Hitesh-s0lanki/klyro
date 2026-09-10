//! MULTI, EXEC, DISCARD, WATCH, UNWATCH and RESET.

mod common;

use common::{bulk, int, ok, KlyroServer, Value, WRONGTYPE};

fn queued() -> Value {
    Value::Simple("QUEUED".into())
}

#[test]
fn a_transaction_queues_then_runs_in_order() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("MULTI"), ok());
    assert_eq!(client.send("SET k 1"), queued());
    assert_eq!(client.send("INCR k"), queued());
    assert_eq!(client.send("GET k"), queued());
    assert_eq!(
        client.send("EXEC"),
        Value::Array(vec![ok(), int(2), bulk("2")])
    );
}

#[test]
fn nothing_is_applied_until_exec() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("MULTI");
    client.send("SET k queued-value");
    // A second client cannot see the queued write.
    assert_eq!(other.send("EXISTS k"), int(0));
    client.send("EXEC");
    assert_eq!(other.send("GET k"), bulk("queued-value"));
}

#[test]
fn an_empty_transaction_is_an_empty_array() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert_eq!(client.send("EXEC"), Value::Array(vec![]));
}

#[test]
fn discard_throws_the_queue_away() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k original");
    client.send("MULTI");
    assert_eq!(client.send("SET k changed"), queued());
    assert_eq!(client.send("DISCARD"), ok());
    assert_eq!(client.send("GET k"), bulk("original"));
    // The transaction is over, so the next command runs normally.
    assert_eq!(client.send("PING"), Value::Simple("PONG".into()));
}

#[test]
fn multi_exec_and_discard_report_being_used_out_of_order() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send("EXEC").error(), "ERR EXEC without MULTI");
    assert_eq!(client.send("DISCARD").error(), "ERR DISCARD without MULTI");
    client.send("MULTI");
    assert_eq!(
        client.send("MULTI").error(),
        "ERR MULTI calls can not be nested"
    );
}

#[test]
fn an_unknown_command_breaks_the_transaction() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert_eq!(client.send("SET k v"), queued());
    assert!(client
        .send("NOSUCHCOMMAND x")
        .error()
        .starts_with("ERR unknown command"));
    assert_eq!(
        client.send("EXEC").error(),
        "EXECABORT Transaction discarded because of previous errors."
    );
    // The queued SET never ran.
    assert_eq!(client.send("EXISTS k"), int(0));
}

#[test]
fn a_broken_transaction_can_be_discarded_and_retried() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    client.send("NOSUCHCOMMAND");
    assert_eq!(client.send("DISCARD"), ok());
    client.send("MULTI");
    assert_eq!(client.send("SET k v"), queued());
    assert_eq!(client.send("EXEC"), Value::Array(vec![ok()]));
}

#[test]
fn a_runtime_error_does_not_stop_the_rest_of_the_transaction() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET str v");
    client.send("MULTI");
    client.send("SET before 1");
    client.send("LPUSH str x");
    client.send("SET after 1");

    let replies = client.send("EXEC");
    let items = replies.items();
    assert_eq!(items.len(), 3);
    assert_eq!(items[0], ok());
    assert_eq!(items[1].error(), WRONGTYPE);
    assert_eq!(items[2], ok());
    // There is no rollback: both writes stuck.
    assert_eq!(client.send("GET before"), bulk("1"));
    assert_eq!(client.send("GET after"), bulk("1"));
}

#[test]
fn watch_lets_an_untouched_transaction_through() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET w start");
    assert_eq!(client.send("WATCH w"), ok());
    client.send("MULTI");
    client.send("SET w mine");
    assert_eq!(client.send("EXEC"), Value::Array(vec![ok()]));
    assert_eq!(client.send("GET w"), bulk("mine"));
}

#[test]
fn watch_aborts_when_another_client_writes_the_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("SET w start");
    client.send("WATCH w");
    other.send("SET w stolen");
    client.send("MULTI");
    client.send("SET w mine");
    // A null array, which clients surface as "retry".
    assert_eq!(client.send("EXEC"), Value::NilArray);
    assert_eq!(client.send("GET w"), bulk("stolen"));
}

#[test]
fn watch_notices_a_delete_and_an_expiry() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("SET w v");
    client.send("WATCH w");
    other.send("DEL w");
    client.send("MULTI");
    client.send("SET w mine");
    assert_eq!(client.send("EXEC"), Value::NilArray);

    client.send("SET e v");
    client.send("WATCH e");
    other.send("EXPIRE e 100");
    client.send("MULTI");
    client.send("SET e mine");
    assert_eq!(client.send("EXEC"), Value::NilArray);
}

#[test]
fn watch_notices_a_change_inside_a_collection() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    // The list keeps three elements, so the key is neither created nor
    // deleted - only its contents move.
    client.send("RPUSH l a b c");
    client.send("WATCH l");
    other.send("LSET l 0 changed");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(client.send("EXEC"), Value::NilArray);
}

#[test]
fn reading_a_watched_key_does_not_abort() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("RPUSH l a b c");
    client.send("HSET h f v");
    client.send("WATCH l h");
    // A pile of reads from another client must leave the watch intact.
    other.send("LRANGE l 0 -1");
    other.send("LLEN l");
    other.send("HGETALL h");
    other.send("HGET h f");
    other.send("EXISTS l");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(
        client.send("EXEC"),
        Value::Array(vec![Value::Simple("PONG".into())])
    );
}

#[test]
fn watching_an_unrelated_key_does_not_abort() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("WATCH watched");
    other.send("SET unwatched anything");
    client.send("MULTI");
    client.send("SET k v");
    assert_eq!(client.send("EXEC"), Value::Array(vec![ok()]));
}

#[test]
fn a_client_may_write_its_own_watched_key() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("WATCH w");
    // Redis aborts on any modification, including the watcher's own.
    client.send("SET w mine");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(client.send("EXEC"), Value::NilArray);
}

#[test]
fn unwatch_clears_every_watch() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("WATCH a b");
    assert_eq!(client.send("UNWATCH"), ok());
    other.send("SET a changed");
    other.send("SET b changed");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(
        client.send("EXEC"),
        Value::Array(vec![Value::Simple("PONG".into())])
    );
}

#[test]
fn exec_and_discard_both_clear_the_watches() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    for finish in ["EXEC", "DISCARD"] {
        client.send("WATCH w");
        client.send("MULTI");
        client.send(finish);
        // The watch is gone, so this write cannot affect the next one.
        other.send("SET w changed");
        client.send("MULTI");
        client.send("PING");
        assert_eq!(
            client.send("EXEC"),
            Value::Array(vec![Value::Simple("PONG".into())]),
            "after {finish}"
        );
    }
}

#[test]
fn watch_is_refused_inside_a_transaction() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert_eq!(
        client.send("WATCH k").error(),
        "ERR WATCH inside MULTI is not allowed"
    );
}

#[test]
fn a_watch_belongs_to_one_connection() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("WATCH w");
    // The other connection never watched anything, so its transaction
    // runs even though the key moved.
    client.send("SET w changed");
    other.send("MULTI");
    other.send("SET w theirs");
    assert_eq!(other.send("EXEC"), Value::Array(vec![ok()]));
}

#[test]
fn a_closed_connection_releases_its_watches() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let watched_keys = |c: &mut common::KlyroClient| -> String {
        c.send("INFO clients")
            .text()
            .lines()
            .find_map(|l| {
                l.trim_end()
                    .strip_prefix("watched_keys:")
                    .map(str::to_string)
            })
            .expect("a watched_keys line")
    };

    {
        let mut temporary = server.connect();
        temporary.send("WATCH a b c");
        assert_eq!(watched_keys(&mut client), "3");
        temporary.send("QUIT");
    }
    // Give the server a moment to notice the close.
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert_eq!(watched_keys(&mut client), "0");
}

#[test]
fn reset_clears_the_transaction_and_the_watches() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("WATCH w");
    client.send("MULTI");
    client.send("SET k v");
    assert_eq!(client.send("RESET"), Value::Simple("RESET".into()));
    // No transaction is open any more.
    assert_eq!(client.send("EXEC").error(), "ERR EXEC without MULTI");
    assert_eq!(client.send("EXISTS k"), int(0));

    other.send("SET w changed");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(
        client.send("EXEC"),
        Value::Array(vec![Value::Simple("PONG".into())])
    );
}

#[test]
fn watch_notices_a_write_to_a_memory_index() {
    // The memory type reaches the store through its own accessor, so it
    // reports changes explicitly rather than through `write_*`. This
    // pins that it still moves the watch stamp.
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("MEM.CREATE ns MODE SEARCH");
    client.send("WATCH ns");
    other.call(&["MEM.ADD", "ns", "ID", "d1", "TEXT", "hello world"]);
    client.send("MULTI");
    client.send("PING");
    assert_eq!(client.send("EXEC"), Value::NilArray);
}

#[test]
fn reading_a_memory_index_does_not_break_a_watch() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let mut other = server.connect();

    client.send("MEM.CREATE ns MODE SEARCH");
    client.call(&["MEM.ADD", "ns", "ID", "d1", "TEXT", "hello world"]);
    client.send("WATCH ns");
    other.send("MEM.GET ns d1");
    other.send("MEM.CARD ns");
    client.send("MULTI");
    client.send("PING");
    assert_eq!(
        client.send("EXEC"),
        Value::Array(vec![Value::Simple("PONG".into())])
    );
}

#[test]
fn a_memory_command_can_be_queued_in_a_transaction() {
    // MEM.* is routed by prefix rather than listed, so the queue-time
    // "is this a command" check has to know about the prefix too.
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MEM.CREATE ns MODE SEARCH");
    client.send("MULTI");
    assert_eq!(
        client.call(&["MEM.ADD", "ns", "ID", "d1", "TEXT", "hello"]),
        queued()
    );
    assert_eq!(client.send("EXEC").items().len(), 1);
    assert_eq!(client.send("MEM.CARD ns"), int(1));
}

#[test]
fn transaction_control_commands_are_never_queued() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    // Each of these acts immediately rather than replying QUEUED.
    assert!(client.send("MULTI").is_error());
    assert_eq!(client.send("DISCARD"), ok());
}

#[test]
fn a_transaction_survives_a_wrong_arity_command_at_exec_time() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    // Arity is not checked at queue time, so this reaches EXEC and
    // reports there. Redis rejects it at queue time instead.
    assert_eq!(client.send("GET"), queued());
    let replies = client.send("EXEC");
    assert_eq!(replies.items().len(), 1);
    assert!(replies.items()[0]
        .error()
        .contains("wrong number of arguments"));
}
