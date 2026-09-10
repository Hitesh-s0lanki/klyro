//! Command parsing and dispatch. The router lives here; the handlers
//! are grouped by the data type they operate on, mirroring `types/`.
//!
//! Every handler takes the already-uppercased command name plus the
//! unparsed remainder of the line, and writes its own reply. Argument
//! parsing is per-command rather than table-driven because the line
//! protocol's "last argument is the rest of the line" rule (which keeps
//! values with spaces working for SET/HSET/LSET) can't be expressed as
//! a uniform arity.

mod generic;
mod hash;
mod list;
mod server;
mod set;
mod string;
mod zset;

use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::util::strutil::{next_token, trim};

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

/// Replies WRONGTYPE and returns `false` if `key` exists with a type
/// other than `want`; otherwise (missing, or already the right type)
/// returns `true` and replies nothing.
pub(crate) fn check_type(app: &mut App, conn: &mut Conn, key: &str, want: StoreType) -> bool {
    // `peek_type`, not `type_of`: a type check is bookkeeping, not a
    // lookup the caller asked for, and counting it would double every
    // read command's contribution to INFO's hit ratio.
    match app.store.peek_type(key) {
        Some(t) if t != want => {
            conn.reply(WRONGTYPE);
            false
        }
        _ => true,
    }
}

/// Replies `ERR usage: <spec>`.
pub(crate) fn usage(conn: &mut Conn, spec: &str) {
    conn.reply(&format!("ERR usage: {}\r\n", spec));
}

/// Sends one line per item, then the `END` terminator that every
/// multi-line reply in this protocol closes with.
pub(crate) fn reply_list<I, S>(conn: &mut Conn, items: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for item in items {
        conn.reply(&format!("{}\r\n", item.as_ref()));
    }
    conn.reply("END\r\n");
}

/// Replies `OK` or `NOT_FOUND` - the shape most mutating single-key
/// commands use.
pub(crate) fn reply_ok_or_missing(conn: &mut Conn, ok: bool) {
    conn.reply(if ok { "OK\r\n" } else { "NOT_FOUND\r\n" });
}

/// Collects every remaining whitespace-delimited token.
pub(crate) fn remaining_tokens<'a>(rest: &mut &'a str) -> Vec<&'a str> {
    let mut tokens = Vec::new();
    while let Some(tok) = next_token(rest) {
        tokens.push(tok);
    }
    tokens
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
];

/// Parses and executes one command line, replying on `conn`.
pub fn dispatch(app: &mut App, conn: &mut Conn, line: &str) {
    let line = trim(line);
    if line.is_empty() {
        return;
    }

    let mut rest = line;
    let cmd = match next_token(&mut rest) {
        Some(c) => c,
        None => return,
    };
    let cmd = cmd.to_ascii_uppercase();

    app.stats.total_commands += 1;
    let before = app.store.lookup_counts();

    run(app, conn, &cmd, rest);

    // Attributing the delta, rather than counting inside each handler,
    // keeps the accounting in one place instead of across 105 commands.
    if READ_COMMANDS.contains(&cmd.as_str()) {
        let after = app.store.lookup_counts();
        app.stats.keyspace_hits += after.0 - before.0;
        app.stats.keyspace_misses += after.1 - before.1;
    }
}

fn run(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    match cmd {
        // --- connection / server ---
        "PING" | "ECHO" | "INFO" | "CONFIG" | "SAVE" | "QUIT" | "SHUTDOWN" => {
            server::dispatch(app, conn, cmd, rest)
        }

        // --- generic key commands ---
        "DEL" | "UNLINK" | "EXISTS" | "EXPIRE" | "PEXPIRE" | "EXPIREAT" | "PEXPIREAT"
        | "PERSIST" | "TTL" | "PTTL" | "TYPE" | "KEYS" | "SCAN" | "DBSIZE" | "RENAME"
        | "RENAMENX" | "COPY" | "RANDOMKEY" | "FLUSHDB" | "FLUSHALL" => {
            generic::dispatch(app, conn, cmd, rest)
        }

        // --- string commands ---
        "SET" | "SETNX" | "SETEX" | "PSETEX" | "GET" | "GETSET" | "GETDEL" | "GETEX" | "MGET"
        | "MSET" | "INCR" | "DECR" | "INCRBY" | "DECRBY" | "INCRBYFLOAT" | "APPEND" | "STRLEN"
        | "GETRANGE" | "SETRANGE" => string::dispatch(app, conn, cmd, rest),

        // --- list commands ---
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" | "LPOP" | "RPOP" | "LLEN" | "LRANGE"
        | "LINDEX" | "LSET" | "LINSERT" | "LREM" | "LTRIM" | "RPOPLPUSH" | "LMOVE" => {
            list::dispatch(app, conn, cmd, rest)
        }

        // --- hash commands ---
        "HSET" | "HSETNX" | "HMSET" | "HGET" | "HMGET" | "HDEL" | "HLEN" | "HEXISTS" | "HKEYS"
        | "HVALS" | "HGETALL" | "HINCRBY" | "HINCRBYFLOAT" | "HSTRLEN" => {
            hash::dispatch(app, conn, cmd, rest)
        }

        // --- set commands ---
        "SADD" | "SREM" | "SISMEMBER" | "SMISMEMBER" | "SCARD" | "SMEMBERS" | "SPOP"
        | "SRANDMEMBER" | "SMOVE" | "SINTER" | "SINTERSTORE" | "SUNION" | "SUNIONSTORE"
        | "SDIFF" | "SDIFFSTORE" => set::dispatch(app, conn, cmd, rest),

        // --- sorted set commands ---
        "ZADD" | "ZSCORE" | "ZMSCORE" | "ZINCRBY" | "ZREM" | "ZCARD" | "ZCOUNT" | "ZRANGE"
        | "ZREVRANGE" | "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZRANK" | "ZREVRANK"
        | "ZREMRANGEBYRANK" | "ZREMRANGEBYSCORE" | "ZPOPMIN" | "ZPOPMAX" => {
            zset::dispatch(app, conn, cmd, rest)
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}
