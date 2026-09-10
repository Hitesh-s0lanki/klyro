//! The RESP wire protocol itself: reply types, request framing, inline
//! commands, pipelining, and protocol errors.

mod common;

use common::{bulk, int, nil, ok, KlyroServer, Value};

#[test]
fn each_command_answers_with_its_redis_reply_type() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    assert_eq!(client.send("PING"), Value::Simple("PONG".into()));
    assert_eq!(client.send("SET k v"), ok());
    assert_eq!(client.send("GET k"), bulk("v"));
    assert_eq!(client.send("GET missing"), nil());
    assert_eq!(client.send("STRLEN k"), int(1));
    assert_eq!(client.send("TYPE k"), Value::Simple("string".into()));
    assert_eq!(client.send("TYPE missing"), Value::Simple("none".into()));
    assert_eq!(client.send("KEYS *"), common::strings(&["k"]));
}

#[test]
fn values_may_contain_spaces_newlines_and_nul_bytes() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    // The single biggest thing RESP buys: a value is a length-prefixed
    // blob, so nothing in it can be mistaken for a delimiter.
    let awkward = b"a b\r\nc\0d\te".to_vec();
    assert_eq!(
        client.call_bytes(&[b"SET".to_vec(), b"k".to_vec(), awkward.clone()]),
        ok()
    );
    assert_eq!(client.send("GET k").bytes(), awkward);
    assert_eq!(client.send("STRLEN k"), int(awkward.len() as i64));
}

#[test]
fn keys_may_also_contain_arbitrary_bytes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    let key = b"awkward key\r\nwith\0bytes".to_vec();

    assert_eq!(
        client.call_bytes(&[b"SET".to_vec(), key.clone(), b"v".to_vec()]),
        ok()
    );
    assert_eq!(
        client.call_bytes(&[b"GET".to_vec(), key.clone()]),
        bulk("v")
    );
    assert_eq!(
        client.call_bytes(&[b"EXISTS".to_vec(), key.clone()]),
        int(1)
    );
    assert_eq!(client.send("KEYS *").items().len(), 1);
    assert_eq!(client.send("KEYS *").items()[0].bytes(), key);
}

#[test]
fn collection_members_may_contain_spaces() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    // The old line protocol could not express these at all.
    assert_eq!(
        client.call(&["RPUSH", "l", "two words", "three more words"]),
        int(2)
    );
    assert_eq!(
        client.send("LRANGE l 0 -1").list(),
        vec!["two words", "three more words"]
    );
    assert_eq!(client.call(&["SADD", "s", "a member"]), int(1));
    assert_eq!(client.call(&["SISMEMBER", "s", "a member"]), int(1));
    assert_eq!(client.call(&["ZADD", "z", "1", "a member"]), int(1));
    assert_eq!(client.call(&["ZSCORE", "z", "a member"]), bulk("1"));
    assert_eq!(client.call(&["HSET", "h", "a field", "a value"]), int(1));
    assert_eq!(client.call(&["HGET", "h", "a field"]), bulk("a value"));
}

#[test]
fn an_empty_bulk_string_is_not_nil() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.call(&["SET", "k", ""]), ok());
    assert_eq!(client.send("GET k"), Value::Bulk(Vec::new()));
    assert_eq!(client.send("STRLEN k"), int(0));
    assert_eq!(client.send("EXISTS k"), int(1));
}

#[test]
fn inline_commands_still_work_for_a_human_at_a_socket() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send_raw(b"PING\r\n"), Value::Simple("PONG".into()));
    assert_eq!(client.send_raw(b"SET k v\r\n"), ok());
    assert_eq!(client.send_raw(b"GET k\r\n"), bulk("v"));
}

