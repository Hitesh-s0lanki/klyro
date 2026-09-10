//! Which keys a command writes.
//!
//! WATCH has to know when a key it is holding changes, and a parked
//! BLPOP has to know when a key it is waiting on gets a value. Both
//! questions are the same one: which keys did the command that just
//! ran modify? Redis answers it inside every handler, calling
//! `signalModifiedKey` at each mutation. Klyro answers it once, from a
//! table, so a handler stays a function from arguments to a reply and
//! no new command can forget to signal.
//!
//! Only write commands appear here. A name that is absent is read-only
//! as far as the registries are concerned, which is the safe default
//! in the sense that matters: a missing entry can only be a command
//! that was never meant to change anything.

use crate::util::bytes::Bytes;

/// Where a command's keys sit in its argument vector.
#[derive(Clone, Copy)]
enum Position {
    /// `argv[first..=last]`, taking every `step`-th argument. A
    /// negative `last` counts back from the end, so `-1` is the last
    /// argument and `-2` the one before it - which is what the
    /// blocking commands need, their final argument being a timeout.
    Range {
        first: usize,
        last: isize,
        step: usize,
    },
    /// A `numkeys` count at `at`, with that many keys after it - the
    /// shape LMPOP and ZMPOP take.
    Numkeys { at: usize },
    /// The whole keyspace: FLUSHDB and FLUSHALL.
    Everything,
}

/// What a command modified.
pub enum Written {
    Keys(Vec<Bytes>),
    Everything,
}

const fn range(first: usize, last: isize, step: usize) -> Position {
    Position::Range { first, last, step }
}

/// One key at argument 1 - by far the most common shape.
const fn first_key() -> Position {
    range(1, 1, 1)
}

/// A source and a destination, at arguments 1 and 2.
const fn two_keys() -> Position {
    range(1, 2, 1)
}

/// Every argument from 1 on is a key.
const fn all_keys() -> Position {
    range(1, -1, 1)
}

/// Every argument from 1 up to the trailing timeout is a key.
const fn all_but_timeout() -> Position {
    range(1, -2, 1)
}

/// Every command that changes the keyspace, with where its keys are.
const WRITES: &[(&str, Position)] = &[
    // --- generic ---
    ("DEL", all_keys()),
    ("UNLINK", all_keys()),
    ("EXPIRE", first_key()),
    ("PEXPIRE", first_key()),
    ("EXPIREAT", first_key()),
    ("PEXPIREAT", first_key()),
    ("PERSIST", first_key()),
    ("RENAME", two_keys()),
    ("RENAMENX", two_keys()),
    ("COPY", two_keys()),
    ("FLUSHDB", Position::Everything),
    ("FLUSHALL", Position::Everything),
    // --- strings ---
    ("SET", first_key()),
    ("SETNX", first_key()),
    ("SETEX", first_key()),
    ("PSETEX", first_key()),
    ("GETSET", first_key()),
    ("GETDEL", first_key()),
    ("GETEX", first_key()),
    ("INCR", first_key()),
    ("DECR", first_key()),
    ("INCRBY", first_key()),
    ("DECRBY", first_key()),
    ("INCRBYFLOAT", first_key()),
    ("APPEND", first_key()),
    ("SETRANGE", first_key()),
    ("MSET", range(1, -1, 2)),
    ("MSETNX", range(1, -1, 2)),
    // --- lists ---
    ("LPUSH", first_key()),
    ("RPUSH", first_key()),
    ("LPUSHX", first_key()),
    ("RPUSHX", first_key()),
    ("LPOP", first_key()),
    ("RPOP", first_key()),
    ("LSET", first_key()),
    ("LINSERT", first_key()),
    ("LREM", first_key()),
    ("LTRIM", first_key()),
    ("RPOPLPUSH", two_keys()),
    ("LMOVE", two_keys()),
    ("LMPOP", Position::Numkeys { at: 1 }),
    ("BLPOP", all_but_timeout()),
    ("BRPOP", all_but_timeout()),
    ("BLMOVE", two_keys()),
    ("BRPOPLPUSH", two_keys()),
    ("BLMPOP", Position::Numkeys { at: 2 }),
    // --- hashes ---
    ("HSET", first_key()),
    ("HSETNX", first_key()),
    ("HMSET", first_key()),
    ("HDEL", first_key()),
    ("HINCRBY", first_key()),
    ("HINCRBYFLOAT", first_key()),
    // --- sets ---
    ("SADD", first_key()),
    ("SREM", first_key()),
    ("SPOP", first_key()),
    ("SMOVE", two_keys()),
    ("SINTERSTORE", all_keys()),
    ("SUNIONSTORE", all_keys()),
    ("SDIFFSTORE", all_keys()),
    // --- sorted sets ---
    ("ZADD", first_key()),
    ("ZINCRBY", first_key()),
    ("ZREM", first_key()),
    ("ZREMRANGEBYRANK", first_key()),
    ("ZREMRANGEBYSCORE", first_key()),
    ("ZPOPMIN", first_key()),
    ("ZPOPMAX", first_key()),
    ("ZMPOP", Position::Numkeys { at: 1 }),
    ("BZPOPMIN", all_but_timeout()),
    ("BZPOPMAX", all_but_timeout()),
    ("BZMPOP", Position::Numkeys { at: 2 }),
    // --- memory indexes ---
    ("MEM.CREATE", first_key()),
    ("MEM.CONFIG", first_key()),
    ("MEM.ADD", first_key()),
    ("MEM.DEL", first_key()),
    ("MEM.SETMETA", first_key()),
    ("MEM.DELMETA", first_key()),
    ("MEM.EXPIRE", first_key()),
];

