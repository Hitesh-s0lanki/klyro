//! Command dispatch. The router lives here; the handlers are grouped by
//! the data type they operate on, mirroring `types/`.
//!
//! A handler takes the argument vector exactly as the client sent it -
//! `argv[0]` is the command name, raw bytes throughout - and returns a
//! [`Reply`]. Nothing writes to the connection directly, so a command's
//! result is a value that can be inspected and tested on its own.

mod blocking;
mod generic;
mod hash;
mod keyspec;
mod list;
mod memory;
mod pubsub;
mod server;
mod set;
mod string;
mod transactions;
mod zset;

use crate::app::App;
use crate::client::{Blocked, Client};
use crate::resp::{Protocol, Reply};
use crate::store::StoreType;
use crate::util::bytes::{to_display, to_upper, Bytes};

/// What dispatch produced: the frames to send this client, plus
/// whether the connection should close once they are flushed.
pub struct Response {
    /// Usually one frame. SUBSCRIBE sends one per channel, and a
    /// command that parked sends none until it is woken.
    pub replies: Vec<Reply>,
    pub close: bool,
    /// Set by HELLO when it switches the connection's protocol version.
    pub protocol: Option<Protocol>,
    /// Set when the command could not be answered yet and the client
    /// should be parked. The event loop stops reading from it until a
    /// key it waits on changes or its deadline passes.
    pub block: Option<Blocked>,
}

impl Response {
    pub(crate) fn new(reply: Reply) -> Response {
        Response::frames(vec![reply])
    }

    pub(crate) fn frames(replies: Vec<Reply>) -> Response {
        Response {
            replies,
            close: false,
            protocol: None,
            block: None,
        }
    }

    pub(crate) fn parked(block: Blocked) -> Response {
        Response {
            replies: Vec::new(),
            close: false,
            protocol: None,
            block: Some(block),
        }
    }

