//! Commands that work on a key regardless of the type it holds, plus
//! keyspace-wide operations.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{remaining_tokens, reply_list, reply_ok_or_missing, usage};
use crate::app::App;
use crate::server::Conn;
use crate::util::glob::glob_match;
use crate::util::strutil::{next_token, parse_long, trim};

/// A deadline `millis` from now. A non-positive value lands in the past,
/// which makes the key expire on its next lookup - the same thing Redis
/// does for `EXPIRE key -1`.
pub(crate) fn deadline_after_millis(millis: i64) -> SystemTime {
    let now = SystemTime::now();
    if millis >= 0 {
        now + Duration::from_millis(millis as u64)
    } else {
        now.checked_sub(Duration::from_millis(millis.unsigned_abs()))
            .unwrap_or(UNIX_EPOCH)
    }
}

/// An absolute deadline from a Unix timestamp in milliseconds.
fn deadline_at_unix_millis(millis: i64) -> SystemTime {
    if millis <= 0 {
        UNIX_EPOCH
    } else {
        UNIX_EPOCH + Duration::from_millis(millis as u64)
    }
}

/// Shared parsing for the four EXPIRE variants: one key, one number.
/// `to_deadline` turns that number into an absolute instant.
fn expire_variant(
    app: &mut App,
    conn: &mut Conn,
    rest: &str,
    spec: &str,
    to_deadline: fn(i64) -> SystemTime,
) {
    let mut rest = rest;
    let key = next_token(&mut rest);
    let amount = parse_long(trim(rest));
    let (key, amount) = match (key, amount) {
        (Some(k), Some(n)) => (k, n),
        _ => return usage(conn, spec),
    };
    let ok = app.store.set_expire_at(key, Some(to_deadline(amount)));
    reply_ok_or_missing(conn, ok);
}

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        // DEL/UNLINK stay backwards compatible: one key keeps the
        // original OK/NOT_FOUND reply, several report a count instead.
        "DEL" | "UNLINK" => {
            let keys = remaining_tokens(&mut rest);
            match keys.len() {
                0 => usage(conn, "DEL key [key ...]"),
                1 => reply_ok_or_missing(conn, app.store.del(keys[0])),
                _ => {
                    let deleted = keys.iter().filter(|k| app.store.del(k)).count();
                    conn.reply(&format!("DELETED {}\r\n", deleted));
                }
            }
        }

        "EXISTS" => {
            let keys = remaining_tokens(&mut rest);
            if keys.is_empty() {
                return usage(conn, "EXISTS key [key ...]");
            }
            // Redis counts each key it is given, so a repeated key
            // that exists counts once per repetition.
            let found = keys.iter().filter(|k| app.store.exists(k)).count();
            conn.reply(&format!("COUNT {}\r\n", found));
        }

        "EXPIRE" => expire_variant(app, conn, rest, "EXPIRE key seconds", |secs| {
            deadline_after_millis(secs.saturating_mul(1000))
        }),

        "PEXPIRE" => expire_variant(app, conn, rest, "PEXPIRE key milliseconds", |ms| {
            deadline_after_millis(ms)
        }),

        "EXPIREAT" => expire_variant(app, conn, rest, "EXPIREAT key unix-time-seconds", |secs| {
            deadline_at_unix_millis(secs.saturating_mul(1000))
        }),

        "PEXPIREAT" => expire_variant(
            app,
            conn,
            rest,
            "PEXPIREAT key unix-time-milliseconds",
            deadline_at_unix_millis,
        ),

        "PERSIST" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "PERSIST key");
            }
            // NOT_FOUND covers both "no such key" and "key has no TTL",
            // matching Redis's single 0 reply for either case.
            let had_expiry = app.store.has_expiry(key);
            if had_expiry {
                app.store.set_expire_at(key, None);
            }
            reply_ok_or_missing(conn, had_expiry);
        }

        "TTL" | "PTTL" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, &format!("{} key", cmd));
            }
            if cmd == "TTL" {
                conn.reply(&format!("TTL {}\r\n", app.store.ttl(key)));
            } else {
                conn.reply(&format!("PTTL {}\r\n", app.store.pttl_ms(key)));
            }
        }

        "TYPE" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "TYPE key");
            }
            match app.store.type_of(key) {
                Some(t) => conn.reply(&format!("{}\r\n", t.name())),
                None => conn.reply("NONE\r\n"),
            }
        }

        "KEYS" => {
            let pattern = trim(rest);
            let pattern = if pattern.is_empty() {
                None
            } else {
                Some(pattern)
            };
            let matches: Vec<String> = app
                .store
                .foreach_key()
                .into_iter()
                .filter(|key| pattern.is_none_or(|p| glob_match(p, key)))
                .collect();
            reply_list(conn, matches);
        }

        "SCAN" => scan(app, conn, rest),

        "DBSIZE" => conn.reply(&format!("COUNT {}\r\n", app.store.size())),

        "RENAME" | "RENAMENX" => {
            let key = next_token(&mut rest);
            let new_key = next_token(&mut rest);
            let (key, new_key) = match (key, new_key) {
                (Some(k), Some(n)) => (k, n),
                _ => return usage(conn, &format!("{} key newkey", cmd)),
            };
            if !app.store.exists(key) {
                return conn.reply("NOT_FOUND\r\n");
            }
            if cmd == "RENAMENX" && key != new_key && app.store.exists(new_key) {
                return conn.reply("FALSE\r\n");
            }
            app.store.rename(key, new_key);
            conn.reply("OK\r\n");
        }

        "COPY" => {
            let source = next_token(&mut rest);
            let dest = next_token(&mut rest);
            let (source, dest) = match (source, dest) {
                (Some(s), Some(d)) => (s, d),
                _ => return usage(conn, "COPY source destination [REPLACE]"),
            };
            let replace = match next_token(&mut rest) {
                None => false,
                Some(opt) if opt.eq_ignore_ascii_case("REPLACE") => true,
                Some(_) => return usage(conn, "COPY source destination [REPLACE]"),
            };
            match app.store.copy(source, dest, replace) {
                None => conn.reply("NOT_FOUND\r\n"),
                Some(false) => conn.reply("FALSE\r\n"),
                Some(true) => conn.reply("OK\r\n"),
            }
        }

        "RANDOMKEY" => match app.store.random_key() {
            Some(key) => conn.reply(&format!("VALUE {}\r\n", key)),
            None => conn.reply("NOT_FOUND\r\n"),
        },

        // One keyspace, so FLUSHDB and FLUSHALL do the same thing. Both
        // exist so either name works.
        "FLUSHDB" | "FLUSHALL" => {
            app.store.flush();
            conn.reply("OK\r\n");
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

const SCAN_USAGE: &str = "SCAN cursor [MATCH pattern] [COUNT count]";

fn scan(app: &mut App, conn: &mut Conn, rest: &str) {
    let mut rest = rest;
    let cursor = match next_token(&mut rest)
        .and_then(parse_long)
        .filter(|&c| c >= 0)
    {
        Some(c) => c as usize,
        None => return usage(conn, SCAN_USAGE),
    };

    let mut pattern: Option<&str> = None;
    let mut count: i64 = app.config.scan_default_count as i64;
    while let Some(opt) = next_token(&mut rest) {
        match opt.to_ascii_uppercase().as_str() {
            "MATCH" => match next_token(&mut rest) {
                Some(p) => pattern = Some(p),
                None => return usage(conn, SCAN_USAGE),
            },
            "COUNT" => match next_token(&mut rest).and_then(parse_long) {
                Some(c) if c > 0 => count = c,
                _ => return usage(conn, SCAN_USAGE),
            },
            _ => return usage(conn, SCAN_USAGE),
        }
    }

    let (batch, next_cursor) = app.store.scan(cursor, count as usize);
    for key in &batch {
        if pattern.is_none_or(|p| glob_match(p, key)) {
            conn.reply(&format!("{}\r\n", key));
        }
    }
    conn.reply(&format!("CURSOR {}\r\n", next_cursor));
}
