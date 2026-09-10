//! List commands.

use super::{check_type, exact_args, min_args, parse_int, syntax_error, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::types::list;
use crate::util::bytes::{eq_ignore_case, Bytes};

/// Which end of a list an operation works on.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum End {
    Left,
    Right,
}

impl End {
    pub(super) fn parse(token: &[u8]) -> Option<End> {
        if eq_ignore_case(token, "LEFT") {
            Some(End::Left)
        } else if eq_ignore_case(token, "RIGHT") {
            Some(End::Right)
        } else {
            None
        }
    }
}

fn pop_from(l: &mut list::List, end: End) -> Option<Bytes> {
    match end {
        End::Left => l.pop_front(),
        End::Right => l.pop_back(),
    }
}

fn push_to(l: &mut list::List, end: End, value: Bytes) {
    match end {
        End::Left => l.push_front(value),
        End::Right => l.push_back(value),
    }
}

/// Pops one value off `end` of the list at `key`, dropping the key if
/// that emptied it. `None` when the key holds no list or the list is
/// empty; the caller is expected to have type-checked the key already.
///
/// Shared with the blocking pops, which is the whole reason it exists:
/// BLPOP has to be exactly LPOP when the list is not empty.
pub(super) fn pop_one(app: &mut App, key: &[u8], end: End) -> Option<Bytes> {
    let value = app
        .store
        .get_existing_list(key)
        .and_then(|l| pop_from(l, end))?;
    app.store.mark_dirty();
    app.store.delete_if_empty(key);
    Some(value)
}

