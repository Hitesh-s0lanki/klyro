//! String commands, including the SET option flags that make an atomic
//! set-if-absent-with-expiry (the distributed-lock primitive) possible.

use super::generic::deadline_after_millis;
use super::{check_type, reply_list, usage};
use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::util::strutil::{
    format_g, next_token, parse_double, parse_long, tokens_with_offsets, trim,
};

const SET_USAGE: &str = "SET key value [NX|XX] [EX seconds|PX milliseconds|KEEPTTL]";
const NOT_SET: &str = "NOT_SET\r\n";

/// Reads `key` as an integer, replying with an error and returning
/// `None` if it holds something that isn't one. A missing key is 0,
/// as Redis's INCR family treats it.
fn integer_value(app: &mut App, conn: &mut Conn, key: &str) -> Option<i64> {
    match app.store.get_string(key) {
        None => Some(0),
        Some(current) => match parse_long(&current) {
            Some(v) => Some(v),
            None => {
                conn.reply("ERR value is not an integer\r\n");
                None
            }
        },
    }
}

/// Applies `delta` to `key`'s integer value and replies with the result.
fn apply_integer_delta(app: &mut App, conn: &mut Conn, key: &str, delta: i64) {
    if !check_type(app, conn, key, StoreType::String) {
        return;
    }
    let Some(value) = integer_value(app, conn, key) else {
        return;
    };
    match value.checked_add(delta) {
        None => conn.reply("ERR increment or decrement would overflow\r\n"),
        Some(updated) => {
            app.store.update_string(key, &updated.to_string());
            conn.reply(&format!("VALUE {}\r\n", updated));
        }
    }
}

