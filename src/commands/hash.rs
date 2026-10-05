//! Hash commands.

use super::{check_type, exact_args, min_args, not_a_float, parse_float, parse_int, Checked};
use crate::app::App;
use crate::resp::Reply;
use crate::store::StoreType;
use crate::util::bytes::{format_f64, parse_f64, parse_i64, Bytes};

pub fn dispatch(app: &mut App, name: &str, argv: &[Bytes]) -> Reply {
    match handle(app, name, argv) {
        Ok(reply) | Err(reply) => reply,
    }
}

fn handle(app: &mut App, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        // HSET is variadic now that RESP delimits arguments; HMSET is
        // the same command with Redis's older reply.
        "HSET" | "HMSET" => {
            min_args(argv, name, 3)?;
            if !argv[2..].len().is_multiple_of(2) {
                return Err(Reply::wrong_arity(name));
            }
            check_type(app, &argv[1], StoreType::Hash)?;
            let h = app
                .store
                .get_or_create_hash(&argv[1])
                .expect("type checked");
            let mut added = 0;
            for pair in argv[2..].chunks(2) {
                if h.insert(pair[0].clone(), pair[1].clone()).is_none() {
                    added += 1;
                }
            }
            Ok(if name == "HMSET" {
                Reply::ok()
            } else {
                Reply::Integer(added)
            })
        }

        "HSETNX" => {
            exact_args(argv, name, 3)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let h = app
                .store
                .get_or_create_hash(&argv[1])
                .expect("type checked");
            if h.contains_key(&argv[2]) {
                // The key may have just been created empty, so clean up.
                app.store.delete_if_empty(&argv[1]);
                return Ok(Reply::bool(false));
            }
            h.insert(argv[2].clone(), argv[3].clone());
            Ok(Reply::bool(true))
        }

        "HGET" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let value = app
                .store
                .read_hash(&argv[1])
                .and_then(|h| h.get(&argv[2]).cloned());
            Ok(value.map_or(Reply::Nil, Reply::Bulk))
        }

        "HMGET" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let hash = app.store.read_hash(&argv[1]).cloned();
            let values = argv[2..]
                .iter()
                .map(|field| match hash.as_ref().and_then(|h| h.get(field)) {
                    Some(v) => Reply::bulk(v.clone()),
                    None => Reply::Nil,
                })
                .collect();
            Ok(Reply::array(values))
        }

        "HDEL" => {
            min_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let removed = match app.store.write_hash(&argv[1]) {
                Some(h) => argv[2..].iter().filter(|f| h.remove(*f).is_some()).count(),
                None => 0,
            };
            app.store.delete_if_empty(&argv[1]);
            Ok(Reply::Integer(removed as i64))
        }

        "HLEN" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let len = app.store.read_hash(&argv[1]).map_or(0, |h| h.len());
            Ok(Reply::Integer(len as i64))
        }

        "HEXISTS" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let present = app
                .store
                .read_hash(&argv[1])
                .is_some_and(|h| h.contains_key(&argv[2]));
            Ok(Reply::bool(present))
        }

        "HSTRLEN" => {
            exact_args(argv, name, 2)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let len = app
                .store
                .read_hash(&argv[1])
                .and_then(|h| h.get(&argv[2]).map(|v| v.len()))
                .unwrap_or(0);
            Ok(Reply::Integer(len as i64))
        }

        "HKEYS" | "HVALS" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let wants_keys = name == "HKEYS";
            let items: Vec<Bytes> = app
                .store
                .read_hash(&argv[1])
                .map(|h| {
                    h.iter()
                        .map(|(f, v)| if wants_keys { f.clone() } else { v.clone() })
                        .collect()
                })
                .unwrap_or_default();
            Ok(Reply::bulk_array(items))
        }

        "HGETALL" => {
            exact_args(argv, name, 1)?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let mut pairs = Vec::new();
            if let Some(hash) = app.store.read_hash(&argv[1]) {
                for (field, value) in hash.iter() {
                    pairs.push((Reply::bulk(field.clone()), Reply::bulk(value.clone())));
                }
            }
            Ok(Reply::Map(pairs))
        }

        "HINCRBY" => {
            exact_args(argv, name, 3)?;
            let delta = parse_int(&argv[3])?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let current = app
                .store
                .read_hash(&argv[1])
                .and_then(|h| h.get(&argv[2]).cloned());
            let base = match current {
                None => 0,
                Some(v) => match parse_i64(&v) {
                    Some(n) => n,
                    None => return Ok(Reply::error("ERR hash value is not an integer")),
                },
            };
            let Some(updated) = base.checked_add(delta) else {
                return Ok(Reply::error("ERR increment or decrement would overflow"));
            };
            app.store
                .get_or_create_hash(&argv[1])
                .expect("type checked")
                .insert(argv[2].clone(), updated.to_string().into_bytes());
            Ok(Reply::Integer(updated))
        }

        "HINCRBYFLOAT" => {
            exact_args(argv, name, 3)?;
            let delta = parse_float(&argv[3])?;
            check_type(app, &argv[1], StoreType::Hash)?;
            let current = app
                .store
                .read_hash(&argv[1])
                .and_then(|h| h.get(&argv[2]).cloned());
            let base = match current {
                None => 0.0,
                Some(v) => parse_f64(&v).ok_or_else(not_a_float)?,
            };
            let sum = base + delta;
            if !sum.is_finite() {
                return Ok(Reply::error("ERR increment would produce NaN or Infinity"));
            }
            let text = format_f64(sum);
            app.store
                .get_or_create_hash(&argv[1])
                .expect("type checked")
                .insert(argv[2].clone(), text.clone());
            Ok(Reply::Bulk(text))
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}
