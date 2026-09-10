//! The blocking pops - BLPOP, BRPOP, BLMOVE, BRPOPLPUSH, BLMPOP,
//! BZPOPMIN, BZPOPMAX, BZMPOP - and LMPOP/ZMPOP, the two non-blocking
//! commands they share their argument shape with.
//!
//! Every one of these is the same command twice. It first tries the
//! non-blocking operation; if that finds something, the reply is what
//! the plain command would have said. If it does not, the client is
//! parked: the handler returns a [`Blocked`] describing which keys to
//! watch and when to give up, the event loop stops reading from that
//! connection, and the command is re-run - through this same code -
//! the moment one of those keys is written. That is why the retry
//! needs no special case: a woken command simply runs again from the
//! top, and blocks again if another waiter got there first.
//!
//! A blocking command inside MULTI never parks. Nothing could unblock
//! it: the connection that would have to send the push is the one
//! sitting inside EXEC.

use std::time::{Duration, Instant};

use super::list::{move_one, pop_one, End};
use super::{check_type, min_args, syntax_error, Checked, Response};
use crate::app::App;
use crate::client::{Blocked, Client};
use crate::resp::Reply;
use crate::store::StoreType;
use crate::util::bytes::{eq_ignore_case, parse_f64, Bytes};

/// Which end of a sorted set a pop takes from.
#[derive(Clone, Copy, PartialEq)]
enum Edge {
    Min,
    Max,
}

impl Edge {
    fn parse(token: &[u8]) -> Option<Edge> {
        if eq_ignore_case(token, "MIN") {
            Some(Edge::Min)
        } else if eq_ignore_case(token, "MAX") {
            Some(Edge::Max)
        } else {
            None
        }
    }
}

pub fn dispatch(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    match handle(app, client, name, argv) {
        Ok(response) => response,
        Err(error) => Response::new(error),
    }
}

