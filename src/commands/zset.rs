//! Sorted set commands.

use super::{check_type, remaining_tokens, reply_list, reply_ok_or_missing, usage};
use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::types::zset::ScoreBound;
use crate::util::strutil::{format_g, next_token, parse_double, parse_long, trim};

/// Scores are shown at 6 significant digits (what the dump file's 17
/// would look like is an implementation detail, not a display format).
fn score_text(score: f64) -> String {
    format_g(score, 6)
}

fn member_score_lines(pairs: &[(String, f64)]) -> Vec<String> {
    pairs
        .iter()
        .map(|(m, s)| format!("{} {}", m, score_text(*s)))
        .collect()
}

/// Parses the `min max` pair every score-range command takes.
fn score_bounds(rest: &mut &str) -> Option<(ScoreBound, ScoreBound)> {
    let first = next_token(rest).and_then(ScoreBound::parse)?;
    let second = ScoreBound::parse(trim(rest))?;
    Some((first, second))
}

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        "ZADD" => zadd(app, conn, rest),

        "ZSCORE" => {
            let key = next_token(&mut rest);
            let member = trim(rest);
            let Some(key) = key.filter(|_| !member.is_empty()) else {
                return usage(conn, "ZSCORE key member");
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            match app
                .store
                .get_existing_zset(key)
                .and_then(|z| z.score(member))
            {
                Some(s) => conn.reply(&format!("VALUE {}\r\n", score_text(s))),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "ZMSCORE" => {
            let key = next_token(&mut rest);
            let members = remaining_tokens(&mut rest);
            let (key, members) = match key {
                Some(k) if !members.is_empty() => (k, members),
                _ => return usage(conn, "ZMSCORE key member [member ...]"),
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let zset = app.store.get_existing_zset(key);
            let lines: Vec<String> = members
                .iter()
                .map(|m| match zset.as_ref().and_then(|z| z.score(m)) {
                    Some(s) => format!("VALUE {}", score_text(s)),
                    None => "NOT_FOUND".to_string(),
                })
                .collect();
            reply_list(conn, lines);
        }

        "ZINCRBY" => {
            let key = next_token(&mut rest);
            let increment = next_token(&mut rest).and_then(parse_double);
            let member = trim(rest);
            let (key, increment) = match (key, increment) {
                (Some(k), Some(i)) if !member.is_empty() => (k, i),
                _ => return usage(conn, "ZINCRBY key increment member"),
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let updated = app
                .store
                .get_or_create_zset(key)
                .expect("type checked")
                .incr_by(member, increment);
            if !updated.is_finite() {
                // Undo, so a NaN/inf score never reaches the keyspace.
                app.store
                    .get_existing_zset(key)
                    .expect("just created")
                    .rem(member);
                app.store.delete_if_empty(key);
                return conn.reply("ERR increment would produce NaN or Infinity\r\n");
            }
            conn.reply(&format!("VALUE {}\r\n", score_text(updated)));
        }

        "ZREM" => {
            let key = next_token(&mut rest);
            let members = remaining_tokens(&mut rest);
            let (key, members) = match key {
                Some(k) if !members.is_empty() => (k, members),
                _ => return usage(conn, "ZREM key member [member ...]"),
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let removed = match app.store.get_existing_zset(key) {
                Some(z) => members.iter().filter(|m| z.rem(m)).count(),
                None => 0,
            };
            app.store.delete_if_empty(key);
            if members.len() == 1 {
                reply_ok_or_missing(conn, removed == 1);
            } else {
                conn.reply(&format!("DELETED {}\r\n", removed));
            }
        }

        "ZCARD" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "ZCARD key");
            }
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let len = app.store.get_existing_zset(key).map_or(0, |z| z.size());
            conn.reply(&format!("LEN {}\r\n", len));
        }

        "ZRANK" | "ZREVRANK" => {
            let key = next_token(&mut rest);
            let member = trim(rest);
            let Some(key) = key.filter(|_| !member.is_empty()) else {
                return usage(conn, &format!("{} key member", cmd));
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let rank = app.store.get_existing_zset(key).and_then(|z| {
                if cmd == "ZRANK" {
                    z.rank(member)
                } else {
                    z.rev_rank(member)
                }
            });
            match rank {
                Some(r) => conn.reply(&format!("RANK {}\r\n", r)),
                None => conn.reply("NOT_FOUND\r\n"),
            }
        }

        "ZRANGE" | "ZREVRANGE" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let stop = parse_long(trim(rest));
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => return usage(conn, &format!("{} key start stop", cmd)),
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let pairs: Vec<(String, f64)> = app
                .store
                .get_existing_zset(key)
                .map(|z| {
                    let window = if cmd == "ZRANGE" {
                        z.range(start, stop)
                    } else {
                        z.rev_range(start, stop)
                    };
                    window
                        .into_iter()
                        .map(|(m, s)| (m.to_string(), s))
                        .collect()
                })
                .unwrap_or_default();
            reply_list(conn, member_score_lines(&pairs));
        }

        // ZREVRANGEBYSCORE takes its bounds high-first, as Redis does.
        "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" | "ZCOUNT" | "ZREMRANGEBYSCORE" => {
            let reversed = cmd == "ZREVRANGEBYSCORE";
            let spec = if reversed {
                "ZREVRANGEBYSCORE key max min"
            } else {
                "<cmd> key min max"
            };
            let key = next_token(&mut rest);
            let bounds = score_bounds(&mut rest);
            let (key, (first, second)) = match (key, bounds) {
                (Some(k), Some(b)) => (k, b),
                _ => return usage(conn, spec),
            };
            let (min, max) = if reversed {
                (second, first)
            } else {
                (first, second)
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }

            if cmd == "ZCOUNT" {
                let count = app
                    .store
                    .get_existing_zset(key)
                    .map_or(0, |z| z.count_by_score(min, max));
                return conn.reply(&format!("COUNT {}\r\n", count));
            }
            if cmd == "ZREMRANGEBYSCORE" {
                let removed = app
                    .store
                    .get_existing_zset(key)
                    .map_or(0, |z| z.remove_range_by_score(min, max));
                app.store.delete_if_empty(key);
                return conn.reply(&format!("REMOVED {}\r\n", removed));
            }

            let mut pairs: Vec<(String, f64)> = app
                .store
                .get_existing_zset(key)
                .map(|z| {
                    z.range_by_score(min, max)
                        .into_iter()
                        .map(|(m, s)| (m.to_string(), s))
                        .collect()
                })
                .unwrap_or_default();
            if reversed {
                pairs.reverse();
            }
            reply_list(conn, member_score_lines(&pairs));
        }

        "ZREMRANGEBYRANK" => {
            let key = next_token(&mut rest);
            let start = next_token(&mut rest).and_then(parse_long);
            let stop = parse_long(trim(rest));
            let (key, start, stop) = match (key, start, stop) {
                (Some(k), Some(s), Some(e)) => (k, s, e),
                _ => return usage(conn, "ZREMRANGEBYRANK key start stop"),
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let removed = app
                .store
                .get_existing_zset(key)
                .map_or(0, |z| z.remove_range_by_rank(start, stop));
            app.store.delete_if_empty(key);
            conn.reply(&format!("REMOVED {}\r\n", removed));
        }

        "ZPOPMIN" | "ZPOPMAX" => {
            let key = next_token(&mut rest);
            let count = match trim(rest) {
                "" => 1,
                text => match parse_long(text).filter(|&c| c >= 0) {
                    Some(c) => c as usize,
                    None => return usage(conn, &format!("{} key [count]", cmd)),
                },
            };
            let Some(key) = key else {
                return usage(conn, &format!("{} key [count]", cmd));
            };
            if !check_type(app, conn, key, StoreType::Zset) {
                return;
            }
            let popped = app
                .store
                .get_existing_zset(key)
                .map(|z| z.pop(count, cmd == "ZPOPMAX"))
                .unwrap_or_default();
            app.store.delete_if_empty(key);
            reply_list(conn, member_score_lines(&popped));
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

const ZADD_USAGE: &str = "ZADD key score member [score member ...]";

fn zadd(app: &mut App, conn: &mut Conn, rest: &str) {
    let mut rest = rest;
    let key = next_token(&mut rest);

    let mut pairs: Vec<(f64, &str)> = Vec::new();
    while let Some(score_text) = next_token(&mut rest) {
        if pairs.len() == app.config.zadd_max_pairs {
            return conn.reply("ERR too many score/member pairs\r\n");
        }
        match (parse_double(score_text), next_token(&mut rest)) {
            (Some(score), Some(member)) => pairs.push((score, member)),
            _ => return usage(conn, ZADD_USAGE),
        }
    }

    let (Some(key), false) = (key, pairs.is_empty()) else {
        return usage(conn, ZADD_USAGE);
    };
    if !check_type(app, conn, key, StoreType::Zset) {
        return;
    }
    let zset = app.store.get_or_create_zset(key).expect("type checked");
    let added = pairs
        .iter()
        .filter(|(score, member)| zset.add(member, *score))
        .count();
    conn.reply(&format!("ADDED {}\r\n", added));
}
