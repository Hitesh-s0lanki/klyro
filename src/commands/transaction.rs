//! MULTI, EXEC, DISCARD, WATCH, UNWATCH and RESET.
//!
//! A transaction here means what it means in Redis: commands are queued
//! and then run back to back on the single event-loop thread, so no
//! other client's command can interleave. It is not a rollback - a
//! command that fails inside EXEC reports its error as one element of
//! the reply, and the rest still run.
//!
//! WATCH adds optimistic locking. Each watched key's stamp is recorded
//! at WATCH time and compared at EXEC; any modification in between
//! aborts the whole transaction.

use super::{exact_args, min_args, Checked, Response};
use crate::app::App;
use crate::resp::Reply;
use crate::session::Session;
use crate::util::bytes::Bytes;

/// Commands that run immediately even inside a MULTI, rather than being
/// queued. Redis has the same list, and UNWATCH is deliberately not on
/// it.
pub fn runs_during_multi(name: &str) -> bool {
    matches!(
        name,
        "MULTI" | "EXEC" | "DISCARD" | "WATCH" | "QUIT" | "RESET"
    )
}

pub fn dispatch(app: &mut App, session: &mut Session, name: &str, argv: &[Bytes]) -> Response {
    match name {
        "EXEC" => return exec(app, session, argv),
        "RESET" => {
            session.reset(&mut app.store);
            return Response::new(Reply::Simple("RESET"));
        }
        _ => {}
    }

    let reply = match handle(app, session, name, argv) {
        Ok(reply) | Err(reply) => reply,
    };
    Response::new(reply)
}

fn handle(app: &mut App, session: &mut Session, name: &str, argv: &[Bytes]) -> Checked<Reply> {
    match name {
        "MULTI" => {
            exact_args(argv, name, 0)?;
            if !session.begin() {
                return Ok(Reply::error("ERR MULTI calls can not be nested"));
            }
            Ok(Reply::ok())
        }

        "DISCARD" => {
            exact_args(argv, name, 0)?;
            if !session.discard() {
                return Ok(Reply::error("ERR DISCARD without MULTI"));
            }
            session.unwatch_all(&mut app.store);
            Ok(Reply::ok())
        }

        "WATCH" => {
            min_args(argv, name, 1)?;
            // Watching after the queue has started would be pointless:
            // the check happens at EXEC, which is already next.
            if session.in_transaction() {
                return Ok(Reply::error("ERR WATCH inside MULTI is not allowed"));
            }
            for key in &argv[1..] {
                session.watch(&mut app.store, key);
            }
            Ok(Reply::ok())
        }

        "UNWATCH" => {
            exact_args(argv, name, 0)?;
            session.unwatch_all(&mut app.store);
            Ok(Reply::ok())
        }

        _ => Ok(Reply::error("ERR unknown command")),
    }
}

/// Runs the queued commands, or explains why it will not.
fn exec(app: &mut App, session: &mut Session, argv: &[Bytes]) -> Response {
    if let Err(reply) = exact_args(argv, "EXEC", 0) {
        return Response::new(reply);
    }
    if !session.in_transaction() {
        return Response::new(Reply::error("ERR EXEC without MULTI"));
    }

    // Read the state before taking the queue, which clears it, and
    // check the watches before releasing them.
    let broken = session.is_broken();
    let queued = session
        .take_queue()
        .expect("in_transaction was just checked");
    let intact = session.watches_intact(&app.store);
    session.unwatch_all(&mut app.store);

    if broken {
        return Response::new(Reply::error(
            "EXECABORT Transaction discarded because of previous errors.",
        ));
    }
    // The optimistic-locking check: if anything watched moved, the
    // transaction does not run, and the client is expected to retry.
    if !intact {
        return Response::new(Reply::NilArray);
    }

    let mut replies = Vec::with_capacity(queued.len());
    let mut close = false;
    for command in &queued {
        let response = super::execute(app, session, command);
        close |= response.close;
        replies.push(response.reply);
    }

    Response {
        reply: Reply::Array(replies),
        close,
        protocol: None,
    }
}
