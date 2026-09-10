//! The pub/sub registry: which clients hold which subscriptions, and
//! who a published message goes to.
//!
//! Subscriptions are stored by client id rather than by connection, so
//! this module has no opinion about sockets. [`PubSub::publish`]
//! answers with the frames to deliver and the ids to deliver them to;
//! the event loop is what writes them.
//!
//! Channel subscriptions and pattern subscriptions are separate maps.
//! A client subscribed to both `news` and `news.*` receives a message
//! on `news` twice, which is what Redis does: the two subscriptions
//! were made independently and are unsubscribed independently.

use std::collections::{BTreeSet, HashMap};

use crate::resp::Reply;
use crate::util::bytes::Bytes;
use crate::util::glob::glob_match;

/// One message to deliver out of band, addressed by client id.
pub struct Delivery {
    pub id: u64,
    pub frame: Reply,
}

#[derive(Default)]
pub struct PubSub {
    channels: HashMap<Bytes, BTreeSet<u64>>,
    patterns: HashMap<Bytes, BTreeSet<u64>>,
}

impl PubSub {
    pub fn new() -> PubSub {
        PubSub::default()
    }

    fn add(table: &mut HashMap<Bytes, BTreeSet<u64>>, name: &[u8], id: u64) {
        table.entry(name.to_vec()).or_default().insert(id);
    }

    fn remove(table: &mut HashMap<Bytes, BTreeSet<u64>>, name: &[u8], id: u64) {
        if let Some(subscribers) = table.get_mut(name) {
            subscribers.remove(&id);
            if subscribers.is_empty() {
                table.remove(name);
            }
        }
    }

    pub fn subscribe(&mut self, channel: &[u8], id: u64) {
        Self::add(&mut self.channels, channel, id);
    }

    pub fn unsubscribe(&mut self, channel: &[u8], id: u64) {
        Self::remove(&mut self.channels, channel, id);
    }

    pub fn psubscribe(&mut self, pattern: &[u8], id: u64) {
        Self::add(&mut self.patterns, pattern, id);
    }

    pub fn punsubscribe(&mut self, pattern: &[u8], id: u64) {
        Self::remove(&mut self.patterns, pattern, id);
    }

    /// Forgets everything a departing connection was subscribed to.
    pub fn drop_client(&mut self, id: u64) {
        self.channels.retain(|_, subscribers| {
            subscribers.remove(&id);
            !subscribers.is_empty()
        });
        self.patterns.retain(|_, subscribers| {
            subscribers.remove(&id);
            !subscribers.is_empty()
        });
    }

    /// Channels with at least one subscriber, optionally filtered by a
    /// glob pattern - PUBSUB CHANNELS. Pattern subscriptions are not
    /// included, as in Redis: a pattern is not a channel.
    pub fn active_channels(&self, pattern: Option<&[u8]>) -> Vec<Bytes> {
        let mut names: Vec<Bytes> = self
            .channels
            .keys()
            .filter(|name| pattern.is_none_or(|p| glob_match(p, name)))
            .cloned()
            .collect();
        names.sort();
        names
    }

    /// Subscribers on one exact channel - PUBSUB NUMSUB.
    pub fn subscriber_count(&self, channel: &[u8]) -> usize {
        self.channels.get(channel).map_or(0, BTreeSet::len)
    }

    /// Distinct patterns subscribed to across all clients - PUBSUB
    /// NUMPAT. It counts patterns, not the clients holding them.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }

    /// Total subscriptions held, for INFO.
    pub fn subscription_count(&self) -> usize {
        let channels: usize = self.channels.values().map(BTreeSet::len).sum();
        let patterns: usize = self.patterns.values().map(BTreeSet::len).sum();
        channels + patterns
    }

    /// The frames a PUBLISH produces, in delivery order: exact-channel
    /// subscribers first, then pattern subscribers. The count of
    /// deliveries is what PUBLISH replies with, and a client
    /// subscribed twice over is counted twice, as Redis counts it.
    pub fn publish(&self, channel: &[u8], payload: &[u8]) -> Vec<Delivery> {
        let mut out = Vec::new();

        if let Some(subscribers) = self.channels.get(channel) {
            for &id in subscribers {
                out.push(Delivery {
                    id,
                    frame: Reply::push(
                        "message",
                        vec![Reply::bulk(channel.to_vec()), Reply::bulk(payload.to_vec())],
                    ),
                });
            }
        }

        for (pattern, subscribers) in &self.patterns {
            if !glob_match(pattern, channel) {
                continue;
            }
            for &id in subscribers {
                out.push(Delivery {
                    id,
                    frame: Reply::push(
                        "pmessage",
                        vec![
                            Reply::bulk(pattern.clone()),
                            Reply::bulk(channel.to_vec()),
                            Reply::bulk(payload.to_vec()),
                        ],
                    ),
                });
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_reaches_channel_and_pattern_subscribers() {
        let mut pubsub = PubSub::new();
        pubsub.subscribe(b"news", 1);
        pubsub.psubscribe(b"ne*", 2);

        let sent = pubsub.publish(b"news", b"hi");
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].id, 1);
        assert_eq!(sent[1].id, 2);
    }

    #[test]
    fn one_client_subscribed_both_ways_is_delivered_to_twice() {
        let mut pubsub = PubSub::new();
        pubsub.subscribe(b"news", 1);
        pubsub.psubscribe(b"news*", 1);
        assert_eq!(pubsub.publish(b"news", b"hi").len(), 2);
    }

    #[test]
    fn a_message_on_a_quiet_channel_goes_nowhere() {
        let pubsub = PubSub::new();
        assert!(pubsub.publish(b"news", b"hi").is_empty());
    }

    #[test]
    fn a_departing_client_leaves_nothing_behind() {
        let mut pubsub = PubSub::new();
        pubsub.subscribe(b"news", 1);
        pubsub.psubscribe(b"ne*", 1);
        pubsub.drop_client(1);
        assert!(pubsub.active_channels(None).is_empty());
        assert_eq!(pubsub.pattern_count(), 0);
    }

    #[test]
    fn channels_are_listed_only_while_someone_listens() {
        let mut pubsub = PubSub::new();
        pubsub.subscribe(b"news", 1);
        pubsub.subscribe(b"sport", 1);
        assert_eq!(pubsub.active_channels(Some(b"n*")), vec![b"news".to_vec()]);
        pubsub.unsubscribe(b"news", 1);
        assert_eq!(pubsub.active_channels(None), vec![b"sport".to_vec()]);
    }
}
