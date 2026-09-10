//! Set commands, including the SINTER/SUNION/SDIFF algebra.

use std::collections::HashSet;

use super::{check_type, remaining_tokens, reply_list, reply_ok_or_missing, usage};
use crate::app::App;
use crate::server::Conn;
use crate::store::StoreType;
use crate::types::set::Set;
use crate::util::rand;
use crate::util::strutil::{next_token, parse_long, trim};

/// Snapshots each named set, treating a missing key as empty. Replies
/// WRONGTYPE and returns `None` if any key holds another type.
///
/// The sets are cloned rather than borrowed because the store hands out
/// one `&mut` at a time; the algebra commands need them all at once.
fn collect_sets(app: &mut App, conn: &mut Conn, keys: &[&str]) -> Option<Vec<Set>> {
    for key in keys {
        if !check_type(app, conn, key, StoreType::Set) {
            return None;
        }
    }
    Some(
        keys.iter()
            .map(|key| app.store.get_existing_set(key).cloned().unwrap_or_default())
            .collect(),
    )
}

/// Removes `count` members at pseudo-random. Fewer are returned if the
/// set is smaller.
fn take_random(set: &mut Set, count: usize) -> Vec<String> {
    let mut members: Vec<String> = set.iter().cloned().collect();
    let count = count.min(members.len());
    let mut taken = Vec::with_capacity(count);
    for _ in 0..count {
        let picked = members.swap_remove(rand::below(members.len()));
        set.remove(&picked);
        taken.push(picked);
    }
    taken
}