/// The expiry an option flag asks for: a deadline, no change, or an
/// explicit clear.
enum Expiry {
    Keep,
    Clear,
    After(i64), // milliseconds from now
}

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        "SET" => set(app, conn, rest),

        "SETNX" => {
            let key = next_token(&mut rest);
            let value = rest;
            let Some(key) = key.filter(|_| !value.is_empty()) else {
                return usage(conn, "SETNX key value");
            };
            if app.store.exists(key) {
                return conn.reply(NOT_SET);
            }
            app.store.set_string(key, value);
            conn.reply("OK\r\n");
        }

        "SETEX" | "PSETEX" => {
            let unit = if cmd == "SETEX" {
                "seconds"
            } else {
                "milliseconds"
            };
            let key = next_token(&mut rest);
            let amount = next_token(&mut rest).and_then(parse_long);
            let value = rest;
            let (key, amount) = match (key, amount) {
                (Some(k), Some(n)) if !value.is_empty() => (k, n),
                _ => return usage(conn, &format!("{} key {} value", cmd, unit)),
            };
            if amount <= 0 {
                return conn.reply("ERR invalid expire time\r\n");
            }
            let millis = if cmd == "SETEX" {
                amount.saturating_mul(1000)
            } else {
                amount
            };
            app.store.set_string(key, value);
            app.store
                .set_expire_at(key, Some(deadline_after_millis(millis)));
            conn.reply("OK\r\n");
        }

        "GET" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "GET key");
            }
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            match app.store.get_string(key) {
                Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "GETSET" => {
            let key = next_token(&mut rest);
            let value = rest;
            let Some(key) = key.filter(|_| !value.is_empty()) else {
                return usage(conn, "GETSET key value");
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let previous = app.store.get_string(key);
            app.store.set_string(key, value);
            match previous {
                Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "GETDEL" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "GETDEL key");
            }
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            match app.store.get_string(key) {
                Some(v) => {
                    app.store.del(key);
                    conn.reply(&format!("VALUE {}\r\n", v));
                }
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "GETEX" => getex(app, conn, rest),

        "MGET" => {
            let mut lines = Vec::new();
            while let Some(key) = next_token(&mut rest) {
                // A non-string key reads as missing rather than aborting
                // the whole reply, mirroring Redis's per-key nil.
                let line = match app.store.peek_type(key) {
                    Some(StoreType::String) => app
                        .store
                        .get_string(key)
                        .map(|v| format!("VALUE {}", v))
                        .unwrap_or_else(|| "NOT_FOUND".to_string()),
                    _ => "NOT_FOUND".to_string(),
                };
                lines.push(line);
            }
            if lines.is_empty() {
                return usage(conn, "MGET key [key ...]");
            }
            reply_list(conn, lines);
        }

        "MSET" => {
            // Values here are single tokens, unlike SET's rest-of-line
            // value - there is no other way to tell pairs apart.
            let mut pairs: Vec<(&str, &str)> = Vec::new();
            while let Some(key) = next_token(&mut rest) {
                match next_token(&mut rest) {
                    Some(value) => pairs.push((key, value)),
                    None => return usage(conn, "MSET key value [key value ...]"),
                }
            }
            if pairs.is_empty() {
                return usage(conn, "MSET key value [key value ...]");
            }
            for (key, value) in pairs {
                app.store.set_string(key, value);
            }
            conn.reply("OK\r\n");
        }

        "INCR" | "DECR" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, &format!("{} key", cmd));
            }
            apply_integer_delta(app, conn, key, if cmd == "INCR" { 1 } else { -1 });
        }

        "INCRBY" | "DECRBY" => {
            let key = next_token(&mut rest);
            let amount = parse_long(trim(rest));
            let (key, amount) = match (key, amount) {
                (Some(k), Some(n)) => (k, n),
                _ => return usage(conn, &format!("{} key increment", cmd)),
            };
            let delta = if cmd == "INCRBY" {
                Some(amount)
            } else {
                amount.checked_neg()
            };
            match delta {
                Some(d) => apply_integer_delta(app, conn, key, d),
                None => conn.reply("ERR increment or decrement would overflow\r\n"),
            }
        }

        "INCRBYFLOAT" => {
            let key = next_token(&mut rest);
            let amount = parse_double(trim(rest));
            let (key, amount) = match (key, amount) {
                (Some(k), Some(n)) => (k, n),
                _ => return usage(conn, "INCRBYFLOAT key increment"),
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = match app.store.get_string(key) {
                None => 0.0,
                Some(v) => match parse_double(&v) {
                    Some(f) => f,
                    None => return conn.reply("ERR value is not a float\r\n"),
                },
            };
            let updated = current + amount;
            if !updated.is_finite() {
                return conn.reply("ERR increment would produce NaN or Infinity\r\n");
            }
            // 17 significant digits round-trips an f64 exactly, so the
            // stored text and the in-memory value never drift apart.
            let text = format_g(updated, 17);
            app.store.update_string(key, &text);
            conn.reply(&format!("VALUE {}\r\n", text));
        }

        "APPEND" => {
            let key = next_token(&mut rest);
            let value = rest;
            let Some(key) = key.filter(|_| !value.is_empty()) else {
                return usage(conn, "APPEND key value");
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = app.store.get_string(key).unwrap_or_default();
            let new_len = current.len() + value.len();
            if new_len > app.config.max_string_bytes {
                return conn.reply("ERR resulting string too long\r\n");
            }
            app.store.update_string(key, &(current + value));
            conn.reply(&format!("LEN {}\r\n", new_len));
        }

        "STRLEN" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "STRLEN key");
            }
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let len = app.store.get_string(key).map_or(0, |v| v.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "GETRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let end = parse_long(trim(rest));
            let (key, start, end) = match (key, start, end) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => return usage(conn, "GETRANGE key start end"),
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let value = app.store.get_string(key).unwrap_or_default();
            let len = value.len() as i64;

            let mut start = if start < 0 { start + len } else { start };
            let mut end = if end < 0 { end + len } else { end };
            if start < 0 {
                start = 0;
            }
            if end >= len {
                end = len - 1;
            }

            if len == 0 || start > end || start >= len {
                return conn.reply("VALUE \r\n");
            }
            conn.reply(&format!(
                "VALUE {}\r\n",
                &value[start as usize..=end as usize]
            ));
        }

        "SETRANGE" => {
            let key = next_token(&mut rest);
            let offset = next_token(&mut rest).and_then(parse_long);
            let value = rest;
            let (key, offset) = match (key, offset) {
                (Some(k), Some(o)) if o >= 0 && !value.is_empty() => (k, o),
                _ => return usage(conn, "SETRANGE key offset value"),
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = app.store.get_string(key).unwrap_or_default();
            let old_len = current.len();
            let add_len = value.len();
            let offset = offset as usize;
            let new_len = (offset + add_len).max(old_len);
            if new_len > app.config.max_string_bytes {
                return conn.reply("ERR resulting string too long\r\n");
            }

            let mut buf = vec![b' '; new_len];
            buf[..old_len].copy_from_slice(current.as_bytes());
            buf[offset..offset + add_len].copy_from_slice(value.as_bytes());
            let combined = String::from_utf8(buf).unwrap();
            app.store.update_string(key, &combined);
            conn.reply(&format!("LEN {}\r\n", new_len));
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

/// The option flags a SET call carries.
#[derive(Default)]
struct SetOptions {
    only_if_absent: bool,
    only_if_present: bool,
    keep_ttl: bool,
    expire_millis: Option<i64>,
}

/// Reads a complete option list from `tokens`, or `None` if they don't
/// all parse - which is how a trailing run of words gets ruled out as
/// flags and left as part of the value instead.
fn parse_set_options(tokens: &[&str]) -> Option<SetOptions> {
    let mut options = SetOptions::default();
    let mut i = 0;
    while i < tokens.len() {
        match tokens[i].to_ascii_uppercase().as_str() {
            "NX" => options.only_if_absent = true,
            "XX" => options.only_if_present = true,
            "KEEPTTL" => options.keep_ttl = true,
            unit @ ("EX" | "PX") => {
                let amount = tokens.get(i + 1).and_then(|t| parse_long(t))?;
                options.expire_millis = Some(if unit == "EX" {
                    amount.saturating_mul(1000)
                } else {
                    amount
                });
                i += 1;
            }
            _ => return None,
        }
        i += 1;
    }
    Some(options)
}

/// `SET key value [NX|XX] [EX seconds|PX milliseconds|KEEPTTL]`
///
/// The value is still the rest of the line, so it may contain spaces.
/// To keep Redis's argument order anyway, the trailing tokens are
/// checked against the option grammar: the longest suffix that parses
/// as a *complete* option list becomes the flags, and everything before
/// it is the value. At least one token always stays behind as the
/// value, so `SET key NX` still stores the literal string `NX`.
///
/// The unavoidable cost of a protocol without argument boundaries: a
/// value whose last words happen to spell valid options (`SET k done
/// XX`) loses them to the parser. Write those with SETEX/SETNX, or wait
/// for the RESP rewrite.
fn set(app: &mut App, conn: &mut Conn, rest: &str) {
    let mut head = rest;
    let Some(key) = next_token(&mut head) else {
        return usage(conn, SET_USAGE);
    };

    let tokens = tokens_with_offsets(head);
    if tokens.is_empty() {
        return usage(conn, SET_USAGE);
    }

    // Try the longest flag suffix first, stopping before the last
    // possible split so the value never ends up empty.
    let mut options = SetOptions::default();
    let mut value_end = head.len();
    for split in 1..tokens.len() {
        let candidate: Vec<&str> = tokens[split..].iter().map(|(_, t)| *t).collect();
        if let Some(parsed) = parse_set_options(&candidate) {
            options = parsed;
            value_end = tokens[split].0;
            break;
        }
    }

    let value = trim(&head[..value_end]);
    if value.is_empty() {
        return usage(conn, SET_USAGE);
    }
    if options.only_if_absent && options.only_if_present {
        return conn.reply("ERR NX and XX are mutually exclusive\r\n");
    }
    if options.keep_ttl && options.expire_millis.is_some() {
        return conn.reply("ERR KEEPTTL cannot be combined with EX or PX\r\n");
    }
    if options.expire_millis.is_some_and(|ms| ms <= 0) {
        return conn.reply("ERR invalid expire time\r\n");
    }

    let exists = app.store.exists(key);
    if (options.only_if_absent && exists) || (options.only_if_present && !exists) {
        return conn.reply(NOT_SET);
    }

    if options.keep_ttl {
        app.store.update_string(key, value); // in-place, so the TTL survives
    } else {
        app.store.set_string(key, value);
    }
    if let Some(millis) = options.expire_millis {
        app.store
            .set_expire_at(key, Some(deadline_after_millis(millis)));
    }
    conn.reply("OK\r\n");
}

/// `GETEX key [EX seconds|PX milliseconds|PERSIST]` - read a value and
/// adjust its TTL in one step.
fn getex(app: &mut App, conn: &mut Conn, rest: &str) {
    const GETEX_USAGE: &str = "GETEX key [EX seconds|PX milliseconds|PERSIST]";
    let mut rest = rest;
    let Some(key) = next_token(&mut rest) else {
        return usage(conn, GETEX_USAGE);
    };

    let expiry = match next_token(&mut rest) {
        None => Expiry::Keep,
        Some(token) => match token.to_ascii_uppercase().as_str() {
            "PERSIST" => Expiry::Clear,
            unit @ ("EX" | "PX") => match next_token(&mut rest).and_then(parse_long) {
                Some(n) if n > 0 => Expiry::After(if unit == "EX" {
                    n.saturating_mul(1000)
                } else {
                    n
                }),
                Some(_) => return conn.reply("ERR invalid expire time\r\n"),
                None => return usage(conn, GETEX_USAGE),
            },
            _ => return usage(conn, GETEX_USAGE),
        },
    };

    if !check_type(app, conn, key, StoreType::String) {
        return;
    }
    let Some(value) = app.store.get_string(key) else {
        return conn.reply("NOT_FOUND\r\n");
    };
    match expiry {
        Expiry::Keep => {}
        Expiry::Clear => {
            app.store.set_expire_at(key, None);
        }
        Expiry::After(millis) => {
            app.store
                .set_expire_at(key, Some(deadline_after_millis(millis)));
        }
    }
    conn.reply(&format!("VALUE {}\r\n", value));
}
