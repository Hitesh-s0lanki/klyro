//! String/List/Hash/Set/Zset ops, WRONGTYPE, empty-collection deletes.
//! Ported from the old test_types.py.

mod common;

use common::KlyroServer;
use std::collections::HashSet;

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

fn server_and_client() -> (KlyroServer, common::KlyroClient) {
    let server = KlyroServer::new();
    let client = server.connect();
    (server, client)
}

// --- String ---

#[test]
fn string_set_get() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("SET k hello world"), "OK\r\n");
    assert_eq!(c.send("GET k"), "VALUE hello world\r\n");
}

#[test]
fn string_get_missing() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("GET missing"), "NOT_FOUND\r\n");
}

#[test]
fn string_set_overwrites() {
    let (_server, mut c) = server_and_client();
    c.send("SET k first");
    c.send("SET k second");
    assert_eq!(c.send("GET k"), "VALUE second\r\n");
}

#[test]
fn string_set_clears_previous_expiry() {
    let (_server, mut c) = server_and_client();
    c.send("SET k v");
    c.send("EXPIRE k 100");
    c.send("SET k v2");
    assert_eq!(c.send("TTL k"), "TTL -1\r\n");
}

#[test]
fn string_incr_decr_on_missing_key_starts_at_zero() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("INCR k"), "VALUE 1\r\n");
    assert_eq!(c.send("DECR k2"), "VALUE -1\r\n");
}

#[test]
fn string_incr_decr_on_existing_value() {
    let (_server, mut c) = server_and_client();
    c.send("SET k 10");
    assert_eq!(c.send("INCR k"), "VALUE 11\r\n");
    assert_eq!(c.send("DECR k"), "VALUE 10\r\n");
}

#[test]
fn string_incr_on_non_integer_value_is_rejected() {
    let (_server, mut c) = server_and_client();
    c.send("SET k notanumber");
    assert_eq!(c.send("INCR k"), "ERR value is not an integer\r\n");
}

#[test]
fn string_incr_and_decr_preserve_ttl() {
    let (_server, mut c) = server_and_client();
    c.send("SET k 5");
    c.send("EXPIRE k 200");
    c.send("INCR k");
    let resp = c.send("TTL k");
    assert!(
        resp == "TTL 200\r\n" || resp == "TTL 199\r\n",
        "got {resp:?}"
    );
}

#[test]
fn string_append_to_missing_key_creates_it() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("APPEND k hello"), "LEN 5\r\n");
    assert_eq!(c.send("GET k"), "VALUE hello\r\n");
}

#[test]
fn string_append_extends_existing_value() {
    let (_server, mut c) = server_and_client();
    c.send("SET k Hello");
    assert_eq!(c.send("APPEND k ,World"), "LEN 11\r\n");
    assert_eq!(c.send("GET k"), "VALUE Hello,World\r\n");
}

#[test]
fn string_append_preserves_ttl() {
    let (_server, mut c) = server_and_client();
    c.send("SET k hi");
    c.send("EXPIRE k 200");
    c.send("APPEND k there");
    let resp = c.send("TTL k");
    assert!(
        resp == "TTL 200\r\n" || resp == "TTL 199\r\n",
        "got {resp:?}"
    );
}

#[test]
fn string_getrange_positive_and_negative_indices() {
    let (_server, mut c) = server_and_client();
    c.send("SET k HelloWorld");
    assert_eq!(c.send("GETRANGE k 0 4"), "VALUE Hello\r\n");
    assert_eq!(c.send("GETRANGE k -5 -1"), "VALUE World\r\n");
    assert_eq!(c.send("GETRANGE k 0 -1"), "VALUE HelloWorld\r\n");
}

#[test]
fn string_getrange_out_of_bounds_is_empty() {
    let (_server, mut c) = server_and_client();
    c.send("SET k short");
    assert_eq!(c.send("GETRANGE k 100 200"), "VALUE \r\n");
}

#[test]
fn string_getrange_on_missing_key_is_empty() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("GETRANGE missing 0 -1"), "VALUE \r\n");
}

#[test]
fn string_setrange_overwrites_in_place() {
    let (_server, mut c) = server_and_client();
    c.send("SET k HelloWorld");
    assert_eq!(c.send("SETRANGE k 5 XXXXX"), "LEN 10\r\n");
    assert_eq!(c.send("GET k"), "VALUE HelloXXXXX\r\n");
}