fn handle(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Checked<Response> {
    match name {
        // BLPOP key [key ...] timeout
        "BLPOP" | "BRPOP" => {
            min_args(argv, name, 2)?;
            let end = if name == "BLPOP" {
                End::Left
            } else {
                End::Right
            };
            let keys = argv[1..argv.len() - 1].to_vec();
            let deadline = parse_timeout(&argv[argv.len() - 1])?;
            let found = pop_first_list(app, &keys, end)?;
            Ok(answer(client, argv, found, keys, deadline, Reply::NilArray))
        }

        // BLMOVE source destination LEFT|RIGHT LEFT|RIGHT timeout
        // BRPOPLPUSH source destination timeout
        "BLMOVE" | "BRPOPLPUSH" => {
            let (from, to, timeout) = if name == "BRPOPLPUSH" {
                super::exact_args(argv, name, 3)?;
                (End::Right, End::Left, &argv[3])
            } else {
                super::exact_args(argv, name, 5)?;
                match (End::parse(&argv[3]), End::parse(&argv[4])) {
                    (Some(from), Some(to)) => (from, to, &argv[5]),
                    _ => return Err(syntax_error()),
                }
            };
            let deadline = parse_timeout(timeout)?;
            let (source, destination) = (argv[1].clone(), argv[2].clone());
            check_type(app, &source, StoreType::List)?;
            check_type(app, &destination, StoreType::List)?;
            let found = move_one(app, &source, &destination, from, to).map(Reply::Bulk);
            Ok(answer(
                client,
                argv,
                found,
                vec![source],
                deadline,
                Reply::Nil,
            ))
        }

        // LMPOP numkeys key [key ...] LEFT|RIGHT [COUNT count]
        // BLMPOP timeout numkeys key [key ...] LEFT|RIGHT [COUNT count]
        "LMPOP" | "BLMPOP" => {
            let blocking = name == "BLMPOP";
            let deadline = if blocking {
                min_args(argv, name, 4)?;
                Some(parse_timeout(&argv[1])?)
            } else {
                min_args(argv, name, 3)?;
                None
            };
            let request = parse_multi_key(argv, if blocking { 2 } else { 1 }, name)?;
            let end = End::parse(&request.direction).ok_or_else(syntax_error)?;
            let found = mpop_lists(app, &request.keys, end, request.count)?;
            match deadline {
                None => Ok(Response::new(found.unwrap_or(Reply::NilArray))),
                Some(deadline) => Ok(answer(
                    client,
                    argv,
                    found,
                    request.keys,
                    deadline,
                    Reply::NilArray,
                )),
            }
        }

        // BZPOPMIN key [key ...] timeout
        "BZPOPMIN" | "BZPOPMAX" => {
            min_args(argv, name, 2)?;
            let edge = if name == "BZPOPMIN" {
                Edge::Min
            } else {
                Edge::Max
            };
            let keys = argv[1..argv.len() - 1].to_vec();
            let deadline = parse_timeout(&argv[argv.len() - 1])?;
            let found = pop_first_zset(app, &keys, edge)?;
            Ok(answer(client, argv, found, keys, deadline, Reply::NilArray))
        }

        // ZMPOP numkeys key [key ...] MIN|MAX [COUNT count]
        // BZMPOP timeout numkeys key [key ...] MIN|MAX [COUNT count]
        "ZMPOP" | "BZMPOP" => {
            let blocking = name == "BZMPOP";
            let deadline = if blocking {
                min_args(argv, name, 4)?;
                Some(parse_timeout(&argv[1])?)
            } else {
                min_args(argv, name, 3)?;
                None
            };
            let request = parse_multi_key(argv, if blocking { 2 } else { 1 }, name)?;
            let edge = Edge::parse(&request.direction).ok_or_else(syntax_error)?;
            let found = mpop_zsets(app, &request.keys, edge, request.count)?;
            match deadline {
                None => Ok(Response::new(found.unwrap_or(Reply::NilArray))),
                Some(deadline) => Ok(answer(
                    client,
                    argv,
                    found,
                    request.keys,
                    deadline,
                    Reply::NilArray,
                )),
            }
        }

        _ => Ok(Response::new(Reply::error("ERR unknown command"))),
    }
}

/// Either the value the command found, or a parked client waiting for
/// one.
///
/// Inside EXEC there is no third option: a transaction answers with the
/// timeout reply straight away rather than parking, because the only
/// client that could feed it is the one running the transaction.
fn answer(
    client: &Client,
    argv: &[Bytes],
    found: Option<Reply>,
    keys: Vec<Bytes>,
    deadline: Option<Instant>,
    on_timeout: Reply,
) -> Response {
    match found {
        Some(reply) => Response::new(reply),
        None if client.in_exec => Response::new(on_timeout),
        None => Response::parked(Blocked {
            id: client.id,
            keys,
            deadline,
            argv: argv.to_vec(),
            on_timeout,
        }),
    }
}

/// Reads a blocking command's timeout, in seconds, into the deadline
/// it means. Zero is Redis's "wait forever", which is `None`.
fn parse_timeout(arg: &[u8]) -> Checked<Option<Instant>> {
    let Some(seconds) = parse_f64(arg) else {
        return Err(Reply::error("ERR timeout is not a float or out of range"));
    };
    if seconds < 0.0 {
        return Err(Reply::error("ERR timeout is negative"));
    }
    if !seconds.is_finite() {
        return Err(Reply::error("ERR timeout is out of range"));
    }
    if seconds == 0.0 {
        return Ok(None);
    }
    Ok(Some(Instant::now() + Duration::from_secs_f64(seconds)))
}

/// The keys and options shared by LMPOP, ZMPOP, and their blocking
/// spellings.
struct MultiKey {
    keys: Vec<Bytes>,
    /// LEFT/RIGHT, or MIN/MAX - whichever the caller's command takes.
    direction: Bytes,
    count: usize,
}

/// Parses `numkeys key [key ...] <direction> [COUNT count]` starting at
/// `at`, where `at` is the position of `numkeys`.
fn parse_multi_key(argv: &[Bytes], at: usize, name: &str) -> Checked<MultiKey> {
    let numkeys = super::parse_int(&argv[at])?;
    if numkeys <= 0 {
        return Err(Reply::error("ERR numkeys should be greater than 0"));
    }
    let numkeys = numkeys as usize;
    let direction_at = at + 1 + numkeys;
    if direction_at >= argv.len() {
        return Err(Reply::wrong_arity(name));
    }
    let keys = argv[at + 1..direction_at].to_vec();
    let direction = argv[direction_at].clone();

    let count = match argv.len() - direction_at - 1 {
        0 => 1,
        2 if eq_ignore_case(&argv[direction_at + 1], "COUNT") => {
            match super::parse_int(&argv[direction_at + 2])? {
                n if n > 0 => n as usize,
                _ => return Err(Reply::error("ERR count should be greater than 0")),
            }
        }
        _ => return Err(syntax_error()),
    };

    Ok(MultiKey {
        keys,
        direction,
        count,
    })
}

/// BLPOP's answer: the first key with anything in it, popped. Keys are
/// tried in the order given, so a client can express a priority order
/// by listing its queues most-important first.
fn pop_first_list(app: &mut App, keys: &[Bytes], end: End) -> Checked<Option<Reply>> {
    for key in keys {
        check_type(app, key, StoreType::List)?;
        if let Some(value) = pop_one(app, key, end) {
            return Ok(Some(Reply::Array(vec![
                Reply::Bulk(key.clone()),
                Reply::Bulk(value),
            ])));
        }
    }
    Ok(None)
}

fn mpop_lists(app: &mut App, keys: &[Bytes], end: End, count: usize) -> Checked<Option<Reply>> {
    for key in keys {
        check_type(app, key, StoreType::List)?;
        let mut popped = Vec::new();
        while popped.len() < count {
            match pop_one(app, key, end) {
                Some(value) => popped.push(value),
                None => break,
            }
        }
        if !popped.is_empty() {
            return Ok(Some(Reply::Array(vec![
                Reply::Bulk(key.clone()),
                Reply::bulk_array(popped),
            ])));
        }
    }
    Ok(None)
}

/// Pops one member off a sorted set, dropping the key if that emptied
/// it.
fn pop_one_scored(app: &mut App, key: &[u8], edge: Edge, count: usize) -> Vec<(Bytes, f64)> {
    let popped = app
        .store
        .get_existing_zset(key)
        .map(|z| z.pop(count, edge == Edge::Max))
        .unwrap_or_default();
    if !popped.is_empty() {
        app.store.mark_dirty();
        app.store.delete_if_empty(key);
    }
    popped
}

fn pop_first_zset(app: &mut App, keys: &[Bytes], edge: Edge) -> Checked<Option<Reply>> {
    for key in keys {
        check_type(app, key, StoreType::Zset)?;
        if let Some((member, score)) = pop_one_scored(app, key, edge, 1).into_iter().next() {
            // Key, member, and score side by side - BZPOPMIN's reply is
            // ZPOPMIN's with the key that answered in front.
            return Ok(Some(Reply::Array(vec![
                Reply::Bulk(key.clone()),
                Reply::Bulk(member),
                Reply::Double(score),
            ])));
        }
    }
    Ok(None)
}

fn mpop_zsets(app: &mut App, keys: &[Bytes], edge: Edge, count: usize) -> Checked<Option<Reply>> {
    for key in keys {
        check_type(app, key, StoreType::Zset)?;
        let popped = pop_one_scored(app, key, edge, count);
        if !popped.is_empty() {
            // Each member is nested with its score, unlike BZPOPMIN's
            // flat reply - ZMPOP can return more than one.
            let members = popped
                .into_iter()
                .map(|(member, score)| {
                    Reply::Array(vec![Reply::Bulk(member), Reply::Double(score)])
                })
                .collect();
            return Ok(Some(Reply::Array(vec![
                Reply::Bulk(key.clone()),
                Reply::Array(members),
            ])));
        }
    }
    Ok(None)
}