    /// The single frame a command produced, for the callers that can
    /// only carry one: EXEC's result array, and a blocking retry.
    pub(crate) fn single(self) -> Reply {
        self.replies.into_iter().next().unwrap_or(Reply::Nil)
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
    "MEM.VSEARCH",
    "MEM.QUERY",
    "MEM.SCAN",
];

/// Which group of handlers owns a command name.
///
/// Routing is a lookup rather than a match arm inside `run` because
/// MULTI needs the same answer: a command it cannot route is one it
/// must refuse to queue, so that EXEC never runs a partial
/// transaction.
#[derive(Clone, Copy, PartialEq)]
enum Family {
    Server,
    Generic,
    String,
    List,
    Hash,
    Set,
    Zset,
    Memory,
    Transaction,
    PubSub,
    Blocking,
}

fn family(name: &str) -> Option<Family> {
    Some(match name {
        // --- connection / server ---
        "PING" | "ECHO" | "INFO" | "CONFIG" | "SAVE" | "QUIT" | "SHUTDOWN" | "COMMAND"
        | "HELLO" | "CLIENT" => Family::Server,

        // --- transactions ---
        "MULTI" | "EXEC" | "DISCARD" | "WATCH" | "UNWATCH" | "RESET" => Family::Transaction,

        // --- pub/sub ---
        "SUBSCRIBE" | "UNSUBSCRIBE" | "PSUBSCRIBE" | "PUNSUBSCRIBE" | "PUBLISH" | "PUBSUB" => {
            Family::PubSub
        }

        // --- blocking pops, and the two non-blocking commands that
        // share their argument shape ---
        "BLPOP" | "BRPOP" | "BLMOVE" | "BRPOPLPUSH" | "BLMPOP" | "LMPOP" | "BZPOPMIN"
        | "BZPOPMAX" | "BZMPOP" | "ZMPOP" => Family::Blocking,

        // --- generic key commands ---
        "DEL" | "UNLINK" | "EXISTS" | "EXPIRE" | "PEXPIRE" | "EXPIREAT" | "PEXPIREAT"
        | "PERSIST" | "TTL" | "PTTL" | "TYPE" | "KEYS" | "SCAN" | "DBSIZE" | "RENAME"
        | "RENAMENX" | "COPY" | "RANDOMKEY" | "FLUSHDB" | "FLUSHALL" => Family::Generic,

        // --- string commands ---
        "SET" | "SETNX" | "SETEX" | "PSETEX" | "GET" | "GETSET" | "GETDEL" | "GETEX" | "MGET"
        | "MSET" | "MSETNX" | "INCR" | "DECR" | "INCRBY" | "DECRBY" | "INCRBYFLOAT" | "APPEND"
        | "STRLEN" | "GETRANGE" | "SUBSTR" | "SETRANGE" => Family::String,

        // --- list commands ---
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" | "LPOP" | "RPOP" | "LLEN" | "LRANGE"
        | "LINDEX" | "LSET" | "LINSERT" | "LREM" | "LTRIM" | "RPOPLPUSH" | "LMOVE" => Family::List,

        // --- hash commands ---
        "HSET" | "HSETNX" | "HMSET" | "HGET" | "HMGET" | "HDEL" | "HLEN" | "HEXISTS" | "HKEYS"
        | "HVALS" | "HGETALL" | "HINCRBY" | "HINCRBYFLOAT" | "HSTRLEN" => Family::Hash,

        // --- set commands ---
        "SADD" | "SREM" | "SISMEMBER" | "SMISMEMBER" | "SCARD" | "SMEMBERS" | "SPOP"
        | "SRANDMEMBER" | "SMOVE" | "SINTER" | "SINTERSTORE" | "SUNION" | "SUNIONSTORE"
        | "SDIFF" | "SDIFFSTORE" => Family::Set,

        // --- sorted set commands ---
        "ZADD" | "ZSCORE" | "ZMSCORE" | "ZINCRBY" | "ZREM" | "ZCARD" | "ZCOUNT" | "ZRANGE"
        | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZRANK" | "ZREVRANK"
        | "ZREMRANGEBYRANK" | "ZREMRANGEBYSCORE" | "ZPOPMIN" | "ZPOPMAX" => Family::Zset,

        // --- memory index commands ---
        // Matched by prefix rather than by listing each verb, so the
        // family can grow without the router growing with it. An
        // unknown verb is answered by the handler, which can say which
        // command was meant instead of falling through to the generic
        // "unknown command".
        _ if name.starts_with("MEM.") => Family::Memory,

        _ => return None,
    })
}

/// Commands a RESP2 connection may still run while it holds a
/// subscription. RESP2 has no marker separating a delivered message
/// from a reply, so a subscriber's socket is reserved for messages;
/// RESP3 marks pushes and lifts the restriction entirely.
const SUBSCRIBER_COMMANDS: &[&str] = &[
    "SUBSCRIBE",
    "UNSUBSCRIBE",
    "PSUBSCRIBE",
    "PUNSUBSCRIBE",
    "PING",
    "QUIT",
    "RESET",
    "SHUTDOWN",
];

/// Executes one already-parsed command.
pub fn dispatch(app: &mut App, client: &mut Client, argv: &[Bytes]) -> Option<Response> {
    if argv.is_empty() {
        return None; // an empty inline line or `*0` array
    }
    let name = to_upper(&argv[0]);

    // Inside MULTI everything but the transaction's own control
    // commands is queued rather than run.
    if client.in_transaction() && !transactions::is_control(&name) {
        return Some(transactions::queue(client, &name, argv));
    }
    Some(execute(app, client, &name, argv))
}

/// Runs one command for real: the path both a plain command and a
/// command replayed by EXEC take, so the counters, the subscriber
/// gate, and the write signal cannot be reached only one of the two
/// ways.
pub(crate) fn execute(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    app.stats.total_commands += 1;

    if client.protocol == Protocol::Resp2
        && client.is_subscriber()
        && !SUBSCRIBER_COMMANDS.contains(&name)
    {
        return Response::new(Reply::error(format!(
            "ERR Can't execute '{}': only (P)SUBSCRIBE / (P)UNSUBSCRIBE / PING / QUIT / RESET are allowed in this context",
            name.to_ascii_lowercase()
        )));
    }

    let before = app.store.lookup_counts();
    let response = run(app, client, name, argv);

    // Attributing the delta, rather than counting inside each handler,
    // keeps the accounting in one place instead of across every
    // command.
    if READ_COMMANDS.contains(&name) {
        let after = app.store.lookup_counts();
        app.stats.keyspace_hits += after.0 - before.0;
        app.stats.keyspace_misses += after.1 - before.1;
    }
    signal_writes(app, name, argv, &response);
    response
}

/// Tells the watch and blocking registries which keys just changed.
///
/// A command that failed its argument checks changed nothing, and one
/// that parked has not run yet, so neither signals - otherwise every
/// WRONGTYPE would abort an unrelated transaction.
fn signal_writes(app: &mut App, name: &str, argv: &[Bytes], response: &Response) {
    if response.block.is_some() || matches!(response.replies.first(), Some(Reply::Error(_))) {
        return;
    }
    match keyspec::written_keys(name, argv) {
        None => {}
        Some(keyspec::Written::Everything) => app.signal_flushed(),
        Some(keyspec::Written::Keys(keys)) => {
            for key in keys {
                app.signal_modified(&key);
            }
        }
    }
}

/// Re-runs a parked command now that one of its keys has changed.
/// `None` means it still cannot be answered and stays parked.
pub fn retry(app: &mut App, client: &mut Client, argv: &[Bytes]) -> Option<Reply> {
    let name = to_upper(&argv[0]);
    let response = execute(app, client, &name, argv);
    // The command was counted when the client first sent it. A retry is
    // that same command finishing, not another one.
    app.stats.total_commands -= 1;
    if response.block.is_some() {
        return None;
    }
    Some(response.single())
}

fn run(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    match family(name) {
        Some(Family::Server) => server::dispatch(app, client, name, argv),
        Some(Family::Transaction) => transactions::dispatch(app, client, name, argv),
        Some(Family::PubSub) => pubsub::dispatch(app, client, name, argv),
        Some(Family::Blocking) => blocking::dispatch(app, client, name, argv),
        Some(Family::Generic) => Response::new(generic::dispatch(app, name, argv)),
        Some(Family::String) => Response::new(string::dispatch(app, name, argv)),
        Some(Family::List) => Response::new(list::dispatch(app, name, argv)),
        Some(Family::Hash) => Response::new(hash::dispatch(app, name, argv)),
        Some(Family::Set) => Response::new(set::dispatch(app, name, argv)),
        Some(Family::Zset) => Response::new(zset::dispatch(app, name, argv)),
        Some(Family::Memory) => Response::new(memory::dispatch(app, name, argv)),
        None => Response::new(unknown_command(argv)),
    }
}

fn unknown_command(argv: &[Bytes]) -> Reply {
    Reply::error(format!(
        "ERR unknown command '{}', with args beginning with: {}",
        to_display(&argv[0]),
        argv[1..]
            .iter()
            .take(3)
            .map(|a| format!("'{}'", to_display(a)))
            .collect::<Vec<_>>()
            .join(", ")
    ))
}