#[test]
fn inline_quotes_carry_a_value_with_spaces() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    assert_eq!(client.send_raw(b"SET k \"hello world\"\r\n"), ok());
    assert_eq!(client.send_raw(b"GET k\r\n"), bulk("hello world"));
    assert_eq!(client.send_raw(b"SET k 'single quoted'\r\n"), ok());
    assert_eq!(client.send_raw(b"GET k\r\n"), bulk("single quoted"));
}

#[test]
fn a_bare_newline_is_ignored_rather_than_erroring() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // Nothing comes back for the blank line, so the PING's reply is the
    // next thing on the wire.
    assert_eq!(
        client.send_raw(b"\r\nPING\r\n"),
        Value::Simple("PONG".into())
    );
}

#[test]
fn pipelined_commands_answer_in_order() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    let mut request = Vec::new();
    for (key, value) in [("a", "1"), ("b", "2"), ("c", "3")] {
        request.extend_from_slice(
            format!("*3\r\n$3\r\nSET\r\n$1\r\n{key}\r\n$1\r\n{value}\r\n").as_bytes(),
        );
    }
    request.extend_from_slice(b"*2\r\n$3\r\nGET\r\n$1\r\nb\r\n");

    // One write, four replies, read back in the order sent.
    assert_eq!(client.send_raw(&request), ok());
    assert_eq!(client.send_raw(b""), ok());
    assert_eq!(client.send_raw(b""), ok());
    assert_eq!(client.send_raw(b""), bulk("2"));
}

#[test]
fn a_request_split_across_writes_is_reassembled() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k hello");

    // Send one command a few bytes at a time; the server must wait for
    // the whole frame rather than erroring on a partial one.
    let request = b"*2\r\n$3\r\nGET\r\n$1\r\nk\r\n";
    for chunk in request[..request.len() - 4].chunks(3) {
        client.send_no_reply(chunk);
    }
    assert_eq!(
        client.send_raw(&request[request.len() - 4..]),
        bulk("hello")
    );
}

#[test]
fn errors_come_back_as_error_replies() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("SET k v");

    assert_eq!(client.send("LPUSH k v").error(), common::WRONGTYPE);
    assert!(client
        .send("BOGUSCOMMAND a b")
        .error()
        .starts_with("ERR unknown command 'BOGUSCOMMAND'"));
    assert_eq!(
        client.send("GET").error(),
        "ERR wrong number of arguments for 'get' command"
    );
    assert_eq!(
        client.send("INCR k").error(),
        "ERR value is not an integer or out of range"
    );
}

#[test]
fn a_malformed_frame_reports_a_protocol_error_and_closes() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    // An array whose elements are not bulk strings.
    let reply = client.send_raw(b"*1\r\n+GET\r\n");
    assert!(
        reply.error().starts_with("ERR Protocol error"),
        "got {reply:?}"
    );
    assert!(client.closed(), "the connection should have been closed");
}

#[test]
fn an_oversized_bulk_string_is_a_protocol_error() {
    let server = KlyroServer::new();
    let mut client = server.connect();
    client.send("CONFIG SET proto-max-bulk-len 16");

    let mut other = server.connect();
    let reply = other.send_raw(b"*2\r\n$3\r\nGET\r\n$1000\r\n");
    assert!(
        reply.error().starts_with("ERR Protocol error"),
        "got {reply:?}"
    );
}

#[test]
fn a_large_reply_is_delivered_whole_rather_than_truncated() {
    let server = KlyroServer::new();
    let mut client = server.connect();

    // The old line protocol silently capped a reply at 64 KiB, so a
    // large SMEMBERS came back partial and looked complete.
    for i in 0..5000 {
        client.call(&["SADD", "big", &format!("member-{i:06}")]);
    }
    assert_eq!(client.send("SCARD big"), int(5000));
    let members = client.send("SMEMBERS big");
    assert_eq!(members.items().len(), 5000);

    let value = "x".repeat(200_000);
    assert_eq!(client.call(&["SET", "big-string", &value]), ok());
    assert_eq!(client.send("GET big-string").text().len(), 200_000);
}
