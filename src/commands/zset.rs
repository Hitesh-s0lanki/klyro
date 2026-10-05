//! Sorted set commands.

use super::{check_type, exact_args, min_args, parse_float, parse_int, syntax_error, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::types::zset::ScoreBound;
use crate::util::bytes::{eq_ignore_case, Bytes};

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

/// Renders (member, score) pairs. Without scores it is a plain array of
/// members; with them, `ScoredMembers` handles the RESP2/RESP3 shape
/// difference.
fn pairs_reply(pairs: Vec<(Bytes, f64)>, with_scores: bool) -> Reply {
    if with_scores {
        Reply::ScoredMembers(pairs)
    } else {
        Reply::bulk_array(pairs.into_iter().map(|(member, _)| member))
    }
}

fn owned(pairs: Vec<(&[u8], f64)>) -> Vec<(Bytes, f64)> {
    pairs.into_iter().map(|(m, s)| (m.to_vec(), s)).collect()
}

/// Parses a `min max` pair, rejecting a bound that isn't a score.
fn score_bounds(min: &[u8], max: &[u8]) -> Checked<(ScoreBound, ScoreBound)> {
    match (ScoreBound::parse(min), ScoreBound::parse(max)) {
        (Some(a), Some(b)) => Ok((a, b)),
        _ => Err(Reply::error("ERR min or max is not a float")),
    }
}

/// Reads a trailing `WITHSCORES`, if present.
fn with_scores(argv: &[Bytes], from: usize) -> Checked<bool> {
    match argv.len() - from {
        0 => Ok(false),
        1 if eq_ignore_case(&argv[from], "WITHSCORES") => Ok(true),
        _ => Err(syntax_error()),
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "ZADD" => zadd(app, argv),

        "ZSCORE" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let score = app
                .store
                .read_zset(&argv[1])
                .and_then(|z| z.score(&argv[2]));
            Ok(score.map_or(Reply::Nil, Reply::Double))
        }

        "ZMSCORE" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let zset = app.store.read_zset(&argv[1]).cloned();
            Ok(Reply::array(
                argv[2..]
                    .iter()
                    .map(|m| match zset.as_ref().and_then(|z| z.score(m)) {
                        Some(s) => Reply::Double(s),
                        None => Reply::Nil,
                    })
                    .collect(),
            ))
        }

        "ZINCRBY" => {
            exact_args(argv, name, 3)?;
            let increment = parse_float(&argv[2])?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let updated = app
                .store
                .get_or_create_zset(&argv[1])
                .expect("type checked")
                .incr_by(&argv[3], increment);
            if !updated.is_finite() {
                // Undo, so a NaN/inf score never reaches the keyspace.
                app.store
                    .write_zset(&argv[1])
                    .expect("just created")
                    .rem(&argv[3]);
                app.store.delete_if_empty(&argv[1]);
                return Ok(Reply::error("ERR resulting score is not a number (NaN)"));
            }
            Ok(Reply::Double(updated))
        }

        "ZREM" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let removed = match app.store.write_zset(&argv[1]) {
                Some(z) => argv[2..].iter().filter(|m| z.rem(m)).count(),
                None => 0,
            };
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "ZCARD" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let len = app.store.read_zset(&argv[1]).map_or(0, |z| z.size());
            Ok(Reply::Integer(len as i64))
        }

        "ZRANK" | "ZREVRANK" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let rank = app.store.read_zset(&argv[1]).and_then(|z| {
                if name == "ZRANK" {
                    z.rank(&argv[2])
                } else {
                    z.rev_rank(&argv[2])
                }
            });
            Ok(rank.map_or(Reply::Nil, |r| Reply::Integer(r as i64)))
        }

        "ZRANGE" | "ZREVRANGE" => {
            min_args(argv, name, 3)?;
            let (start, stop) = (parse_int(&argv[2])?, parse_int(&argv[3])?);
            let scores = with_scores(argv, 4)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let pairs = app
                .store
                .read_zset(&argv[1])
                .map(|z| {
                    owned(if name == "ZRANGE" {
                        z.range(start, stop)
                    } else {
                        z.rev_range(start, stop)
                    })
                })
                .unwrap_or_default();
            Ok(pairs_reply(pairs, scores))
        }

        // ZREVRANGEBYSCORE takes its bounds high-first, as Redis does.
        "ZRANGEBYSCORE" | "ZREVRANGEBYSCORE" => {
            min_args(argv, name, 3)?;
            let reversed = name == "ZREVRANGEBYSCORE";
            let (min, max) = if reversed {
                score_bounds(&argv[3], &argv[2])?
            } else {
                score_bounds(&argv[2], &argv[3])?
            };
            let scores = with_scores(argv, 4)?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let mut pairs = app
                .store
                .read_zset(&argv[1])
                .map(|z| owned(z.range_by_score(min, max)))
                .unwrap_or_default();
            if reversed {
                pairs.reverse();
            }
            Ok(pairs_reply(pairs, scores))
        }

        "ZCOUNT" => {
            exact_args(argv, name, 3)?;
            let (min, max) = score_bounds(&argv[2], &argv[3])?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let count = app
                .store
                .read_zset(&argv[1])
                .map_or(0, |z| z.count_by_score(min, max));
            Ok(Reply::Integer(count as i64))
        }

        "ZREMRANGEBYSCORE" => {
            exact_args(argv, name, 3)?;
            let (min, max) = score_bounds(&argv[2], &argv[3])?;
            check_type(app, &argv[1], StoreType::Zset)?;
            let removed = app
                .store
                .write_zset(&argv[1])
                .map_or(0, |z| z.remove_range_by_score(min, max));
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "ZREMRANGEBYRANK" => {
            exact_args(argv, name, 3)?;
            let (start, stop) = (parse_int(&argv[2])?, parse_int(&argv[3])?);
            check_type(app, &argv[1], StoreType::Zset)?;
            let removed = app
                .store
                .write_zset(&argv[1])
                .map_or(0, |z| z.remove_range_by_rank(start, stop));
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "ZPOPMIN" | "ZPOPMAX" => {
            min_args(argv, name, 1)?;
            let count = match argv.len() {
                2 => None,
                3 => match parse_int(&argv[2])? {
                    c if c >= 0 => Some(c as usize),
                    _ => return Ok(Reply::error("ERR value is out of range, must be positive")),
                },
                _ => return Err(Reply::wrong_arity(name)),
            };
            check_type(app, &argv[1], StoreType::Zset)?;
            let mut popped = app
                .store
                .write_zset(&argv[1])
                .map(|z| z.pop(count.unwrap_or(1), name == "ZPOPMAX"))
                .unwrap_or_default();
            app.store.delete_if_empty(&argv[1]);
            Ok(match count {
                // Without a count Redis replies with the single member
                // and its score side by side, not as a nested pair.
                None => match popped.pop() {
                    Some((member, score)) => {
                        Reply::array(vec![Reply::Bulk(member), Reply::Double(score)])
                    }
                    None => Reply::array(Vec::new()),
                },
                Some(_) => Reply::ScoredMembers(popped),
            })
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}

/// `ZADD key score member [score member ...]`
fn zadd(app: &mut App, argv: &[Bytes]) -> Checked<Reply> {
    min_args(argv, "ZADD", 3)?;
    let rest = &argv[2..];
    if !rest.len().is_multiple_of(2) {
        return Err(syntax_error());
    }
    if rest.len() / 2 > app.config.zadd_max_pairs {
        return Err(Reply::error("ERR too many score/member pairs"));
    }

    // Parse every score before touching the keyspace, so a bad score
    // late in the list can't leave a half-applied ZADD behind.
    let mut pairs = Vec::with_capacity(rest.len() / 2);
    for pair in rest.chunks(2) {
        pairs.push((parse_float(&pair[0])?, &pair[1]));
    }

    check_type(app, &argv[1], StoreType::Zset)?;
    let zset = app
        .store
        .get_or_create_zset(&argv[1])
        .expect("type checked");
    let added = pairs
        .iter()
        .filter(|(score, member)| zset.add(member, *score))
        .count();
    Ok(Reply::Integer(added as i64))
}
