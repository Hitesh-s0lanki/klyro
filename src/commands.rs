//! Command line parsing and dispatch.

use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::types::list;
use crate::util::glob::glob_match;
use crate::util::strutil::{next_token, parse_double, parse_int, parse_long, trim};

const MAX_ZADD_PAIRS: usize = 128;
const MAX_STRING_LEN: usize = 64 * 1024; // matches the network protocol's line-length cap

const WRONGTYPE: &str = "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n";

/// Replies WRONGTYPE and returns `false` if `key` exists with a type
/// other than `want`; otherwise (missing, or already the right type)
/// returns `true` and replies nothing.
fn check_type(app: &mut App, conn: &mut Conn, key: &str, want: StoreType) -> bool {
    match app.store.type_of(key) {
        Some(t) if t != want => {
            conn.reply(WRONGTYPE);
            false
        }
        _ => true,
    }
}

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

    match cmd.as_str() {
        "PING" => conn.reply("PONG\r\n"),

        // --- generic key commands ---
        "DEL" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: DEL key\r\n");
                return;
            }
            conn.reply(if app.store.del(key) {
                "OK\r\n"
            } else {
                "NOT_FOUND\r\n"
            });
        }

        "EXPIRE" => {
            let key = next_token(&mut rest);
            let secs = parse_int(rest);
            let (key, secs) = match (key, secs) {
                (Some(k), Some(s)) => (k, s),
                _ => {
                    conn.reply("ERR usage: EXPIRE key seconds\r\n");
                    return;
                }
            };
            conn.reply(if app.store.expire(key, secs as i64) {
                "OK\r\n"
            } else {
                "NOT_FOUND\r\n"
            });
        }

        "TTL" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: TTL key\r\n");
                return;
            }
            let ttl = app.store.ttl(key);
            conn.reply(&format!("TTL {}\r\n", ttl));
        }

        "TYPE" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: TYPE key\r\n");
                return;
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
            for key in app.store.foreach_key() {
                if pattern.is_none_or(|p| glob_match(p, &key)) {
                    conn.reply(&format!("{}\r\n", key));
                }
            }
            conn.reply("END\r\n");
        }

        "SCAN" => {
            let cursor_str = next_token(&mut rest);
            let cursor = cursor_str.and_then(parse_long).filter(|&c| c >= 0);
            let cursor = match cursor {
                Some(c) => c as usize,
                None => {
                    conn.reply("ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
                    return;
                }
            };

            let mut pattern: Option<&str> = None;
            let mut count: i64 = 10;
            while let Some(opt) = next_token(&mut rest) {
                match opt.to_ascii_uppercase().as_str() {
                    "MATCH" => match next_token(&mut rest) {
                        Some(p) => pattern = Some(p),
                        None => {
                            conn.reply("ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
                            return;
                        }
                    },
                    "COUNT" => match next_token(&mut rest).and_then(parse_long) {
                        Some(c) if c > 0 => count = c,
                        _ => {
                            conn.reply("ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
                            return;
                        }
                    },
                    _ => {
                        conn.reply("ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
                        return;
                    }
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

        "DBSIZE" => conn.reply(&format!("COUNT {}\r\n", app.store.size())),

        // --- string commands ---
        "SET" => {
            let key = next_token(&mut rest);
            let value = rest;
            let key = match key {
                Some(k) if !value.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: SET key value\r\n");
                    return;
                }
            };
            app.store.set_string(key, value);
            conn.reply("OK\r\n");
        }

        "GET" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: GET key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            match app.store.get_string(key) {
                Some(v) => conn.reply(&format!("VALUE {}\r\n", v)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "INCR" | "DECR" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply(&format!("ERR usage: {} key\r\n", cmd));
                return;
            }
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = app.store.get_string(key);
            let mut value: i64 = 0;
            if let Some(cur) = &current {
                match parse_long(cur) {
                    Some(v) => value = v,
                    None => {
                        conn.reply("ERR value is not an integer\r\n");
                        return;
                    }
                }
            }
            let incr = cmd == "INCR";
            if (incr && value == i64::MAX) || (!incr && value == i64::MIN) {
                conn.reply("ERR increment or decrement would overflow\r\n");
                return;
            }
            value += if incr { 1 } else { -1 };
            app.store.update_string(key, &value.to_string());
            conn.reply(&format!("VALUE {}\r\n", value));
        }

        "APPEND" => {
            let key = next_token(&mut rest);
            let value = rest;
            let key = match key {
                Some(k) if !value.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: APPEND key value\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = app.store.get_string(key).unwrap_or_default();
            let new_len = current.len() + value.len();
            if new_len > MAX_STRING_LEN {
                conn.reply("ERR resulting string too long\r\n");
                return;
            }
            let combined = current + value;
            app.store.update_string(key, &combined);
            conn.reply(&format!("LEN {}\r\n", new_len));
        }

        "GETRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let end = parse_long(rest);
            let (key, start, end) = match (key, start, end) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => {
                    conn.reply("ERR usage: GETRANGE key start end\r\n");
                    return;
                }
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
                conn.reply("VALUE \r\n");
                return;
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
                _ => {
                    conn.reply("ERR usage: SETRANGE key offset value\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::String) {
                return;
            }
            let current = app.store.get_string(key).unwrap_or_default();
            let old_len = current.len();
            let add_len = value.len();
            let offset = offset as usize;
            let new_len = (offset + add_len).max(old_len);
            if new_len > MAX_STRING_LEN {
                conn.reply("ERR resulting string too long\r\n");
                return;
            }

            let mut buf = vec![b' '; new_len];
            buf[..old_len].copy_from_slice(current.as_bytes());
            buf[offset..offset + add_len].copy_from_slice(value.as_bytes());
            let combined = String::from_utf8(buf).unwrap();
            app.store.update_string(key, &combined);
            conn.reply(&format!("LEN {}\r\n", new_len));
        }

        // --- list commands ---
        "LPUSH" | "RPUSH" => {
            let key = next_token(&mut rest);
            let first_value = next_token(&mut rest);
            let (key, first_value) = match (key, first_value) {
                (Some(k), Some(v)) => (k, v),
                _ => {
                    conn.reply(&format!("ERR usage: {} key value [value ...]\r\n", cmd));
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let push_left = cmd == "LPUSH";
            let list = app.store.get_or_create_list(key).unwrap();
            let mut value = Some(first_value);
            while let Some(v) = value {
                if push_left {
                    list.push_front(v.to_string());
                } else {
                    list.push_back(v.to_string());
                }
                value = next_token(&mut rest);
            }
            conn.reply(&format!("LEN {}\r\n", list.len()));
        }

        "LPOP" | "RPOP" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply(&format!("ERR usage: {} key\r\n", cmd));
                return;
            }
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let popped = app.store.get_existing_list(key).and_then(|list| {
                if cmd == "LPOP" {
                    list.pop_front()
                } else {
                    list.pop_back()
                }
            });
            match popped {
                Some(v) => {
                    conn.reply(&format!("VALUE {}\r\n", v));
                    app.store.delete_if_empty(key);
                }
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "LLEN" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: LLEN key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            let len = app.store.get_existing_list(key).map_or(0, |l| l.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "LRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_int);
            let stop = parse_int(rest);
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s as i64, e as i64),
                _ => {
                    conn.reply("ERR usage: LRANGE key start stop\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::List) {
                return;
            }
            if let Some(l) = app.store.get_existing_list(key) {
                for v in list::range(l, start, stop) {
                    conn.reply(&format!("{}\r\n", v));
                }
            }
            conn.reply("END\r\n");
        }

        // --- hash commands ---
        "HSET" => {
            let key = next_token(&mut rest);
            let field = next_token(&mut rest);
            let value = rest;
            let (key, field) = match (key, field) {
                (Some(k), Some(f)) if !value.is_empty() => (k, f),
                _ => {
                    conn.reply("ERR usage: HSET key field value\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let hash = app.store.get_or_create_hash(key).unwrap();
            hash.insert(field.to_string(), value.to_string());
            conn.reply("OK\r\n");
        }

        "HGET" => {
            let key = next_token(&mut rest);
            let field = rest;
            let key = match key {
                Some(k) if !field.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: HGET key field\r\n");
                    return;
                }
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

        "HDEL" => {
            let key = next_token(&mut rest);
            let field = rest;
            let key = match key {
                Some(k) if !field.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: HDEL key field\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let removed = app
                .store
                .get_existing_hash(key)
                .is_some_and(|h| h.remove(field).is_some());
            if removed {
                conn.reply("OK\r\n");
                app.store.delete_if_empty(key);
            } else {
                conn.reply("NOT_FOUND\r\n");
            }
        }

        "HLEN" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: HLEN key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            let len = app.store.get_existing_hash(key).map_or(0, |h| h.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "HGETALL" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: HGETALL key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::Hash) {
                return;
            }
            if let Some(hash) = app.store.get_existing_hash(key) {
                for (field, value) in hash.iter() {
                    conn.reply(&format!("{}\r\n{}\r\n", field, value));
                }
            }
            conn.reply("END\r\n");
        }

        // --- set commands ---
        "SADD" => {
            let key = next_token(&mut rest);
            let first_member = next_token(&mut rest);
            let (key, first_member) = match (key, first_member) {
                (Some(k), Some(m)) => (k, m),
                _ => {
                    conn.reply("ERR usage: SADD key member [member ...]\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let set = app.store.get_or_create_set(key).unwrap();
            let mut added = 0;
            let mut member = Some(first_member);
            while let Some(m) = member {
                if set.insert(m.to_string()) {
                    added += 1;
                }
                member = next_token(&mut rest);
            }
            conn.reply(&format!("ADDED {}\r\n", added));
        }

        "SREM" => {
            let key = next_token(&mut rest);
            let member = rest;
            let key = match key {
                Some(k) if !member.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: SREM key member\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let removed = app
                .store
                .get_existing_set(key)
                .is_some_and(|s| s.remove(member));
            if removed {
                conn.reply("OK\r\n");
                app.store.delete_if_empty(key);
            } else {
                conn.reply("NOT_FOUND\r\n");
            }
        }

        "SISMEMBER" => {
            let key = next_token(&mut rest);
            let member = rest;
            let key = match key {
                Some(k) if !member.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: SISMEMBER key member\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let is_member = app
                .store
                .get_existing_set(key)
                .is_some_and(|s| s.contains(member));
            conn.reply(if is_member { "TRUE\r\n" } else { "FALSE\r\n" });
        }

        "SCARD" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: SCARD key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let len = app.store.get_existing_set(key).map_or(0, |s| s.len());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "SMEMBERS" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: SMEMBERS key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            if let Some(set) = app.store.get_existing_set(key) {
                for m in set.iter() {
                    conn.reply(&format!("{}\r\n", m));
                }
            }
            conn.reply("END\r\n");
        }

        // --- sorted set commands ---
        "ZADD" => {
            let key = next_token(&mut rest);
            let mut pairs: Vec<(&str, &str)> = Vec::new();
            let mut dangling = false;
            while let Some(tok) = next_token(&mut rest) {
                if pairs.len() == MAX_ZADD_PAIRS {
                    conn.reply("ERR too many score/member pairs\r\n");
                    return;
                }
                let member = match next_token(&mut rest) {
                    Some(m) => m,
                    None => {
                        dangling = true;
                        break;
                    }
                };
                pairs.push((tok, member));
            }

            let mut valid = key.is_some() && !pairs.is_empty() && !dangling;
            let mut scores: Vec<f64> = Vec::with_capacity(pairs.len());
            if valid {
                for (score_str, _) in &pairs {
                    match parse_double(score_str) {
                        Some(s) => scores.push(s),
                        None => {
                            valid = false;
                            break;
                        }
                    }
                }
            }
            if !valid {
                conn.reply("ERR usage: ZADD key score member [score member ...]\r\n");
                return;
            }
            let key = key.unwrap();
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let zset = app.store.get_or_create_zset(key).unwrap();
            let mut added = 0;
            for ((_, member), score) in pairs.iter().zip(scores.iter()) {
                if zset.add(member, *score) {
                    added += 1;
                }
            }
            conn.reply(&format!("ADDED {}\r\n", added));
        }

        "ZSCORE" => {
            let key = next_token(&mut rest);
            let member = rest;
            let key = match key {
                Some(k) if !member.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: ZSCORE key member\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let score = app
                .store
                .get_existing_zset(key)
                .and_then(|z| z.score(member));
            match score {
                Some(s) => conn.reply(&format!(
                    "VALUE {}\r\n",
                    crate::util::strutil::format_g(s, 6)
                )),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "ZREM" => {
            let key = next_token(&mut rest);
            let member = rest;
            let key = match key {
                Some(k) if !member.is_empty() => k,
                _ => {
                    conn.reply("ERR usage: ZREM key member\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let removed = app
                .store
                .get_existing_zset(key)
                .is_some_and(|z| z.rem(member));
            if removed {
                conn.reply("OK\r\n");
                app.store.delete_if_empty(key);
            } else {
                conn.reply("NOT_FOUND\r\n");
            }
        }

        "ZCARD" => {
            let key = trim(rest);
            if key.is_empty() {
                conn.reply("ERR usage: ZCARD key\r\n");
                return;
            }
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let len = app.store.get_existing_zset(key).map_or(0, |z| z.size());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "ZRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_int);
            let stop = parse_int(rest);
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s as i64, e as i64),
                _ => {
                    conn.reply("ERR usage: ZRANGE key start stop\r\n");
                    return;
                }
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            if let Some(zset) = app.store.get_existing_zset(key) {
                for (member, score) in zset.range(start, stop) {
                    conn.reply(&format!(
                        "{} {}\r\n",
                        member,
                        crate::util::strutil::format_g(score, 6)
                    ));
                }
            }
            conn.reply("END\r\n");
        }

        // --- connection / server commands ---
        "SAVE" => {
            app.persist.save(&mut app.store);
            conn.reply("OK\r\n");
        }

        "QUIT" => {
            conn.reply("BYE\r\n");
            conn.request_close();
        }

        "SHUTDOWN" => {
            conn.reply("SHUTTING_DOWN\r\n");
            app.running = false;
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}
