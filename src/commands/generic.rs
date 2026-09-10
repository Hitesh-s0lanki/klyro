//! Commands that work on a key regardless of the type it holds, plus
//! keyspace-wide operations.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{exact_args, min_args, parse_int, syntax_error, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::util::bytes::{eq_ignore_case, Bytes};
use crate::util::glob::glob_match;

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

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "DEL" | "UNLINK" => {
            min_args(argv, name, 1)?;
            let deleted = argv[1..].iter().filter(|key| app.store.del(key)).count();
            Ok(Reply::Integer(deleted as i64))
        }

        "EXISTS" => {
            min_args(argv, name, 1)?;
            // Redis counts each key it is given, so a repeated key that
            // exists counts once per repetition.
            let found = argv[1..].iter().filter(|key| app.store.exists(key)).count();
            Ok(Reply::Integer(found as i64))
        }

        "EXPIRE" => expire_variant(app, argv, name, |secs| {
            deadline_after_millis(secs.saturating_mul(1000))
        }),
        "PEXPIRE" => expire_variant(app, argv, name, deadline_after_millis),
        "EXPIREAT" => expire_variant(app, argv, name, |secs| {
            deadline_at_unix_millis(secs.saturating_mul(1000))
        }),
        "PEXPIREAT" => expire_variant(app, argv, name, deadline_at_unix_millis),

        "PERSIST" => {
            exact_args(argv, name, 1)?;
            // 0 covers both "no such key" and "key has no TTL", as in
            // Redis.
            let had_expiry = app.store.has_expiry(&argv[1]);
            if had_expiry {
                app.store.set_expire_at(&argv[1], None);
            }
            Ok(Reply::bool(had_expiry))
        }

        "TTL" => {
            exact_args(argv, name, 1)?;
            Ok(Reply::Integer(app.store.ttl(&argv[1])))
        }

        "PTTL" => {
            exact_args(argv, name, 1)?;
            Ok(Reply::Integer(app.store.pttl_ms(&argv[1])))
        }

        "TYPE" => {
            exact_args(argv, name, 1)?;
            Ok(Reply::Simple(match app.store.type_of(&argv[1]) {
                Some(t) => t.name(),
                None => "none",
            }))
        }

        "KEYS" => {
            exact_args(argv, name, 1)?;
            let pattern = &argv[1];
            let matches: Vec<Bytes> = app
                .store
                .foreach_key()
                .into_iter()
                .filter(|key| glob_match(pattern, key))
                .collect();
            Ok(Reply::bulk_array(matches))
        }

        "SCAN" => scan(app, argv),

        "DBSIZE" => {
            exact_args(argv, name, 0)?;
            Ok(Reply::Integer(app.store.size() as i64))
        }

        "RENAME" | "RENAMENX" => {
            exact_args(argv, name, 2)?;
            let (key, new_key) = (&argv[1], &argv[2]);
            if !app.store.exists(key) {
                return Ok(Reply::error("ERR no such key"));
            }
            if name == "RENAMENX" {
                if key != new_key && app.store.exists(new_key) {
                    return Ok(Reply::bool(false));
                }
                app.store.rename(key, new_key);
                return Ok(Reply::bool(true));
            }
            app.store.rename(key, new_key);
            Ok(Reply::ok())
        }

        "COPY" => {
            min_args(argv, name, 2)?;
            let replace = match argv.len() {
                3 => false,
                4 if eq_ignore_case(&argv[3], "REPLACE") => true,
                _ => return Err(syntax_error()),
            };
            match app.store.copy(&argv[1], &argv[2], replace) {
                None | Some(false) => Ok(Reply::bool(false)),
                Some(true) => Ok(Reply::bool(true)),
            }
        }

        "RANDOMKEY" => {
            exact_args(argv, name, 0)?;
            Ok(match app.store.random_key() {
                Some(key) => Reply::Bulk(key),
                None => Reply::Nil,
            })
        }

        // One keyspace, so FLUSHDB and FLUSHALL do the same thing. Both
        // exist so either name works.
        "FLUSHDB" | "FLUSHALL" => {
            app.store.flush();
            Ok(Reply::ok())
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}

/// Shared parsing for the four EXPIRE variants: one key, one number.
/// `to_deadline` turns that number into an absolute instant.
fn expire_variant(
    app: &mut App,
    argv: &[Bytes],
    name: &str,
    to_deadline: fn(i64) -> SystemTime,
) -> Checked<Reply> {
    exact_args(argv, name, 2)?;
    let amount = parse_int(&argv[2])?;
    Ok(Reply::bool(
        app.store.set_expire_at(&argv[1], Some(to_deadline(amount))),
    ))
}

/// `SCAN cursor [MATCH pattern] [COUNT count]` - replies with the next
/// cursor as a bulk string and the batch as an array, the two-element
/// shape every Redis client expects.
fn scan(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "SCAN", 1)?;
    let cursor = match parse_int(&argv[1]) {
        Ok(c) if c >= 0 => c as usize,
        _ => return Err(Reply::error("ERR invalid cursor")),
    };

    let mut pattern: Option<&Bytes> = None;
    let mut count = app.config.scan_default_count as i64;
    let mut i = 2;
    while i < argv.len() {
        if eq_ignore_case(&argv[i], "MATCH") && i + 1 < argv.len() {
            pattern = Some(&argv[i + 1]);
        } else if eq_ignore_case(&argv[i], "COUNT") && i + 1 < argv.len() {
            count = match parse_int(&argv[i + 1])? {
                c if c > 0 => c,
                _ => return Err(syntax_error()),
            };
        } else {
            return Err(syntax_error());
        }
        i += 2;
    }

    let (batch, next_cursor) = app.store.scan(cursor, count as usize);
    let keys: Vec<Bytes> = batch
        .into_iter()
        .filter(|key| pattern.is_none_or(|p| glob_match(p, key)))
        .collect();
    Ok(Reply::array(vec![
        Reply::bulk(next_cursor.to_string()),
        Reply::bulk_array(keys),
    ]))
}
