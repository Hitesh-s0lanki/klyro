//! MULTI, EXEC, DISCARD, WATCH, UNWATCH, and RESET.
//!
//! A transaction here is what it is in Redis: not isolation in the
//! database sense, but a queue that runs with nothing interleaved.
//! Klyro executes one command at a time on one thread, so "nothing
//! interleaved" costs nothing to guarantee - the work is all in the
//! queueing, in refusing to run a queue that failed to build, and in
//! WATCH's optimistic check.
//!
//! What EXEC deliberately does *not* do is roll back. A command that
//! fails at runtime - WRONGTYPE, say - leaves its error in the result
//! array and the ones after it still run, exactly as Redis behaves.
//! Errors that can be caught while queueing are caught there instead,
//! which is what the `failed` flag is for.

use super::{exact_args, min_args, Response};
use crate::app::App;
use crate::client::Client;
use crate::resp::Reply;
use crate::util::bytes::{to_upper, Bytes};

/// The commands MULTI runs immediately instead of queueing.
pub fn is_control(name: &str) -> bool {
    matches!(
        name,
        "MULTI" | "EXEC" | "DISCARD" | "WATCH" | "RESET" | "QUIT"
    )
}

/// Commands that cannot be queued at all: they take over the
/// connection, which is the one thing a transaction cannot hand back.
fn refused_in_multi(name: &str) -> bool {
    matches!(
        name,
        "SUBSCRIBE" | "UNSUBSCRIBE" | "PSUBSCRIBE" | "PUNSUBSCRIBE"
    )
}

/// Adds one command to the open transaction, or records that it could
/// not be added.
pub fn queue(client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    let refusal = if super::family(name).is_none() {
        Some(super::unknown_command(argv))
    } else if refused_in_multi(name) {
        Some(Reply::error(format!(
            "ERR {} is not allowed in transactions",
            name
        )))
    } else {
        None
    };

    let transaction = client
        .transaction
        .as_mut()
        .expect("only called while a transaction is open");

    match refusal {
        Some(error) => {
            // EXEC will refuse the whole queue rather than run the
            // commands that did parse.
            transaction.failed = true;
            Response::new(error)
        }
        None => {
            transaction.queued.push(argv.to_vec());
            Response::new(Reply::Simple("QUEUED"))
        }
    }
}

pub fn dispatch(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    match name {
        "MULTI" => match exact_args(argv, name, 0) {
            Err(error) => Response::new(error),
            Ok(()) if client.in_transaction() => {
                Response::new(Reply::error("ERR MULTI calls can not be nested"))
            }
            Ok(()) => {
                client.begin_transaction();
                Response::new(Reply::ok())
            }
        },

        "DISCARD" => match exact_args(argv, name, 0) {
            Err(error) => Response::new(error),
            Ok(()) if !client.in_transaction() => {
                Response::new(Reply::error("ERR DISCARD without MULTI"))
            }
            Ok(()) => {
                discard(app, client);
                Response::new(Reply::ok())
            }
        },

        "WATCH" => match min_args(argv, name, 1) {
            Err(error) => Response::new(error),
            Ok(()) if client.in_transaction() => {
                Response::new(Reply::error("ERR WATCH inside MULTI is not allowed"))
            }
            Ok(()) => {
                for key in &argv[1..] {
                    watch_one(app, client, key);
                }
                Response::new(Reply::ok())
            }
        },

        "UNWATCH" => match exact_args(argv, name, 0) {
            Err(error) => Response::new(error),
            Ok(()) => {
                app.unwatch_all(client);
                Response::new(Reply::ok())
            }
        },

        "EXEC" => match exact_args(argv, name, 0) {
            Err(error) => Response::new(error),
            Ok(()) => exec(app, client),
        },

        "RESET" => {
            discard(app, client);
            unsubscribe_everything(app, client);
            Response::new(Reply::Simple("RESET"))
        }

        _ => Response::new(Reply::error("ERR unknown command")),
    }
}

/// Watching the same key twice is one watch, as in Redis: the second
/// WATCH would otherwise take a version reading *after* a modification
/// the first one was meant to catch.
fn watch_one(app: &mut App, client: &mut Client, key: &[u8]) {
    if client.watched.iter().any(|(watched, _)| watched == key) {
        return;
    }
    let version = app.watch.watch(key);
    client.watched.push((key.to_vec(), version));
}

/// Throws away the queue and the watch list together. DISCARD,
/// RESET, and the end of every EXEC all leave the connection here.
fn discard(app: &mut App, client: &mut Client) {
    client.transaction = None;
    app.unwatch_all(client);
}

fn unsubscribe_everything(app: &mut App, client: &mut Client) {
    for channel in std::mem::take(&mut client.channels) {
        app.pubsub.unsubscribe(&channel, client.id);
    }
    for pattern in std::mem::take(&mut client.patterns) {
        app.pubsub.punsubscribe(&pattern, client.id);
    }
}

/// Whether any watched key has been modified since it was watched.
fn watch_broken(app: &App, client: &Client) -> bool {
    client
        .watched
        .iter()
        .any(|(key, version)| app.watch.version(key) != Some(*version))
}

fn exec(app: &mut App, client: &mut Client) -> Response {
    let Some(transaction) = client.transaction.take() else {
        return Response::new(Reply::error("ERR EXEC without MULTI"));
    };
    let broken = watch_broken(app, client);
    app.unwatch_all(client);

    if transaction.failed {
        return Response::new(Reply::error(
            "EXECABORT Transaction discarded because of previous errors.",
        ));
    }
    // A null array, not an empty one: the caller has to be able to tell
    // "ran and produced nothing" from "did not run".
    if broken {
        return Response::new(Reply::NilArray);
    }

    // Nothing can interleave here - this is a single-threaded server
    // running a loop - but the flag still matters, because a blocking
    // command inside a transaction must answer rather than park.
    app.stats.transactions += 1;
    client.in_exec = true;
    let mut results = Vec::with_capacity(transaction.queued.len());
    let mut close = false;
    let mut protocol = None;
    for queued in &transaction.queued {
        let name = to_upper(&queued[0]);
        let response = super::execute(app, client, &name, queued);
        close |= response.close;
        protocol = response.protocol.or(protocol);
        results.push(response.single());
    }
    client.in_exec = false;

    Response {
        replies: vec![Reply::Array(results)],
        close,
        protocol,
        block: None,
    }
}
