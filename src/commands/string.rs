//! String commands. With RESP's argument boundaries, `SET`'s options
//! sit after the value exactly as Redis documents them - the old line
//! protocol had to guess where the value ended.

use super::generic::deadline_after_millis;
use super::{
    check_type, exact_args, min_args, not_a_float, parse_float, parse_int, syntax_error, Checked,
};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::util::bytes::{eq_ignore_case, format_f64, parse_f64, parse_i64, Bytes};

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "SET" => set(app, argv),

        "SETNX" => {
            exact_args(argv, name, 2)?;
            if app.store.exists(&argv[1]) {
                return Ok(Reply::bool(false));
            }
            app.store.set_string(&argv[1], &argv[2]);
            Ok(Reply::bool(true))
        }

        "SETEX" | "PSETEX" => {
            exact_args(argv, name, 3)?;
            let amount = parse_int(&argv[2])?;
            if amount <= 0 {
                return Ok(Reply::error(format!(
                    "ERR invalid expire time in '{}' command",
                    name.to_ascii_lowercase()
                )));
            }
            let millis = if name == "SETEX" {
                amount.saturating_mul(1000)
            } else {
                amount
            };
            app.store.set_string(&argv[1], &argv[3]);
            app.store
                .set_expire_at(&argv[1], Some(deadline_after_millis(millis)));
            Ok(Reply::ok())
        }

        "GET" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::String)?;
            Ok(bulk_or_nil(app.store.get_string(&argv[1])))
        }

        "GETSET" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::String)?;
            let previous = app.store.get_string(&argv[1]);
            app.store.set_string(&argv[1], &argv[2]);
            Ok(bulk_or_nil(previous))
        }

        "GETDEL" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::String)?;
            let value = app.store.get_string(&argv[1]);
            if value.is_some() {
                app.store.del(&argv[1]);
            }
            Ok(bulk_or_nil(value))
        }

        "GETEX" => getex(app, argv),

        "MGET" => {
            min_args(argv, name, 1)?;
            // A non-string key reads as nil rather than aborting the
            // whole reply, mirroring Redis.
            let values = argv[1..]
                .iter()
                .map(|key| match app.store.peek_type(key) {
                    Some(StoreType::String) => bulk_or_nil(app.store.get_string(key)),
                    _ => Reply::Nil,
                })
                .collect();
            Ok(Reply::array(values))
        }

        "MSET" | "MSETNX" => {
            min_args(argv, name, 2)?;
            if !argv[1..].len().is_multiple_of(2) {
                return Err(Reply::wrong_arity(name));
            }
            let pairs: Vec<&[Bytes]> = argv[1..].chunks(2).collect();
            if name == "MSETNX" {
                // All or nothing, as Redis defines it.
                if pairs.iter().any(|pair| app.store.exists(&pair[0])) {
                    return Ok(Reply::bool(false));
                }
            }
            for pair in pairs {
                app.store.set_string(&pair[0], &pair[1]);
            }
            Ok(if name == "MSETNX" {
                Reply::bool(true)
            } else {
                Reply::ok()
            })
        }

        "INCR" | "DECR" => {
            exact_args(argv, name, 1)?;
            apply_integer_delta(app, &argv[1], if name == "INCR" { 1 } else { -1 })
        }

        "INCRBY" | "DECRBY" => {
            exact_args(argv, name, 2)?;
            let amount = parse_int(&argv[2])?;
            let delta = if name == "INCRBY" {
                Some(amount)
            } else {
                amount.checked_neg()
            };
            match delta {
                Some(d) => apply_integer_delta(app, &argv[1], d),
                None => Ok(Reply::error("ERR increment or decrement would overflow")),
            }
        }

        "INCRBYFLOAT" => {
            exact_args(argv, name, 2)?;
            let amount = parse_float(&argv[2])?;
            check_type(app, &argv[1], StoreType::String)?;
            let current = match app.store.get_string(&argv[1]) {
                None => 0.0,
                Some(v) => parse_f64(&v).ok_or_else(not_a_float)?,
            };
            let updated = current + amount;
            if !updated.is_finite() {
                return Ok(Reply::error("ERR increment would produce NaN or Infinity"));
            }
            let text = format_f64(updated);
            app.store.update_string(&argv[1], &text);
            Ok(Reply::Bulk(text))
        }

        "APPEND" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::String)?;
            let mut value = app.store.get_string(&argv[1]).unwrap_or_default();
            if value.len() + argv[2].len() > app.config.max_string_bytes {
                return Ok(Reply::error("ERR string exceeds maximum allowed size"));
            }
            value.extend_from_slice(&argv[2]);
            let len = value.len();
            app.store.update_string(&argv[1], &value);
            Ok(Reply::Integer(len as i64))
        }

        "STRLEN" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::String)?;
            let len = app.store.get_string(&argv[1]).map_or(0, |v| v.len());
            Ok(Reply::Integer(len as i64))
        }

        "GETRANGE" | "SUBSTR" => {
            exact_args(argv, name, 3)?;
            let (start, end) = (parse_int(&argv[2])?, parse_int(&argv[3])?);
            check_type(app, &argv[1], StoreType::String)?;
            let value = app.store.get_string(&argv[1]).unwrap_or_default();
            Ok(Reply::Bulk(substring(&value, start, end)))
        }

        "SETRANGE" => {
            exact_args(argv, name, 3)?;
            let offset = parse_int(&argv[2])?;
            if offset < 0 {
                return Ok(Reply::error("ERR offset is out of range"));
            }
            check_type(app, &argv[1], StoreType::String)?;
            let offset = offset as usize;
            let patch = &argv[3];
            let mut value = app.store.get_string(&argv[1]).unwrap_or_default();
            if patch.is_empty() {
                return Ok(Reply::Integer(value.len() as i64));
            }
            if offset + patch.len() > app.config.max_string_bytes {
                return Ok(Reply::error("ERR string exceeds maximum allowed size"));
            }
            // Redis pads the gap with NUL bytes, which raw byte values
            // can now represent - the old text protocol used spaces.
            if value.len() < offset + patch.len() {
                value.resize(offset + patch.len(), 0);
            }
            value[offset..offset + patch.len()].copy_from_slice(patch);
            let len = value.len();
            app.store.update_string(&argv[1], &value);
            Ok(Reply::Integer(len as i64))
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}

