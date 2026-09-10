//! Command dispatch. The router lives here; the handlers are grouped by
//! the data type they operate on, mirroring `types/`.
//!
//! A handler takes the argument vector exactly as the client sent it -
//! `argv[0]` is the command name, raw bytes throughout - and returns a
//! [`Reply`]. Nothing writes to the connection directly, so a command's
//! result is a value that can be inspected and tested on its own.

mod generic;
mod hash;
mod list;
mod memory;
mod server;
mod set;
mod string;
mod zset;

use crate::app::App;
use crate::resp::{Protocol, Reply};
use crate::store::StoreType;
use crate::util::bytes::{to_display, to_upper, Bytes};

/// What dispatch produced: a reply, plus whether the connection should
/// close once it has been flushed.
pub struct Response {
    pub reply: Reply,
    pub close: bool,
    /// Set by HELLO when it switches the connection's protocol version.
    pub protocol: Option<Protocol>,
}

impl Response {
    fn new(reply: Reply) -> Response {
        Response {
            reply,
            close: false,
            protocol: None,
        }
    }
}

/// Both arms of these results carry a reply; `Err` just marks the ones
/// that stop the handler early, so argument checks can use `?`.
pub(crate) type Checked<T> = Result<T, Reply>;

pub(crate) fn wrongtype() -> Reply {
    Reply::error("WRONGTYPE Operation against a key holding the wrong kind of value")
}

pub(crate) fn not_an_integer() -> Reply {
    Reply::error("ERR value is not an integer or out of range")
}

pub(crate) fn not_a_float() -> Reply {
    Reply::error("ERR value is not a valid float")
}

pub(crate) fn syntax_error() -> Reply {
    Reply::error("ERR syntax error")
}

/// Fails with WRONGTYPE if `key` holds something other than `want`. A
/// missing key passes, since most commands create it.
pub(crate) fn check_type(app: &mut App, key: &[u8], want: StoreType) -> Checked<()> {
    match app.store.peek_type(key) {
        Some(found) if found != want => Err(wrongtype()),
        _ => Ok(()),
    }
}

pub(crate) fn parse_int(arg: &[u8]) -> Checked<i64> {
    crate::util::bytes::parse_i64(arg).ok_or_else(not_an_integer)
}

pub(crate) fn parse_float(arg: &[u8]) -> Checked<f64> {
    crate::util::bytes::parse_f64(arg).ok_or_else(not_a_float)
}

/// Requires exactly `n` arguments after the command name.
pub(crate) fn exact_args(argv: &[Bytes], name: &str, n: usize) -> Checked<()> {
    if argv.len() == n + 1 {
        Ok(())
    } else {
        Err(Reply::wrong_arity(name))
    }
}

/// Requires at least `n` arguments after the command name.
pub(crate) fn min_args(argv: &[Bytes], name: &str, n: usize) -> Checked<()> {
    if argv.len() > n {
        Ok(())
    } else {
        Err(Reply::wrong_arity(name))
    }
}

/// Commands that only read the keyspace. INFO's hit ratio counts the
/// lookups these make and nothing else, so the number means what it
/// does in Redis rather than also counting every write's lookup.
const READ_COMMANDS: &[&str] = &[
    "EXISTS",
    "TYPE",
    "TTL",
    "PTTL",
    "GET",
    "MGET",
    "GETRANGE",
    "SUBSTR",
    "STRLEN",
    "LINDEX",
    "LLEN",
    "LRANGE",
    "HGET",
    "HMGET",
    "HLEN",
    "HEXISTS",
    "HKEYS",
    "HVALS",
    "HGETALL",
    "HSTRLEN",
    "SISMEMBER",
    "SMISMEMBER",
    "SCARD",
    "SMEMBERS",
    "SRANDMEMBER",
    "SINTER",
    "SUNION",
    "SDIFF",
    "ZSCORE",
    "ZMSCORE",
    "ZCARD",
    "ZCOUNT",
    "ZRANGE",
    "ZREVRANGE",
    "ZRANGEBYSCORE",
    "ZREVRANGEBYSCORE",
    "ZRANK",
    "ZREVRANK",
    "MEM.GET",
    "MEM.MGET",
    "MEM.CARD",
    "MEM.INFO",
    "MEM.SEARCH",
];