/// Moves one value from `source`'s `from` end to `destination`'s `to`
/// end - the body of RPOPLPUSH, LMOVE, and both blocking spellings.
pub(super) fn move_one(
    app: &mut App,
    source: &[u8],
    destination: &[u8],
    from: End,
    to: End,
) -> Option<Bytes> {
    let value = app
        .store
        .get_existing_list(source)
        .and_then(|l| pop_from(l, from))?;
    app.store.mark_dirty();
    // Rotating a list onto itself is legal, so push before the
    // empty-cleanup runs - otherwise a one-element self-move would
    // delete the key it is about to write back into.
    let target = app
        .store
        .get_or_create_list(destination)
        .expect("type checked");
    push_to(target, to, value.clone());
    app.store.delete_if_empty(source);
    Some(value)
}

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "LPUSH" | "RPUSH" | "LPUSHX" | "RPUSHX" => {
            min_args(argv, name, 2)?;
            let key = &argv[1];
            let end = if name.starts_with('L') {
                End::Left
            } else {
                End::Right
            };
            check_type(app, key, StoreType::List)?;
            if name.ends_with('X') && !app.store.exists(key) {
                return Ok(Reply::Integer(0));
            }
            let l = app.store.get_or_create_list(key).expect("type checked");
            for value in &argv[2..] {
                push_to(l, end, value.clone());
            }
            Ok(Reply::Integer(l.len() as i64))
        }

        "LPOP" | "RPOP" => {
            min_args(argv, name, 1)?;
            let key = &argv[1];
            let end = if name == "LPOP" {
                End::Left
            } else {
                End::Right
            };
            let count = match argv.len() {
                2 => None,
                3 => match parse_int(&argv[2])? {
                    c if c >= 0 => Some(c as usize),
                    _ => return Ok(Reply::error("ERR value is out of range, must be positive")),
                },
                _ => return Err(Reply::wrong_arity(name)),
            };
            check_type(app, key, StoreType::List)?;

            let mut popped = Vec::new();
            if let Some(l) = app.store.get_existing_list(key) {
                for _ in 0..count.unwrap_or(1) {
                    match pop_from(l, end) {
                        Some(v) => popped.push(v),
                        None => break,
                    }
                }
            }
            app.store.delete_if_empty(key);

            Ok(match count {
                // Without a count: one value, or nil. With one: an
                // array, or a nil array when the key is missing.
                None => popped.into_iter().next().map_or(Reply::Nil, Reply::Bulk),
                Some(_) if popped.is_empty() => Reply::NilArray,
                Some(_) => Reply::bulk_array(popped),
            })
        }

        "LLEN" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::List)?;
            let len = app.store.get_existing_list(&argv[1]).map_or(0, |l| l.len());
            Ok(Reply::Integer(len as i64))
        }

        "LRANGE" => {
            exact_args(argv, name, 3)?;
            let (start, stop) = (parse_int(&argv[2])?, parse_int(&argv[3])?);
            check_type(app, &argv[1], StoreType::List)?;
            let values: Vec<Bytes> = app
                .store
                .get_existing_list(&argv[1])
                .map(|l| {
                    list::range(l, start, stop)
                        .into_iter()
                        .map(<[u8]>::to_vec)
                        .collect()
                })
                .unwrap_or_default();
            Ok(Reply::bulk_array(values))
        }

        "LINDEX" => {
            exact_args(argv, name, 2)?;
            let index = parse_int(&argv[2])?;
            check_type(app, &argv[1], StoreType::List)?;
            let value = app
                .store
                .get_existing_list(&argv[1])
                .and_then(|l| list::resolve_index(l.len(), index).and_then(|i| l.get(i).cloned()));
            Ok(value.map_or(Reply::Nil, Reply::Bulk))
        }

        "LSET" => {
            exact_args(argv, name, 3)?;
            let index = parse_int(&argv[2])?;
            check_type(app, &argv[1], StoreType::List)?;
            let Some(l) = app.store.get_existing_list(&argv[1]) else {
                return Ok(Reply::error("ERR no such key"));
            };
            match list::resolve_index(l.len(), index) {
                Some(i) => {
                    l[i] = argv[3].clone();
                    Ok(Reply::ok())
                }
                None => Ok(Reply::error("ERR index out of range")),
            }
        }

        "LINSERT" => {
            exact_args(argv, name, 4)?;
            let before = if eq_ignore_case(&argv[2], "BEFORE") {
                true
            } else if eq_ignore_case(&argv[2], "AFTER") {
                false
            } else {
                return Err(syntax_error());
            };
            check_type(app, &argv[1], StoreType::List)?;
            let new_len = app
                .store
                .get_existing_list(&argv[1])
                .and_then(|l| list::insert(l, before, &argv[3], &argv[4]));
            // -1 means the pivot is absent; 0 means the key is.
            Ok(Reply::Integer(match new_len {
                Some(len) => len as i64,
                None if app.store.exists(&argv[1]) => -1,
                None => 0,
            }))
        }

        "LREM" => {
            exact_args(argv, name, 3)?;
            let count = parse_int(&argv[2])?;
            check_type(app, &argv[1], StoreType::List)?;
            let removed = app
                .store
                .get_existing_list(&argv[1])
                .map_or(0, |l| list::remove(l, count, &argv[3]));
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "LTRIM" => {
            exact_args(argv, name, 3)?;
            let (start, stop) = (parse_int(&argv[2])?, parse_int(&argv[3])?);
            check_type(app, &argv[1], StoreType::List)?;
            if let Some(l) = app.store.get_existing_list(&argv[1]) {
                list::trim(l, start, stop);
            }
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::ok())
        }

        "RPOPLPUSH" | "LMOVE" => {
            let (from, to) = if name == "RPOPLPUSH" {
                exact_args(argv, name, 2)?;
                (End::Right, End::Left)
            } else {
                exact_args(argv, name, 4)?;
                match (End::parse(&argv[3]), End::parse(&argv[4])) {
                    (Some(f), Some(t)) => (f, t),
                    _ => return Err(syntax_error()),
                }
            };
            let (source, destination) = (argv[1].clone(), argv[2].clone());
            check_type(app, &source, StoreType::List)?;
            check_type(app, &destination, StoreType::List)?;

            Ok(move_one(app, &source, &destination, from, to).map_or(Reply::Nil, Reply::Bulk))
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}