pub fn dispatch(app: &mut App, conn: &mut Conn, cmd: &str, rest: &str) {
    let mut rest = rest;

    match cmd {
        "SADD" => {
            let key = next_token(&mut rest);
            let members = remaining_tokens(&mut rest);
            let (key, members) = match key {
                Some(k) if !members.is_empty() => (k, members),
                _ => return usage(conn, "SADD key member [member ...]"),
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let s = app.store.get_or_create_set(key).expect("type checked");
            let added = members.iter().filter(|m| s.insert(m.to_string())).count();
            conn.reply(&format!("ADDED {}\r\n", added));
        }

        "SREM" => {
            let key = next_token(&mut rest);
            let members = remaining_tokens(&mut rest);
            let (key, members) = match key {
                Some(k) if !members.is_empty() => (k, members),
                _ => return usage(conn, "SREM key member [member ...]"),
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let removed = match app.store.get_existing_set(key) {
                Some(s) => members.iter().filter(|m| s.remove(**m)).count(),
                None => 0,
            };
            app.store.delete_if_empty(key);
            if members.len() == 1 {
                reply_ok_or_missing(conn, removed == 1);
            } else {
                conn.reply(&format!("DELETED {}\r\n", removed));
            }
        }

        "SISMEMBER" => {
            let key = next_token(&mut rest);
            let member = trim(rest);
            let Some(key) = key.filter(|_| !member.is_empty()) else {
                return usage(conn, "SISMEMBER key member");
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let present = app
                .store
                .get_existing_set(key)
                .is_some_and(|s| s.contains(member));
            conn.reply(if present { "TRUE\r\n" } else { "FALSE\r\n" });
        }

        "SMISMEMBER" => {
            let key = next_token(&mut rest);
            let members = remaining_tokens(&mut rest);
            let (key, members) = match key {
                Some(k) if !members.is_empty() => (k, members),
                _ => return usage(conn, "SMISMEMBER key member [member ...]"),
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let set = app.store.get_existing_set(key).cloned().unwrap_or_default();
            let lines: Vec<&str> = members
                .iter()
                .map(|m| if set.contains(*m) { "TRUE" } else { "FALSE" })
                .collect();
            reply_list(conn, lines);
        }

        "SCARD" => {
            let key = trim(rest);
            if key.is_empty() {
                return usage(conn, "SCARD key");
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
                return usage(conn, "SMEMBERS key");
            }
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }
            let members: Vec<String> = app
                .store
                .get_existing_set(key)
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            reply_list(conn, members);
        }

        "SPOP" | "SRANDMEMBER" => {
            let key = next_token(&mut rest);
            let count = match trim(rest) {
                "" => None,
                text => match parse_long(text) {
                    Some(c) => Some(c),
                    None => return usage(conn, &format!("{} key [count]", cmd)),
                },
            };
            let Some(key) = key else {
                return usage(conn, &format!("{} key [count]", cmd));
            };
            if !check_type(app, conn, key, StoreType::Set) {
                return;
            }

            let picked = if cmd == "SPOP" {
                let wanted = count.unwrap_or(1).max(0) as usize;
                let taken = app
                    .store
                    .get_existing_set(key)
                    .map(|s| take_random(s, wanted))
                    .unwrap_or_default();
                app.store.delete_if_empty(key);
                taken
            } else {
                // SRANDMEMBER with a negative count may repeat members,
                // and always returns exactly that many when the set is
                // non-empty - the same rule Redis uses.
                let set = app.store.get_existing_set(key).cloned().unwrap_or_default();
                let members: Vec<&String> = set.iter().collect();
                match count {
                    Some(n) if n < 0 => (0..n.unsigned_abs())
                        .filter_map(|_| {
                            members
                                .get(rand::below(members.len()))
                                .map(|m| (*m).clone())
                        })
                        .collect(),
                    _ => {
                        let mut copy = set.clone();
                        take_random(&mut copy, count.unwrap_or(1).max(0) as usize)
                    }
                }
            };

            match count {
                Some(_) => reply_list(conn, picked),
                None => match picked.first() {
                    Some(m) => conn.reply(&format!("VALUE {}\r\n", m)),
                    None => conn.reply("NOT_FOUND\r\n"),
                },
            }
        }

        "SMOVE" => {
            let source = next_token(&mut rest);
            let destination = next_token(&mut rest);
            let member = trim(rest);
            let (source, destination) = match (source, destination) {
                (Some(s), Some(d)) if !member.is_empty() => (s, d),
                _ => return usage(conn, "SMOVE source destination member"),
            };
            if !check_type(app, conn, source, StoreType::Set)
                || !check_type(app, conn, destination, StoreType::Set)
            {
                return;
            }
            let removed = app
                .store
                .get_existing_set(source)
                .is_some_and(|s| s.remove(member));
            if !removed {
                return conn.reply("NOT_FOUND\r\n");
            }
            app.store
                .get_or_create_set(destination)
                .expect("type checked")
                .insert(member.to_string());
            app.store.delete_if_empty(source);
            conn.reply("OK\r\n");
        }

        "SINTER" | "SUNION" | "SDIFF" => {
            let keys = remaining_tokens(&mut rest);
            if keys.is_empty() {
                return usage(conn, &format!("{} key [key ...]", cmd));
            }
            let Some(sets) = collect_sets(app, conn, &keys) else {
                return;
            };
            reply_list(conn, combine(cmd, &sets));
        }

        "SINTERSTORE" | "SUNIONSTORE" | "SDIFFSTORE" => {
            let destination = next_token(&mut rest);
            let keys = remaining_tokens(&mut rest);
            let (destination, keys) = match destination {
                Some(d) if !keys.is_empty() => (d, keys),
                _ => return usage(conn, &format!("{} destination key [key ...]", cmd)),
            };
            if !check_type(app, conn, destination, StoreType::Set) {
                return;
            }
            let Some(sets) = collect_sets(app, conn, &keys) else {
                return;
            };
            let result = combine(cmd.trim_end_matches("STORE"), &sets);

            // Storing an empty result deletes the destination, keeping
            // the "empty collections don't exist" rule.
            app.store.del(destination);
            let len = result.len();
            if len > 0 {
                let target = app
                    .store
                    .get_or_create_set(destination)
                    .expect("just deleted, so it is free");
                target.extend(result);
            }
            conn.reply(&format!("LEN {}\r\n", len));
        }

        _ => conn.reply("ERR unknown command\r\n"),
    }
}

/// Intersection, union, or difference of `sets` in the order given.
fn combine(op: &str, sets: &[Set]) -> Vec<String> {
    let (first, others) = sets.split_first().expect("at least one key was required");
    let mut accumulated: HashSet<&String> = first.iter().collect();
    for other in others {
        match op {
            "SINTER" => accumulated.retain(|m| other.contains(*m)),
            "SUNION" => accumulated.extend(other.iter()),
            _ => accumulated.retain(|m| !other.contains(*m)),
        }
    }
    accumulated.into_iter().cloned().collect()
}