/// Executes one already-parsed command.
pub fn dispatch(app: &mut App, argv: &[Bytes]) -> Option<Response> {
    if argv.is_empty() {
        return None; // an empty inline line or `*0` array
    }
    let name = to_upper(&argv[0]);

    app.stats.total_commands += 1;
    let before = app.store.lookup_counts();

    let response = run(app, &name, argv);

    // Attributing the delta, rather than counting inside each handler,
    // keeps the accounting in one place instead of across 108 commands.
    if READ_COMMANDS.contains(&name.as_str()) {
        let after = app.store.lookup_counts();
        app.stats.keyspace_hits += after.0 - before.0;
        app.stats.keyspace_misses += after.1 - before.1;
    }
    Some(response)
}

fn run(app: &mut App, name: &str, argv: &[Bytes]) -> Response {
    match name {
        // --- connection / server ---
        "PING" | "ECHO" | "INFO" | "CONFIG" | "SAVE" | "QUIT" | "SHUTDOWN" | "COMMAND"
        | "HELLO" => server::dispatch(app, name, argv),

        // --- generic key commands ---
        "DEL" | "UNLINK" | "EXISTS" | "EXPIRE" | "PEXPIRE" | "EXPIREAT" | "PEXPIREAT"
        | "PERSIST" | "TTL" | "PTTL" | "TYPE" | "KEYS" | "SCAN" | "DBSIZE" | "RENAME"
        | "RENAMENX" | "COPY" | "RANDOMKEY" | "FLUSHDB" | "FLUSHALL" => {
            Response::new(generic::dispatch(app, name, argv))
        }

        // --- string commands ---
        "SET" | "SETNX" | "SETEX" | "PSETEX" | "GET" | "GETSET" | "GETDEL" | "GETEX" | "MGET"
        | "MSET" | "MSETNX" | "INCR" | "DECR" | "INCRBY" | "DECRBY" | "INCRBYFLOAT" | "APPEND"
        | "STRLEN" | "GETRANGE" | "SUBSTR" | "SETRANGE" => {
            Response::new(string::dispatch(app, name, argv))
        }

        // --- list commands ---
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" | "LPOP" | "RPOP" | "LLEN" | "LRANGE"
        | "LINDEX" | "LSET" | "LINSERT" | "LREM" | "LTRIM" | "RPOPLPUSH" | "LMOVE" => {
            Response::new(list::dispatch(app, name, argv))
        }

        // --- hash commands ---
        "HSET" | "HSETNX" | "HMSET" | "HGET" | "HMGET" | "HDEL" | "HLEN" | "HEXISTS" | "HKEYS"
        | "HVALS" | "HGETALL" | "HINCRBY" | "HINCRBYFLOAT" | "HSTRLEN" => {
            Response::new(hash::dispatch(app, name, argv))
        }

        // --- set commands ---
        "SADD" | "SREM" | "SISMEMBER" | "SMISMEMBER" | "SCARD" | "SMEMBERS" | "SPOP"
        | "SRANDMEMBER" | "SMOVE" | "SINTER" | "SINTERSTORE" | "SUNION" | "SUNIONSTORE"
        | "SDIFF" | "SDIFFSTORE" => Response::new(set::dispatch(app, name, argv)),

        // --- sorted set commands ---
        "ZADD" | "ZSCORE" | "ZMSCORE" | "ZINCRBY" | "ZREM" | "ZCARD" | "ZCOUNT" | "ZRANGE"
        | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZRANK" | "ZREVRANK"
        | "ZREMRANGEBYRANK" | "ZREMRANGEBYSCORE" | "ZPOPMIN" | "ZPOPMAX" => {
            Response::new(zset::dispatch(app, name, argv))
        }

        // --- memory index commands ---
        // Matched by prefix rather than by listing each verb, so the
        // family can grow without the router growing with it. An
        // unknown verb is answered by the handler, which can say which
        // command was meant instead of falling through to the generic
        // "unknown command".
        _ if name.starts_with("MEM.") => Response::new(memory::dispatch(app, name, argv)),

        _ => Response::new(Reply::error(format!(
            "ERR unknown command '{}', with args beginning with: {}",
            to_display(&argv[0]),
            argv[1..]
                .iter()
                .take(3)
                .map(|a| format!("'{}'", to_display(a)))
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}