/// The keys `argv` modifies, or `None` for a read-only command.
///
/// Arguments that fall outside the vector are skipped rather than
/// reported: a malformed command is answered with an arity error and
/// changes nothing, so there is nothing to signal.
pub fn written_keys(name: &str, argv: &[Bytes]) -> Option<Written> {
    let position = WRITES
        .iter()
        .find(|(command, _)| *command == name)
        .map(|(_, position)| *position)?;

    Some(match position {
        Position::Everything => Written::Everything,
        Position::Numkeys { at } => {
            let count = argv
                .get(at)
                .and_then(|n| crate::util::bytes::parse_i64(n))
                .unwrap_or(0)
                .max(0) as usize;
            Written::Keys(argv.iter().skip(at + 1).take(count).cloned().collect())
        }
        Position::Range { first, last, step } => {
            let last = if last < 0 {
                argv.len() as isize + last
            } else {
                last
            };
            let mut keys = Vec::new();
            let mut at = first;
            while (at as isize) <= last && at < argv.len() {
                keys.push(argv[at].clone());
                at += step;
            }
            Written::Keys(keys)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(command: &str) -> Vec<String> {
        let argv: Vec<Bytes> = command
            .split_whitespace()
            .map(|a| a.as_bytes().to_vec())
            .collect();
        let name = crate::util::bytes::to_upper(&argv[0]);
        match written_keys(&name, &argv) {
            Some(Written::Keys(keys)) => keys
                .into_iter()
                .map(|k| String::from_utf8(k).unwrap())
                .collect(),
            Some(Written::Everything) => vec!["*".to_string()],
            None => Vec::new(),
        }
    }

    #[test]
    fn a_read_command_writes_nothing() {
        assert!(keys("GET k").is_empty());
        assert!(keys("LRANGE k 0 -1").is_empty());
    }

    #[test]
    fn the_common_shape_is_one_key_at_the_front() {
        assert_eq!(keys("SET k v"), vec!["k"]);
        assert_eq!(keys("LPUSH q a b c"), vec!["q"]);
    }

    #[test]
    fn variadic_commands_report_every_key() {
        assert_eq!(keys("DEL a b c"), vec!["a", "b", "c"]);
        assert_eq!(keys("SINTERSTORE dest a b"), vec!["dest", "a", "b"]);
    }

    #[test]
    fn mset_skips_the_values_between_its_keys() {
        assert_eq!(keys("MSET a 1 b 2 c 3"), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_trailing_timeout_is_not_a_key() {
        assert_eq!(keys("BLPOP one two 0"), vec!["one", "two"]);
        assert_eq!(keys("BZPOPMIN z 5"), vec!["z"]);
    }

    #[test]
    fn a_numkeys_count_bounds_the_key_list() {
        assert_eq!(keys("LMPOP 2 a b LEFT"), vec!["a", "b"]);
        assert_eq!(keys("BLMPOP 0 2 a b LEFT COUNT 3"), vec!["a", "b"]);
        assert_eq!(keys("ZMPOP 1 z MIN"), vec!["z"]);
    }

    #[test]
    fn a_flush_reports_the_whole_keyspace() {
        assert_eq!(keys("FLUSHALL"), vec!["*"]);
    }

    #[test]
    fn a_truncated_command_reports_only_what_is_there() {
        assert!(keys("SET").is_empty());
        assert!(keys("LMPOP 2 a").len() <= 1);
    }
}