fn bulk_or_nil(value: Option<Bytes>) -> Reply {
    match value {
        Some(v) => Reply::Bulk(v),
        None => Reply::Nil,
    }
}

/// The inclusive `[start, end]` slice Redis's GETRANGE returns, with
/// negative indices counting from the end.
fn substring(value: &[u8], start: i64, end: i64) -> Bytes {
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
        return Vec::new();
    }
    value[start as usize..=end as usize].to_vec()
}

/// Applies `delta` to `key`'s integer value and replies with the result.
fn apply_integer_delta(app: &mut App, key: &[u8], delta: i64) -> Checked<Reply> {
    check_type(app, key, StoreType::String)?;
    let value = match app.store.get_string(key) {
        None => 0,
        Some(current) => parse_i64(&current).ok_or_else(super::not_an_integer)?,
    };
    match value.checked_add(delta) {
        None => Ok(Reply::error("ERR increment or decrement would overflow")),
        Some(updated) => {
            app.store.update_string(key, updated.to_string().as_bytes());
            Ok(Reply::Integer(updated))
        }
    }
}

/// The expiry an option flag asks for.
enum Expiry {
    Keep,
    Clear,
    At(std::time::SystemTime),
}

/// `SET key value [NX|XX] [GET] [EX s|PX ms|EXAT ts|PXAT ts|KEEPTTL]`
fn set(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "SET", 2)?;
    let (key, value) = (&argv[1], &argv[2]);

    let mut only_if_absent = false;
    let mut only_if_present = false;
    let mut return_old = false;
    let mut expiry = Expiry::Clear;

    let mut i = 3;
    while i < argv.len() {
        let option = &argv[i];
        let takes_argument = |unit_millis: i64, absolute: bool| -> Checked<Expiry> {
            if i + 1 >= argv.len() {
                return Err(syntax_error());
            }
            let amount = parse_int(&argv[i + 1])?;
            if !absolute && amount <= 0 {
                return Err(Reply::error("ERR invalid expire time in 'set' command"));
            }
            let millis = amount.saturating_mul(unit_millis);
            Ok(Expiry::At(if absolute {
                std::time::UNIX_EPOCH + std::time::Duration::from_millis(millis.max(0) as u64)
            } else {
                deadline_after_millis(millis)
            }))
        };

        if eq_ignore_case(option, "NX") {
            only_if_absent = true;
        } else if eq_ignore_case(option, "XX") {
            only_if_present = true;
        } else if eq_ignore_case(option, "GET") {
            return_old = true;
        } else if eq_ignore_case(option, "KEEPTTL") {
            expiry = Expiry::Keep;
        } else if eq_ignore_case(option, "EX") {
            expiry = takes_argument(1000, false)?;
            i += 1;
        } else if eq_ignore_case(option, "PX") {
            expiry = takes_argument(1, false)?;
            i += 1;
        } else if eq_ignore_case(option, "EXAT") {
            expiry = takes_argument(1000, true)?;
            i += 1;
        } else if eq_ignore_case(option, "PXAT") {
            expiry = takes_argument(1, true)?;
            i += 1;
        } else {
            return Err(syntax_error());
        }
        i += 1;
    }

    if only_if_absent && only_if_present {
        return Err(syntax_error());
    }

    // SET ... GET reports the previous value, which must be a string.
    let previous = if return_old {
        check_type(app, key, StoreType::String)?;
        app.store.get_string(key)
    } else {
        None
    };

    let exists = app.store.exists(key);
    if (only_if_absent && exists) || (only_if_present && !exists) {
        return Ok(if return_old {
            bulk_or_nil(previous)
        } else {
            Reply::Nil
        });
    }

    match expiry {
        // An in-place update, so the TTL survives.
        Expiry::Keep => app.store.update_string(key, value),
        _ => app.store.set_string(key, value),
    }
    if let Expiry::At(deadline) = expiry {
        app.store.set_expire_at(key, Some(deadline));
    }

    Ok(if return_old {
        bulk_or_nil(previous)
    } else {
        Reply::ok()
    })
}

/// `GETEX key [EX seconds|PX milliseconds|PERSIST]`
fn getex(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "GETEX", 1)?;
    let key = &argv[1];

    let expiry = match argv.len() {
        2 => Expiry::Keep,
        3 if eq_ignore_case(&argv[2], "PERSIST") => Expiry::Clear,
        4 if eq_ignore_case(&argv[2], "EX") || eq_ignore_case(&argv[2], "PX") => {
            let amount = parse_int(&argv[3])?;
            if amount <= 0 {
                return Ok(Reply::error("ERR invalid expire time in 'getex' command"));
            }
            let millis = if eq_ignore_case(&argv[2], "EX") {
                amount.saturating_mul(1000)
            } else {
                amount
            };
            Expiry::At(deadline_after_millis(millis))
        }
        _ => return Err(syntax_error()),
    };

    check_type(app, key, StoreType::String)?;
    let Some(value) = app.store.get_string(key) else {
        return Ok(Reply::Nil);
    };
    match expiry {
        Expiry::Keep => {}
        Expiry::Clear => {
            app.store.set_expire_at(key, None);
        }
        Expiry::At(deadline) => {
            app.store.set_expire_at(key, Some(deadline));
        }
    }
    Ok(Reply::Bulk(value))
}