#[test]
fn string_setrange_extends_past_current_end() {
    let (_server, mut c) = server_and_client();
    c.send("SET k Hi");
    assert_eq!(c.send("SETRANGE k 5 end"), "LEN 8\r\n");
    assert_eq!(c.send("GET k"), "VALUE Hi   end\r\n");
}

#[test]
fn string_setrange_on_missing_key_pads_with_spaces() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("SETRANGE k 3 end"), "LEN 6\r\n");
    assert_eq!(c.send("GET k"), "VALUE    end\r\n");
}

#[test]
fn string_setrange_preserves_ttl() {
    let (_server, mut c) = server_and_client();
    c.send("SET k hello");
    c.send("EXPIRE k 200");
    c.send("SETRANGE k 0 world");
    let resp = c.send("TTL k");
    assert!(
        resp == "TTL 200\r\n" || resp == "TTL 199\r\n",
        "got {resp:?}"
    );
}

#[test]
fn string_wrongtype_for_new_numeric_commands() {
    let (_server, mut c) = server_and_client();
    c.send("LPUSH k v");
    assert_eq!(c.send("INCR k"), WRONGTYPE);
    assert_eq!(c.send("APPEND k x"), WRONGTYPE);
    assert_eq!(c.send("GETRANGE k 0 -1"), WRONGTYPE);
    assert_eq!(c.send("SETRANGE k 0 x"), WRONGTYPE);
}

#[test]
fn string_large_value_round_trips_through_get() {
    // Exercises the reply path for values longer than a typical
    // single-recv buffer.
    let (_server, mut c) = server_and_client();
    let big_value = "x".repeat(5000);
    c.send(&format!("SET k {big_value}"));
    assert_eq!(c.send("GET k"), format!("VALUE {big_value}\r\n"));
}

// --- List ---

#[test]
fn list_push_pop_len() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("LPUSH k b"), "LEN 1\r\n");
    assert_eq!(c.send("LPUSH k a"), "LEN 2\r\n");
    assert_eq!(c.send("RPUSH k c"), "LEN 3\r\n");
    assert_eq!(c.send("LLEN k"), "LEN 3\r\n");
    assert_eq!(c.send("TYPE k"), "LIST\r\n");
}

#[test]
fn list_lrange_full_and_partial() {
    let (_server, mut c) = server_and_client();
    for v in ["a", "b", "c", "d"] {
        c.send(&format!("RPUSH k {v}"));
    }
    assert_eq!(c.send("LRANGE k 0 -1"), "a\r\nb\r\nc\r\nd\r\nEND\r\n");
    assert_eq!(c.send("LRANGE k 1 2"), "b\r\nc\r\nEND\r\n");
    assert_eq!(c.send("LRANGE k -2 -1"), "c\r\nd\r\nEND\r\n");
}

#[test]
fn list_pop_missing_key() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("LPOP missing"), "NOT_FOUND\r\n");
}

#[test]
fn list_emptied_list_deletes_key() {
    let (_server, mut c) = server_and_client();
    c.send("RPUSH k only");
    assert_eq!(c.send("LPOP k"), "VALUE only\r\n");
    assert_eq!(c.send("TYPE k"), "NONE\r\n");
    assert_eq!(c.send("LPOP k"), "NOT_FOUND\r\n");
}

// --- Hash ---

#[test]
fn hash_set_get_del() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("HSET k name Alice"), "OK\r\n");
    assert_eq!(c.send("HGET k name"), "VALUE Alice\r\n");
    assert_eq!(c.send("HGET k nofield"), "NOT_FOUND\r\n");
    assert_eq!(c.send("TYPE k"), "HASH\r\n");
}

#[test]
fn hash_hgetall() {
    let (_server, mut c) = server_and_client();
    c.send("HSET k a 1");
    c.send("HSET k b 2");
    let resp = c.send("HGETALL k");
    let pairs: Vec<&str> = resp.trim_end_matches("\r\n").split("\r\n").collect();
    let pairs = &pairs[..pairs.len() - 1]; // drop END
    let mut got = HashSet::new();
    for chunk in pairs.chunks(2) {
        got.insert((chunk[0], chunk[1]));
    }
    assert_eq!(got, HashSet::from([("a", "1"), ("b", "2")]));
}

