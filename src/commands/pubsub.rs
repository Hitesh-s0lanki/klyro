//! SUBSCRIBE, PSUBSCRIBE, their opposites, PUBLISH, and PUBSUB.
//!
//! Subscribing is the one thing a command does that changes what the
//! connection *is* rather than what the keyspace holds, and publishing
//! is the one thing a command does that produces output for somebody
//! else. The first is why these handlers take the client; the second
//! is why they hand messages for other connections to `App`'s outbox
//! instead of returning them.

use super::{exact_args, min_args, Response};
use crate::app::App;
use crate::client::Client;
use crate::resp::Reply;
use crate::util::bytes::{to_upper, Bytes};

pub fn dispatch(app: &mut App, client: &mut Client, name: &str, argv: &[Bytes]) -> Response {
    match name {
        "SUBSCRIBE" | "PSUBSCRIBE" => match min_args(argv, name, 1) {
            Err(error) => Response::new(error),
            Ok(()) => Response::frames(subscribe(app, client, name == "PSUBSCRIBE", &argv[1..])),
        },

        "UNSUBSCRIBE" | "PUNSUBSCRIBE" => {
            Response::frames(unsubscribe(app, client, name == "PUNSUBSCRIBE", &argv[1..]))
        }

        "PUBLISH" => match exact_args(argv, name, 2) {
            Err(error) => Response::new(error),
            Ok(()) => publish(app, client, &argv[1], &argv[2]),
        },

        "PUBSUB" => match min_args(argv, name, 1) {
            Err(error) => Response::new(error),
            Ok(()) => Response::new(introspect(app, argv)),
        },

        _ => Response::new(Reply::error("ERR unknown command")),
    }
}

/// The confirmation sent for each channel named, whichever direction
/// it went. The trailing count is every subscription the connection
/// holds, channels and patterns together, which is how a client knows
/// when it has left subscriber mode.
fn confirmation(kind: &'static str, name: Reply, client: &Client) -> Reply {
    Reply::push(
        kind,
        vec![name, Reply::Integer(client.subscription_count() as i64)],
    )
}

fn subscribe(app: &mut App, client: &mut Client, patterns: bool, names: &[Bytes]) -> Vec<Reply> {
    let mut frames = Vec::with_capacity(names.len());
    for name in names {
        // Subscribing twice to the same channel is a no-op that is
        // still confirmed, so a client counting replies sees one per
        // argument it sent.
        if patterns {
            if client.patterns.insert(name.clone()) {
                app.pubsub.psubscribe(name, client.id);
            }
        } else if client.channels.insert(name.clone()) {
            app.pubsub.subscribe(name, client.id);
        }
        let kind = if patterns { "psubscribe" } else { "subscribe" };
        frames.push(confirmation(kind, Reply::bulk(name.clone()), client));
    }
    frames
}

fn unsubscribe(app: &mut App, client: &mut Client, patterns: bool, names: &[Bytes]) -> Vec<Reply> {
    // No arguments means "all of them", which is also the case where
    // there may be nothing to confirm.
    let targets: Vec<Bytes> = if names.is_empty() {
        if patterns {
            client.patterns.iter().cloned().collect()
        } else {
            client.channels.iter().cloned().collect()
        }
    } else {
        names.to_vec()
    };

    let kind = if patterns {
        "punsubscribe"
    } else {
        "unsubscribe"
    };

    if targets.is_empty() {
        // Redis still answers, with a null channel name, so a client
        // that unsubscribes from nothing is not left waiting.
        return vec![confirmation(kind, Reply::Nil, client)];
    }

    let mut frames = Vec::with_capacity(targets.len());
    for name in targets {
        if patterns {
            if client.patterns.remove(&name) {
                app.pubsub.punsubscribe(&name, client.id);
            }
        } else if client.channels.remove(&name) {
            app.pubsub.unsubscribe(&name, client.id);
        }
        frames.push(confirmation(kind, Reply::bulk(name), client));
    }
    frames
}

/// Sends `payload` to every subscriber and replies with how many
/// received it.
///
/// A client subscribed to both a matching channel and a matching
/// pattern is counted - and delivered to - twice, because it made two
/// subscriptions and will unsubscribe them separately.
fn publish(app: &mut App, client: &Client, channel: &[u8], payload: &[u8]) -> Response {
    let deliveries = app.pubsub.publish(channel, payload);
    let received = deliveries.len() as i64;
    app.stats.messages_published += 1;

    // A publisher subscribed to its own channel gets the message on
    // the same pass, ahead of the count, rather than through the
    // outbox: only frames for other connections need to leave here.
    let mut frames = Vec::new();
    for delivery in deliveries {
        if delivery.id == client.id {
            frames.push(delivery.frame);
        } else {
            app.outbox.push((delivery.id, delivery.frame));
        }
    }
    frames.push(Reply::Integer(received));
    Response::frames(frames)
}

fn introspect(app: &mut App, argv: &[Bytes]) -> Reply {
    match to_upper(&argv[1]).as_str() {
        "CHANNELS" => match argv.len() {
            2 => Reply::bulk_array(app.pubsub.active_channels(None)),
            3 => Reply::bulk_array(app.pubsub.active_channels(Some(&argv[2]))),
            _ => Reply::wrong_arity("pubsub|channels"),
        },

        // A flat channel, count, channel, count array - the shape a
        // client unpacks into a map.
        "NUMSUB" => Reply::Map(
            argv[2..]
                .iter()
                .map(|channel| {
                    (
                        Reply::bulk(channel.clone()),
                        Reply::Integer(app.pubsub.subscriber_count(channel) as i64),
                    )
                })
                .collect(),
        ),

        // Patterns, not the clients holding them: two clients on the
        // same pattern count once.
        "NUMPAT" => Reply::Integer(app.pubsub.pattern_count() as i64),

        other => Reply::error(format!(
            "ERR Unknown PUBSUB subcommand or wrong number of arguments for '{}'",
            other
        )),
    }
}
