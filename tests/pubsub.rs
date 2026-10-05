//! SUBSCRIBE, PSUBSCRIBE, PUBLISH, and PUBSUB.

mod common;

use std::time::Duration;

use common::{bulk, int, KlyroServer, Value};

/// The three-element frame every subscribe confirmation and message
/// delivery takes.
fn frame(kind: &str, name: &str, tail: Value) -> Value {
    Value::Array(vec![bulk(kind), bulk(name), tail])
}

#[test]
fn subscribing_is_confirmed_once_per_channel() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert_eq!(
        client.call(&["SUBSCRIBE", "news"]),
        frame("subscribe", "news", int(1))
    );
    // One frame per channel named, with a running count of every
    // subscription held.
    client.send_only(&["SUBSCRIBE", "sport", "weather"]);
    assert_eq!(client.read(), frame("subscribe", "sport", int(2)));
    assert_eq!(client.read(), frame("subscribe", "weather", int(3)));
}

#[test]
fn a_published_message_reaches_its_subscribers() {
    let server = KlyroServer::new();
    let (mut listener, mut publisher) = (server.connect(), server.connect());

    listener.call(&["SUBSCRIBE", "news"]);
    assert_eq!(publisher.call(&["PUBLISH", "news", "hello world"]), int(1));
    assert_eq!(
        listener.read(),
        frame("message", "news", bulk("hello world"))
    );
}

#[test]
fn publishing_where_nobody_listens_reaches_nobody() {
    let server = KlyroServer::new();
    let mut publisher = server.connect();
    assert_eq!(publisher.call(&["PUBLISH", "news", "hi"]), int(0));
}

#[test]
fn a_message_does_not_reach_another_channel() {
    let server = KlyroServer::new();
    let (mut listener, mut publisher) = (server.connect(), server.connect());

    listener.call(&["SUBSCRIBE", "news"]);
    publisher.call(&["PUBLISH", "sport", "hi"]);
    assert!(listener.quiet_for(Duration::from_millis(200)));
}

#[test]
fn a_pattern_subscriber_is_told_which_pattern_matched() {
    let server = KlyroServer::new();
    let (mut listener, mut publisher) = (server.connect(), server.connect());

    assert_eq!(
        listener.call(&["PSUBSCRIBE", "news.*"]),
        frame("psubscribe", "news.*", int(1))
    );
    publisher.call(&["PUBLISH", "news.sport", "goal"]);
    assert_eq!(
        listener.read(),
        Value::Array(vec![
            bulk("pmessage"),
            bulk("news.*"),
            bulk("news.sport"),
            bulk("goal"),
        ])
    );
}

#[test]
fn a_client_subscribed_both_ways_receives_a_message_twice() {
    let server = KlyroServer::new();
    let (mut listener, mut publisher) = (server.connect(), server.connect());

    listener.call(&["SUBSCRIBE", "news"]);
    listener.call(&["PSUBSCRIBE", "ne*"]);
    assert_eq!(publisher.call(&["PUBLISH", "news", "hi"]), int(2));
    assert_eq!(listener.read(), frame("message", "news", bulk("hi")));
    assert_eq!(
        listener.read(),
        Value::Array(vec![
            bulk("pmessage"),
            bulk("ne*"),
            bulk("news"),
            bulk("hi")
        ])
    );
}

#[test]
fn unsubscribing_leaves_subscriber_mode() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.call(&["SUBSCRIBE", "news"]);
    assert_eq!(
        client.call(&["UNSUBSCRIBE", "news"]),
        frame("unsubscribe", "news", int(0))
    );
    // Out of subscriber mode, ordinary commands work again.
    assert_eq!(client.send("SET k v"), common::ok());
}

#[test]
fn unsubscribing_from_everything_needs_no_arguments() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send_only(&["SUBSCRIBE", "a", "b"]);
    client.read();
    client.read();

    client.send_only(&["UNSUBSCRIBE"]);
    let first = client.read();
    let second = client.read();
    assert_eq!(first.items()[0], bulk("unsubscribe"));
    assert_eq!(second.items()[2], int(0));
}

#[test]
fn unsubscribing_from_nothing_is_still_answered() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(
        client.call(&["UNSUBSCRIBE"]),
        Value::Array(vec![bulk("unsubscribe"), Value::Nil, int(0)])
    );
}

#[test]
fn a_resp2_subscriber_may_only_run_the_subscribe_commands() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.call(&["SUBSCRIBE", "news"]);
    let refused = client.send("GET k");
    assert!(refused.error().contains("only (P)SUBSCRIBE"));
    // PING is on the allowed list, so a client can still keep the
    // connection alive.
    assert_eq!(client.send("PING"), Value::Simple("PONG".into()));
}

#[test]
fn a_resp3_subscriber_can_still_run_ordinary_commands() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.call(&["HELLO", "3"]);
    client.call(&["SUBSCRIBE", "news"]);
    // RESP3 marks pushes, so there is nothing to keep apart by hand.
    assert_eq!(client.send("SET k v"), common::ok());
    assert_eq!(client.send("GET k"), bulk("v"));
}

#[test]
fn pubsub_reports_channels_subscribers_and_patterns() {
    let server = KlyroServer::new();
    let (mut listener, mut other) = (server.connect(), server.connect());

    listener.send_only(&["SUBSCRIBE", "news", "sport"]);
    listener.read();
    listener.read();
    listener.call(&["PSUBSCRIBE", "news.*"]);

    let mut watcher = server.connect();
    assert_eq!(watcher.send("PUBSUB CHANNELS").sorted(), ["news", "sport"]);
    assert_eq!(watcher.send("PUBSUB CHANNELS n*").list(), ["news"]);
    assert_eq!(
        watcher.send("PUBSUB NUMSUB news sport quiet").pairs(),
        [
            ("news".to_string(), "1".to_string()),
            ("quiet".to_string(), "0".to_string()),
            ("sport".to_string(), "1".to_string()),
        ]
    );
    assert_eq!(watcher.send("PUBSUB NUMPAT"), int(1));

    // A second client on the same pattern is one more subscription but
    // not one more pattern.
    other.call(&["PSUBSCRIBE", "news.*"]);
    assert_eq!(watcher.send("PUBSUB NUMPAT"), int(1));
}

#[test]
fn a_subscriber_that_disconnects_is_forgotten() {
    let server = KlyroServer::new();
    let mut listener = server.connect();
    listener.call(&["SUBSCRIBE", "news"]);
    drop(listener);

    let mut publisher = server.connect();
    // The dropped socket takes a moment to be noticed; PUBLISH is what
    // notices it.
    let mut attempts = 0;
    while publisher.call(&["PUBLISH", "news", "hi"]) != int(0) && attempts < 50 {
        std::thread::sleep(Duration::from_millis(20));
        attempts += 1;
    }
    assert_eq!(publisher.send("PUBSUB CHANNELS"), Value::Array(vec![]));
}

#[test]
fn subscribing_is_refused_inside_a_transaction() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.send("MULTI");
    assert!(client
        .call(&["SUBSCRIBE", "news"])
        .error()
        .contains("not allowed in transactions"));
    assert!(client.send("EXEC").error().starts_with("EXECABORT"));
}

#[test]
fn publishing_to_yourself_arrives_before_the_count() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    client.call(&["HELLO", "3"]);
    client.call(&["SUBSCRIBE", "news"]);
    client.send_only(&["PUBLISH", "news", "hi"]);
    assert_eq!(client.read(), frame("message", "news", bulk("hi")));
    assert_eq!(client.read(), int(1));
}
