//! Per-connection state.
//!
//! The keyspace is shared, but a MULTI queue, a WATCH list, pub/sub
//! subscriptions, and a parked blocking command all belong to exactly
//! one connection. The event loop keeps a [`Client`] beside every
//! socket and hands it to dispatch alongside `App`, so a command can
//! read and change the connection it arrived on without the command
//! layer knowing that sockets exist.

use std::collections::BTreeSet;
use std::time::Instant;

use crate::resp::{Protocol, Reply};
use crate::util::bytes::Bytes;

/// A MULTI in progress: the commands queued so far, and whether one of
/// them was refused.
pub struct Transaction {
    pub queued: Vec<Vec<Bytes>>,
    /// Set when a command could not be queued at all. EXEC then runs
    /// nothing, which is what keeps a transaction from executing the
    /// half of itself that did parse.
    pub failed: bool,
}

impl Transaction {
    fn new() -> Transaction {
        Transaction {
            queued: Vec::new(),
            failed: false,
        }
    }
}

pub struct Client {
    pub id: u64,
    /// Negotiated by HELLO, and RESP2 until then - the version every
    /// client starts out speaking.
    pub protocol: Protocol,
    pub addr: String,
    /// Set by CLIENT SETNAME; empty until then.
    pub name: Bytes,
    pub created: Instant,
    pub transaction: Option<Transaction>,
    /// Watched keys, each with the version it carried when WATCH ran.
    /// EXEC compares these against the registry to decide whether to
    /// run at all.
    pub watched: Vec<(Bytes, u64)>,
    pub channels: BTreeSet<Bytes>,
    pub patterns: BTreeSet<Bytes>,
    /// True only while EXEC is running this client's queue. Blocking
    /// commands read it, because a transaction must never park: the
    /// only client that could unblock it is the one holding the
    /// server.
    pub in_exec: bool,
    /// True while a blocking command is parked. The event loop stops
    /// reading commands from a client in this state; the pending
    /// request sits in the registry on `App` until a key it waits on
    /// changes or its deadline passes.
    pub blocked: bool,
}

impl Client {
    pub fn new(id: u64, addr: String) -> Client {
        Client {
            id,
            protocol: Protocol::Resp2,
            addr,
            name: Vec::new(),
            created: Instant::now(),
            transaction: None,
            watched: Vec::new(),
            channels: BTreeSet::new(),
            patterns: BTreeSet::new(),
            in_exec: false,
            blocked: false,
        }
    }

    pub fn begin_transaction(&mut self) {
        self.transaction = Some(Transaction::new());
    }

    pub fn in_transaction(&self) -> bool {
        self.transaction.is_some()
    }

    /// Every subscription this client holds, channels and patterns
    /// together - the count Redis reports back on every subscribe and
    /// unsubscribe confirmation.
    pub fn subscription_count(&self) -> usize {
        self.channels.len() + self.patterns.len()
    }

    /// Whether this connection is in subscriber mode. A RESP2 client
    /// here may only run the subscribe commands, PING, RESET, and QUIT:
    /// there is no push marker in RESP2, so anything else would be
    /// indistinguishable from a delivered message.
    pub fn is_subscriber(&self) -> bool {
        self.subscription_count() > 0
    }
}

/// A blocking command that has been parked, held centrally on `App`
/// rather than on the connection so the wake-up scan is one pass over
/// waiting clients instead of one over every socket.
pub struct Blocked {
    pub id: u64,
    /// The keys whose modification should make the command retry.
    pub keys: Vec<Bytes>,
    /// `None` for a timeout of 0, which means wait forever.
    pub deadline: Option<Instant>,
    /// The original command, re-run whenever one of `keys` changes.
    pub argv: Vec<Bytes>,
    /// What to answer once the deadline passes. BLPOP's family says
    /// null array; BLMOVE's says null.
    pub on_timeout: Reply,
}
