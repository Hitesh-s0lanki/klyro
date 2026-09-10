//! Set commands, including the SINTER/SUNION/SDIFF algebra.

use std::collections::HashSet;

use super::{check_type, exact_args, min_args, parse_int, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::types::set::Set;
use crate::util::bytes::Bytes;
use crate::util::rand;

/// Snapshots each named set, treating a missing key as empty.
///
/// The sets are cloned rather than borrowed because the store hands out
/// one `&mut` at a time; the algebra commands need them all at once.
fn collect_sets(app: &mut App, keys: &[Bytes]) -> Checked<Vec<Set>> {
    for key in keys {
        check_type(app, key, StoreType::Set)?;
    }
    Ok(keys
        .iter()
        .map(|key| app.store.read_set(key).cloned().unwrap_or_default())
        .collect())
}

/// Removes `count` members at pseudo-random. Fewer are returned if the
/// set is smaller.
fn take_random(set: &mut Set, count: usize) -> Vec<Bytes> {
    let mut members: Vec<Bytes> = set.iter().cloned().collect();
    let count = count.min(members.len());
    let mut taken = Vec::with_capacity(count);
    for _ in 0..count {
        let picked = members.swap_remove(rand::below(members.len()));
        set.remove(&picked);
        taken.push(picked);
    }
    taken
}

/// Intersection, union, or difference of `sets` in the order given.
fn combine(op: &str, sets: &[Set]) -> Vec<Bytes> {
    let (first, others) = sets.split_first().expect("at least one key was required");
    let mut accumulated: HashSet<&Bytes> = first.iter().collect();
    for other in others {
        match op {
            "SINTER" => accumulated.retain(|m| other.contains(*m)),
            "SUNION" => accumulated.extend(other.iter()),
            _ => accumulated.retain(|m| !other.contains(*m)),
        }
    }
    accumulated.into_iter().cloned().collect()
}

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "SADD" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let s = app.store.get_or_create_set(&argv[1]).expect("type checked");
            let added = argv[2..].iter().filter(|m| s.insert((*m).clone())).count();
            Ok(Reply::Integer(added as i64))
        }

        "SREM" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let removed = match app.store.write_set(&argv[1]) {
                Some(s) => argv[2..].iter().filter(|m| s.remove(*m)).count(),
                None => 0,
            };
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "SISMEMBER" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let present = app
                .store
                .read_set(&argv[1])
                .is_some_and(|s| s.contains(&argv[2]));
            Ok(Reply::bool(present))
        }

        "SMISMEMBER" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let set = app.store.read_set(&argv[1]).cloned().unwrap_or_default();
            Ok(Reply::array(
                argv[2..]
                    .iter()
                    .map(|m| Reply::bool(set.contains(m)))
                    .collect(),
            ))
        }

        "SCARD" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let len = app.store.read_set(&argv[1]).map_or(0, |s| s.len());
            Ok(Reply::Integer(len as i64))
        }

        "SMEMBERS" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Set)?;
            let members: Vec<Bytes> = app
                .store
                .read_set(&argv[1])
                .map(|s| s.iter().cloned().collect())
                .unwrap_or_default();
            Ok(Reply::Set(members.into_iter().map(Reply::bulk).collect()))
        }

        "SPOP" | "SRANDMEMBER" => {
            min_args(argv, name, 1)?;
            let count = match argv.len() {
                2 => None,
                3 => Some(parse_int(&argv[2])?),
                _ => return Err(Reply::wrong_arity(name)),
            };
            if name == "SPOP" && count.is_some_and(|c| c < 0) {
                return Ok(Reply::error("ERR value is out of range, must be positive"));
            }
            check_type(app, &argv[1], StoreType::Set)?;

            let picked = if name == "SPOP" {
                let wanted = count.unwrap_or(1).max(0) as usize;
                let taken = app
                    .store
                    .write_set(&argv[1])
                    .map(|s| take_random(s, wanted))
                    .unwrap_or_default();
                app.store.delete_if_empty(&argv[1]);
                taken
            } else {
                // SRANDMEMBER with a negative count may repeat members,
                // and always returns exactly that many when the set is
                // non-empty - the same rule Redis uses.
                let set = app.store.read_set(&argv[1]).cloned().unwrap_or_default();
                match count {
                    Some(n) if n < 0 => {
                        let members: Vec<&Bytes> = set.iter().collect();
                        (0..n.unsigned_abs())
                            .filter_map(|_| {
                                members
                                    .get(rand::below(members.len()))
                                    .map(|m| (*m).clone())
                            })
                            .collect()
                    }
                    _ => {
                        let mut copy = set.clone();
                        take_random(&mut copy, count.unwrap_or(1).max(0) as usize)
                    }
                }
            };

            Ok(match count {
                None => picked.into_iter().next().map_or(Reply::Nil, Reply::Bulk),
                // SPOP returns a set; SRANDMEMBER an array, because a
                // negative count lets it repeat members.
                Some(_) if name == "SPOP" => {
                    Reply::Set(picked.into_iter().map(Reply::bulk).collect())
                }
                Some(_) => Reply::bulk_array(picked),
            })
        }

        "SMOVE" => {
            exact_args(argv, name, 3)?;
            let (source, destination, member) = (&argv[1], &argv[2], &argv[3]);
            check_type(app, source, StoreType::Set)?;
            check_type(app, destination, StoreType::Set)?;
            let removed = app
                .store
                .write_set(source)
                .is_some_and(|s| s.remove(member));
            if !removed {
                return Ok(Reply::bool(false));
            }
            app.store
                .get_or_create_set(destination)
                .expect("type checked")
                .insert(member.clone());
            app.store.delete_if_empty(source);
            Ok(Reply::bool(true))
        }

        "SINTER" | "SUNION" | "SDIFF" => {
            min_args(argv, name, 1)?;
            let sets = collect_sets(app, &argv[1..])?;
            Ok(Reply::Set(
                combine(name, &sets).into_iter().map(Reply::bulk).collect(),
            ))
        }

        "SINTERSTORE" | "SUNIONSTORE" | "SDIFFSTORE" => {
            min_args(argv, name, 2)?;
            let destination = &argv[1];
            check_type(app, destination, StoreType::Set)?;
            let sets = collect_sets(app, &argv[2..])?;
            let result = combine(name.trim_end_matches("STORE"), &sets);

            // Storing an empty result deletes the destination, keeping
            // the "empty collections don't exist" rule.
            app.store.del(destination);
            let len = result.len();
            if len > 0 {
                app.store
                    .get_or_create_set(destination)
                    .expect("just deleted, so it is free")
                    .extend(result);
            }
            Ok(Reply::Integer(len as i64))
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}
