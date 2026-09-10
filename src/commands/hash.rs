//! Hash commands.

use super::{check_type, remaining_tokens, reply_list, reply_ok_or_missing, usage};
use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::util::strutil::{format_g, next_token, parse_double, parse_long, trim};

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        // HSET keeps its rest-of-line value (so hash values may contain
        // spaces); HMSET is the variadic form, with single-token values.
        "HSET" | "HSETNX" => {
            let key = next_token(&mut rest);
            let field = next_token(&mut rest);
            let value = rest;
            let (key, field) = match (key, field) {
                (Some(k), Some(f)) if !value.is_empty() => (k, f),
                _ => return usage(conn, &format!("{} key field value", cmd)),
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let h = app.store.get_or_create_hash(key).expect("type checked");
            if cmd == "HSETNX" && h.contains_key(field) {
                return conn.reply("FALSE\r\n");
            }
            h.insert(field.to_string(), value.to_string());
            conn.reply("OK\r\n");
        }

        "HMSET" => {
            let key = next_token(&mut rest);
            let tokens = remaining_tokens(&mut rest);
            let (key, tokens) = match key {
                Some(k) if !tokens.is_empty() && tokens.len().is_multiple_of(2) => (k, tokens),
                _ => return usage(conn, "HMSET key field value [field value ...]"),
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let h = app.store.get_or_create_hash(key).expect("type checked");
            for pair in tokens.chunks(2) {
                h.insert(pair[0].to_string(), pair[1].to_string());
            }
            conn.reply("OK\r\n");
        }

        "HGET" => {
            let key = next_token(&mut rest);
            let field = trim(rest);
            let Some(key) = key.filter(|_| !field.is_empty()) else {
                return usage(conn, "HGET key field");
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let value = app
                .store
                .get_existing_hash(key)
                .and_then(|h| h.get(field).cloned());
            match value {
                Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "HMGET" => {
            let key = next_token(&mut rest);
            let fields = remaining_tokens(&mut rest);
            let (key, fields) = match key {
                Some(k) if !fields.is_empty() => (k, fields),
                _ => return usage(conn, "HMGET key field [field ...]"),
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let hash = app.store.get_existing_hash(key);
            let lines: Vec<String> = fields
                .iter()
                .map(|field| match hash.as_ref().and_then(|h| h.get(*field)) {
                    Some(v) => format!("VALUE {}", v),
                    None => "NOT_FOUND".to_string(),
                })
                .collect();
            reply_list(conn, lines);
        }

        "HDEL" => {
            let key = next_token(&mut rest);
            let fields = remaining_tokens(&mut rest);
            let (key, fields) = match key {
                Some(k) if !fields.is_empty() => (k, fields),
                _ => return usage(conn, "HDEL key field [field ...]"),
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let removed = match app.store.get_existing_hash(key) {
                Some(h) => fields.iter().filter(|f| h.remove(**f).is_some()).count(),
                None => 0,
            };
            app.store.delete_if_empty(key);
            if fields.len() == 1 {
                reply_ok_or_missing(conn, removed == 1);
            } else {
                conn.reply(&format!("DELETED {}\r\n", removed));
            }
        }

        "HLEN" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "HLEN key");
            }
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let len = app.store.get_existing_hash(key).map_or(0, |h| h.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "HEXISTS" => {
            let key = next_token(&mut rest);
            let field = trim(rest);
            let Some(key) = key.filter(|_| !field.is_empty()) else {
                return usage(conn, "HEXISTS key field");
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let present = app
                .store
                .get_existing_hash(key)
                .is_some_and(|h| h.contains_key(field));
            conn.reply(if present { "TRUE\r\n" } else { "FALSE\r\n" });
        }

        "HSTRLEN" => {
            let key = next_token(&mut rest);
            let field = trim(rest);
            let Some(key) = key.filter(|_| !field.is_empty()) else {
                return usage(conn, "HSTRLEN key field");
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let len = app
                .store
                .get_existing_hash(key)
                .and_then(|h| h.get(field).map(|v| v.len()))
                .unwrap_or(0);
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "HKEYS" | "HVALS" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, &format!("{} key", cmd));
            }
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let wants_keys = cmd == "HKEYS";
            let items: Vec<String> = app
                .store
                .get_existing_hash(key)
                .map(|h| {
                    h.iter()
                        .map(|(f, v)| if wants_keys { f.clone() } else { v.clone() })
                        .collect()
                })
                .unwrap_or_default();
            reply_list(conn, items);
        }

        "HGETALL" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "HGETALL key");
            }
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            if let Some(hash) = app.store.get_existing_hash(key) {
                let pairs: Vec<(String, String)> =
                    hash.iter().map(|(f, v)| (f.clone(), v.clone())).collect();
                for (field, value) in pairs {
                    conn.reply(&format!("{}\r\n{}\r\n", field, value));
                }
            }
            conn.reply("END\r\n");
        }

        "HINCRBY" | "HINCRBYFLOAT" => {
            let key = next_token(&mut rest);
            let field = next_token(&mut rest);
            let increment = trim(rest);
            let (key, field) = match (key, field) {
                (Some(k), Some(f)) if !increment.is_empty() => (k, f),
                _ => return usage(conn, &format!("{} key field increment", cmd)),
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let current = app
                .store
                .get_existing_hash(key)
                .and_then(|h| h.get(field).cloned());

            let updated = if cmd == "HINCRBY" {
                let Some(delta) = parse_long(increment) else {
                    return usage(conn, "HINCRBY key field increment");
                };
                let base = match current {
                    None => 0,
                    Some(v) => match parse_long(&v) {
                        Some(n) => n,
                        None => return conn.reply("ERR hash value is not an integer\r\n"),
                    },
                };
                match base.checked_add(delta) {
                    Some(n) => n.to_string(),
                    None => return conn.reply("ERR increment or decrement would overflow\r\n"),
                }
            } else {
                let Some(delta) = parse_double(increment) else {
                    return usage(conn, "HINCRBYFLOAT key field increment");
                };
                let base = match current {
                    None => 0.0,
                    Some(v) => match parse_double(&v) {
                        Some(f) => f,
                        None => return conn.reply("ERR hash value is not a float\r\n"),
                    },
                };
                let sum = base + delta;
                if !sum.is_finite() {
                    return conn.reply("ERR increment would produce NaN or Infinity\r\n");
                }
                format_g(sum, 17)
            };

            app.store
                .get_or_create_hash(key)
                .expect("type checked")
                .insert(field.to_string(), updated.clone());
            conn.reply(&format!("VALUE {}\r\n", updated));
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}