#[test]
fn hash_emptied_hash_deletes_key() {
    let (_server, mut c) = server_and_client();
    c.send("HSET k only field");
    assert_eq!(c.send("HDEL k only"), "OK\r\n");
    assert_eq!(c.send("TYPE k"), "NONE\r\n");
    assert_eq!(c.send("HDEL k only"), "NOT_FOUND\r\n");
}

// --- Set ---

#[test]
fn set_add_ismember_card() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("SADD k x"), "ADDED 1\r\n");
    assert_eq!(c.send("SADD k x"), "ADDED 0\r\n");
    assert_eq!(c.send("SISMEMBER k x"), "TRUE\r\n");
    assert_eq!(c.send("SISMEMBER k y"), "FALSE\r\n");
    assert_eq!(c.send("SCARD k"), "LEN 1\r\n");
    assert_eq!(c.send("TYPE k"), "SET\r\n");
}

#[test]
fn set_smembers() {
    let (_server, mut c) = server_and_client();
    c.send("SADD k x");
    c.send("SADD k y");
    let resp = c.send("SMEMBERS k");
    let members: HashSet<&str> = common::lines_before_terminator(&resp, "END")
        .into_iter()
        .collect();
    assert_eq!(members, HashSet::from(["x", "y"]));
}

#[test]
fn set_emptied_set_deletes_key() {
    let (_server, mut c) = server_and_client();
    c.send("SADD k only");
    assert_eq!(c.send("SREM k only"), "OK\r\n");
    assert_eq!(c.send("TYPE k"), "NONE\r\n");
    assert_eq!(c.send("SREM k only"), "NOT_FOUND\r\n");
}

// --- Zset ---

#[test]
fn zset_add_score_card() {
    let (_server, mut c) = server_and_client();
    assert_eq!(c.send("ZADD k 100 alice"), "ADDED 1\r\n");
    assert_eq!(c.send("ZSCORE k alice"), "VALUE 100\r\n");
    assert_eq!(c.send("ZSCORE k nobody"), "NOT_FOUND\r\n");
    assert_eq!(c.send("ZCARD k"), "LEN 1\r\n");
    assert_eq!(c.send("TYPE k"), "ZSET\r\n");
}

#[test]
fn zset_zrange_ascending_by_score() {
    let (_server, mut c) = server_and_client();
    c.send("ZADD k 100 alice");
    c.send("ZADD k 50 bob");
    c.send("ZADD k 75 carol");
    assert_eq!(
        c.send("ZRANGE k 0 -1"),
        "bob 50\r\ncarol 75\r\nalice 100\r\nEND\r\n"
    );
}

#[test]
fn zset_zadd_repositions_existing_member() {
    let (_server, mut c) = server_and_client();
    c.send("ZADD k 100 alice");
    assert_eq!(c.send("ZADD k 5 alice"), "ADDED 0\r\n");
    assert_eq!(c.send("ZSCORE k alice"), "VALUE 5\r\n");
}

#[test]
fn zset_emptied_zset_deletes_key() {
    let (_server, mut c) = server_and_client();
    c.send("ZADD k 1 only");
    assert_eq!(c.send("ZREM k only"), "OK\r\n");
    assert_eq!(c.send("TYPE k"), "NONE\r\n");
    assert_eq!(c.send("ZREM k only"), "NOT_FOUND\r\n");
}

// --- WrongType ---

#[test]
fn wrongtype_list_op_on_string_key() {
    let (_server, mut c) = server_and_client();
    c.send("SET k v");
    assert_eq!(c.send("LPUSH k x"), WRONGTYPE);
}

#[test]
fn wrongtype_hash_op_on_string_key() {
    let (_server, mut c) = server_and_client();
    c.send("SET k v");
    assert_eq!(c.send("HSET k f v"), WRONGTYPE);
}

#[test]
fn wrongtype_set_op_on_list_key() {
    let (_server, mut c) = server_and_client();
    c.send("LPUSH k v");
    assert_eq!(c.send("SADD k m"), WRONGTYPE);
}

#[test]
fn wrongtype_zset_op_on_hash_key() {
    let (_server, mut c) = server_and_client();
    c.send("HSET k f v");
    assert_eq!(c.send("ZADD k 1 m"), WRONGTYPE);
}

#[test]
fn wrongtype_set_always_overwrites_regardless_of_type() {
    let (_server, mut c) = server_and_client();
    c.send("LPUSH k v");
    assert_eq!(c.send("SET k now-a-string"), "OK\r\n");
    assert_eq!(c.send("TYPE k"), "STRING\r\n");
}
