//! MULTI, EXEC, DISCARD, WATCH, UNWATCH, and RESET.

mod common;

use common::{array, bulk, int, nil, ok, KlyroServer, Value};

#[test]
fn commands_are_queued_and_run_only_by_exec() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert_eq!(client.send("MULTI"), ok());
    assert_eq!(client.send("SET k v"), Value::Simple("QUEUED".into()));
    assert_eq!(client.send("INCR n"), Value::Simple("QUEUED".into()));
    // Nothing has run yet.
    assert_eq!(client.send("GET k"), Value::Simple("QUEUED".into()));

    assert_eq!(client.send("EXEC"), array(vec![ok(), int(1), bulk("v")]));
    assert_eq!(client.send("GET k"), bulk("v"));
}

#[test]
fn a_transaction_that_queued_nothing_replies_with_an_empty_array() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert_eq!(client.send("EXEC"), array(vec![]));
}

#[test]
fn exec_and_discard_without_multi_are_errors() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert!(client.send("EXEC").error().contains("without MULTI"));
    assert!(client.send("DISCARD").error().contains("without MULTI"));
}

#[test]
fn multi_cannot_be_nested() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert!(client.send("MULTI").error().contains("can not be nested"));
}

#[test]
fn discard_throws_the_queue_away() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("MULTI");
    client.send("SET k v");
    assert_eq!(client.send("DISCARD"), ok());
    assert_eq!(client.send("GET k"), nil());
    // The connection is out of the transaction, not still in it.
    assert_eq!(client.send("SET k direct"), ok());
}

#[test]
fn a_command_that_cannot_be_queued_aborts_the_whole_transaction() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("MULTI");
    client.send("SET k v");
    assert!(client
        .send("NOSUCHCOMMAND")
        .error()
        .contains("unknown command"));
    assert!(client.send("EXEC").error().starts_with("EXECABORT"));
    // The one command that did queue must not have run.
    assert_eq!(client.send("GET k"), nil());
}

#[test]
fn a_runtime_error_leaves_the_rest_of_the_queue_running() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("LPUSH list a");

    client.send("MULTI");
    client.send("SET first 1");
    client.send("INCR list"); // wrong type, but only at run time
    client.send("SET last 1");
    let results = client.send("EXEC");

    assert_eq!(results.items().len(), 3);
    assert!(results.items()[1].is_error());
    // No rollback: both writes stand.
    assert_eq!(client.send("GET first"), bulk("1"));
    assert_eq!(client.send("GET last"), bulk("1"));
}

#[test]
fn watch_lets_a_transaction_through_when_nothing_changed() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("SET k 1");
    client.send("WATCH k");
    client.send("MULTI");
    client.send("SET k 2");
    assert_eq!(client.send("EXEC"), array(vec![ok()]));
    assert_eq!(client.send("GET k"), bulk("2"));
}

#[test]
fn watch_aborts_a_transaction_when_another_client_writes() {
    let server = KlyroServer::new();
    let (mut client, mut other) = (server.connect(), server.connect());

    client.send("SET k 1");
    client.send("WATCH k");
    other.send("SET k changed");

    client.send("MULTI");
    client.send("SET k 2");
    // A null array, distinct from the empty array a transaction that
    // ran and produced nothing returns.
    assert_eq!(client.send("EXEC"), Value::NilArray);
    assert_eq!(client.send("GET k"), bulk("changed"));
}

#[test]
fn a_read_by_another_client_does_not_abort_a_transaction() {
    let server = KlyroServer::new();
    let (mut client, mut other) = (server.connect(), server.connect());

    client.send("RPUSH q a");
    client.send("WATCH q");
    other.send("LRANGE q 0 -1");
    other.send("LLEN q");

    client.send("MULTI");
    client.send("RPUSH q b");
    assert_eq!(client.send("EXEC"), array(vec![int(2)]));
}

#[test]
fn a_flush_aborts_every_watching_transaction() {
    let server = KlyroServer::new();
    let (mut client, mut other) = (server.connect(), server.connect());

    client.send("SET k 1");
    client.send("WATCH k");
    other.send("FLUSHALL");

    client.send("MULTI");
    client.send("SET k 2");
    assert_eq!(client.send("EXEC"), Value::NilArray);
}

#[test]
fn unwatch_drops_the_guard() {
    let server = KlyroServer::new();
    let (mut client, mut other) = (server.connect(), server.connect());

    client.send("SET k 1");
    client.send("WATCH k");
    other.send("SET k changed");
    assert_eq!(client.send("UNWATCH"), ok());

    client.send("MULTI");
    client.send("SET k 2");
    assert_eq!(client.send("EXEC"), array(vec![ok()]));
}

#[test]
fn exec_leaves_no_watches_behind() {
    let server = KlyroServer::new();
    let (mut client, mut other) = (server.connect(), server.connect());

    client.send("WATCH k");
    client.send("MULTI");
    assert_eq!(client.send("EXEC"), array(vec![]));

    // The watch from the first transaction must not reach into the
    // second one.
    other.send("SET k changed");
    client.send("MULTI");
    client.send("SET other 1");
    assert_eq!(client.send("EXEC"), array(vec![ok()]));
}

#[test]
fn watch_inside_multi_is_refused() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("MULTI");
    assert!(client
        .send("WATCH k")
        .error()
        .contains("WATCH inside MULTI"));
}

#[test]
fn reset_returns_the_connection_to_a_clean_state() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("WATCH k");
    client.send("MULTI");
    client.send("SET k v");
    assert_eq!(client.send("RESET"), Value::Simple("RESET".into()));

    assert!(client.send("EXEC").error().contains("without MULTI"));
    assert_eq!(client.send("GET k"), nil());
}

#[test]
fn a_watching_client_that_leaves_frees_its_watch() {
    let server = KlyroServer::new();
    let mut watcher = server.connect();
    watcher.send("WATCH k");
    drop(watcher);

    // Nothing is left holding the key, so an unrelated transaction on
    // it still runs.
    let mut client = server.connect();
    client.send("MULTI");
    client.send("SET k v");
    assert_eq!(client.send("EXEC"), array(vec![ok()]));
}
