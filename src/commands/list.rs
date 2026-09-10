//! List commands.

use super::{check_type, remaining_tokens, reply_list, usage};
use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::types::list;
use crate::util::strutil::{next_token, parse_long, trim};

/// Which end of a list an operation works on.
#[derive(Clone, Copy, PartialEq)]
enum End {
    Left,
    Right,
}

impl End {
    fn parse(token: &str) -> Option<End> {
        match token.to_ascii_uppercase().as_str() {
            "LEFT" => Some(End::Left),
            "RIGHT" => Some(End::Right),
            _ => None,
        }
    }
}

fn pop_from(l: &mut list::List, end: End) -> Option<String> {
    match end {
        End::Left => l.pop_front(),
        End::Right => l.pop_back(),
    }
}

fn push_to(l: &mut list::List, end: End, value: String) {
    match end {
        End::Left => l.push_front(value),
        End::Right => l.push_back(value),
    }
}

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" => {
            let only_if_exists = cmd.ends_with('X');
            let end = if cmd.starts_with('L') {
                End::Left
            } else {
                End::Right
            };
            let key = next_token(&mut rest);
            let values = remaining_tokens(&mut rest);
            let (key, values) = match key {
                Some(k) if !values.is_empty() => (k, values),
                _ => return usage(conn, &format!("{} key value [value ...]", cmd)),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            if only_if_exists && !app.store.exists(key) {
                return conn.reply("NOT_FOUND\r\n");
            }
            let l = app.store.get_or_create_list(key).expect("type checked");
            for value in values {
                push_to(l, end, value.to_string());
            }
            conn.reply(&format!("LEN {}\r\n", l.len()));
        }

        "LPOP" | "RPOP" => {
            let end = if cmd == "LPOP" { End::Left } else { End::Right };
            let key = next_token(&mut rest);
            let count = match trim(rest) {
                "" => None,
                text => match parse_long(text).filter(|&c| c >= 0) {
                    Some(c) => Some(c as usize),
                    None => return usage(conn, &format!("{} key [count]", cmd)),
                },
            };
            let Some(key) = key else {
                return usage(conn, &format!("{} key [count]", cmd));
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }

            // Without a count this replies with a single value, the
            // shape it has always had; with one it replies as a list.
            let wanted = count.unwrap_or(1);
            let mut popped = Vec::new();
            if let Some(l) = app.store.get_existing_list(key) {
                for _ in 0..wanted {
                    match pop_from(l, end) {
                        Some(v) => popped.push(v),
                        None => break,
                    }
                }
            }
            app.store.delete_if_empty(key);

            match count {
                Some(_) => reply_list(conn, popped),
                None => match popped.first() {
                    Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                    None => conn.reply("NOT_FOUND\r\n"),
                },
            }
        }

        "LLEN" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "LLEN key");
            }
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let len = app.store.get_existing_list(key).map_or(0, |l| l.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "LRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let stop = parse_long(trim(rest));
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => return usage(conn, "LRANGE key start stop"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let values: Vec<String> = app
                .store
                .get_existing_list(key)
                .map(|l| {
                    list::range(l, start, stop)
                        .into_iter()
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            reply_list(conn, values);
        }

        "LINDEX" => {
            let key = next_token(&mut rest);
            let index = parse_long(trim(rest));
            let (key, index) = match (key, index) {
                (Some(k), Some(i)) => (k, i),
                _ => return usage(conn, "LINDEX key index"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let value = app
                .store
                .get_existing_list(key)
                .and_then(|l| list::resolve_index(l.len(), index).and_then(|i| l.get(i).cloned()));
            match value {
                Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "LSET" => {
            let key = next_token(&mut rest);
            let index = next_token(&mut rest).and_then(parse_long);
            let value = rest;
            let (key, index) = match (key, index) {
                (Some(k), Some(i)) if !value.is_empty() => (k, i),
                _ => return usage(conn, "LSET key index value"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let updated = app
                .store
                .get_existing_list(key)
                .and_then(|l| list::resolve_index(l.len(), index).map(|i| l[i] = value.to_string()))
                .is_some();
            // NOT_FOUND covers a missing key and an out-of-range index.
            super::reply_ok_or_missing(conn, updated);
        }

        "LINSERT" => {
            let key = next_token(&mut rest);
            let position = next_token(&mut rest);
            let pivot = next_token(&mut rest);
            let value = rest;
            let (key, position, pivot) = match (key, position, pivot) {
                (Some(k), Some(p), Some(v)) if !value.is_empty() => (k, p, v),
                _ => return usage(conn, "LINSERT key BEFORE|AFTER pivot value"),
            };
            let before = match position.to_ascii_uppercase().as_str() {
                "BEFORE" => true,
                "AFTER" => false,
                _ => return usage(conn, "LINSERT key BEFORE|AFTER pivot value"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let new_len = app
                .store
                .get_existing_list(key)
                .and_then(|l| list::insert(l, before, pivot, value));
            match new_len {
                Some(len) => conn.reply(&format!("LEN {}\r\n", len)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "LREM" => {
            let key = next_token(&mut rest);
            let count = next_token(&mut rest).and_then(parse_long);
            let value = rest;
            let (key, count) = match (key, count) {
                (Some(k), Some(c)) if !value.is_empty() => (k, c),
                _ => return usage(conn, "LREM key count value"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let removed = app
                .store
                .get_existing_list(key)
                .map_or(0, |l| list::remove(l, count, value));
            app.store.delete_if_empty(key);
            conn.reply(&format!("REMOVED {}\r\n", removed));
        }

        "LTRIM" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let stop = parse_long(trim(rest));
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => return usage(conn, "LTRIM key start stop"),
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            if let Some(l) = app.store.get_existing_list(key) {
                list::trim(l, start, stop);
            }
            app.store.delete_if_empty(key);
            conn.reply("OK\r\n");
        }

        "RPOPLPUSH" | "LMOVE" => {
            let source = next_token(&mut rest);
            let destination = next_token(&mut rest);
            let (source, destination) = match (source, destination) {
                (Some(s), Some(d)) => (s, d),
                _ => return usage(conn, move_usage(cmd)),
            };
            // RPOPLPUSH is LMOVE with the ends fixed.
            let (from, to) = if cmd == "RPOPLPUSH" {
                (End::Right, End::Left)
            } else {
                match (
                    next_token(&mut rest).and_then(End::parse),
                    next_token(&mut rest).and_then(End::parse),
                ) {
                    (Some(f), Some(t)) => (f, t),
                    _ => return usage(conn, move_usage(cmd)),
                }
            };
            if !check_type(app, conn, source, StoreType::List)
                || !check_type(app, conn, destination, StoreType::List)
            {
                return;
            }

            let Some(value) = app
                .store
                .get_existing_list(source)
                .and_then(|l| pop_from(l, from))
            else {
                return conn.reply("NOT_FOUND\r\n");
            };
            // Rotating a list onto itself is legal, so push before the
            // empty-cleanup runs - otherwise a one-element self-move
            // would delete the key it is about to write back into.
            let target = app
                .store
                .get_or_create_list(destination)
                .expect("type checked");
            push_to(target, to, value.clone());
            app.store.delete_if_empty(source);
            conn.reply(&format!("VALUE {}\r\n", value));
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

fn move_usage(cmd: &str) -> &'static str {
    if cmd == "RPOPLPUSH" {
        "RPOPLPUSH source destination"
    } else {
        "LMOVE source destination LEFT|RIGHT LEFT|RIGHT"
    }
}
