# Transactions, pub/sub, and blocking commands

The three features that turn Klyro from a request/reply key-value store
into something a real workload can be built on landed together on
2026-09-10, because they are the same problem three times: a command
whose effect is not confined to the keyspace and the connection it
arrived on. See [redis-feature-gap.md](redis-feature-gap.md) for what
remains, and [resp-protocol.md](resp-protocol.md) for the protocol work
this builds on.

## The one thing they share

Before this, dispatch was a pure function: arguments in, `Reply` out.
Nothing about a command depended on which connection sent it, and
nothing it produced went anywhere else.

Each of these three breaks that in a different direction.

- **MULTI** has to remember, between commands, that this connection is
  queueing rather than running.
- **PUBLISH** produces output for connections that asked for nothing.
- **BLPOP** produces no output at all, until some other connection
  writes the key it is waiting on.

So dispatch now takes a `&mut Client` beside `&mut App`, and returns a
`Response` that can carry several frames, no frames at all, or a request
to park the connection. `Client` (`src/client.rs`) holds everything that
belongs to one socket: protocol version, MULTI queue, WATCH list,
subscriptions, and whether it is parked. `App` holds the three shared
registries — `watch`, `pubsub`, and the list of blocked clients — keyed
by client id, so none of them knows what a file descriptor is. The event
loop is the only code that maps an id back to a socket.

## Knowing which keys a command wrote

WATCH needs to know when a key it holds changes. A parked BLPOP needs to
know when a key it waits on gets a value. Both are the same question:
*which keys did the command that just ran modify?*

Redis answers it inside every handler, calling `signalModifiedKey` at
each mutation site. Klyro answers it once, from a table
(`src/commands/keyspec.rs`) that names every write command and where its
keys sit in the argument vector:

```rust
("SET", first_key()),            // argv[1]
("MSET", range(1, -1, 2)),       // every other argument
("BLPOP", all_but_timeout()),    // argv[1..] minus the trailing timeout
("LMPOP", Position::Numkeys { at: 1 }),
("FLUSHALL", Position::Everything),
```

Dispatch consults it after every command and signals the keys it names.
A handler stays a function from arguments to a reply, and a new command
cannot forget to signal — it can only be missing from the table, which
is a visible omission rather than a silent one. Commands that failed
their argument checks signal nothing, so a `WRONGTYPE` never aborts an
unrelated transaction.

## WATCH without touching other connections

Redis marks every watching client dirty at the moment of a write, which
means a write reaches into other connections' state. Klyro keeps a
version counter per watched key instead (`src/watch.rs`): `WATCH`
records the version it saw, a write bumps it, and `EXEC` compares. A
transaction's abort decision is made entirely from its own connection's
recorded versions.

Only watched keys are tracked. An entry appears on the first `WATCH` and
disappears when the last watcher drops it, so the map is empty in the
common case, and a write costs one failed hash lookup.

Two deviations from Redis worth naming:

- A write command that ran successfully but changed nothing (`SET k v`
  to the value it already held, `DEL` of a missing key) still bumps the
  version, so it still aborts a watching transaction. Redis signals only
  on a real modification. The error is in the safe direction: a
  transaction retries rather than running on a stale read.
- A key expiring does not bump anything. That matches Redis 6 and later,
  where expiry alone no longer fails an `EXEC`.

`EXEC` runs its queue through the same `execute` path a plain command
takes, so the counters, the subscriber-mode gate, and the write signal
cannot be reached one way and not the other. It does not roll back: a
command that fails at run time leaves its error in the result array and
the rest still run, exactly as Redis behaves. Errors that *can* be
caught while queueing — an unknown command, a `SUBSCRIBE` — are caught
there, and `EXEC` then refuses the whole queue with `EXECABORT` rather
than running the half that parsed.

## Pub/sub and the RESP2 restriction

`PubSub` (`src/pubsub.rs`) maps channels and patterns to sets of client
ids. `publish` returns the frames to deliver and who to deliver them to;
the event loop writes them. Frames for other connections go through
`App`'s outbox; frames for the publisher's own connection are returned
inline, so a client subscribed to a channel it publishes on sees the
message before the delivery count, in the order Redis sends them.

RESP3 gives push messages their own type marker (`>`), so a client can
tell a delivered message from the reply to whatever it asked. RESP2 has
no such marker, which is why a RESP2 connection holding a subscription
may only run the subscribe commands, `PING`, `RESET`, and `QUIT`. Klyro
enforces exactly that, and lifts it entirely for RESP3 — the same rule
Redis 7 uses.

## Parking a connection

A blocking command is the same command twice. It tries the non-blocking
operation; if that finds a value, the reply is what the plain command
would have said. If it does not, the handler returns a `Blocked` naming
the keys to wait on and when to give up, and the event loop stops
reading commands from that connection.

The wake-up is the interesting half:

1. A command runs and signals the keys it wrote.
2. After the poll pass, every parked command whose keys are in that set
   is **re-run from the top**, oldest waiter first.
3. A retry that succeeds sends its reply and unparks the connection. A
   retry that fails — someone else took the value — keeps waiting, on
   the deadline it started with rather than a fresh one.
4. A retry can itself make another key ready. A `BLMOVE` woken by a push
   writes its destination, which may wake a `BLPOP` on that list, so the
   scan repeats until no keys are left ready.

Re-running the whole command is what keeps this small: there is no
separate "resume" path that could drift from the command's own
behaviour, and no per-command state beyond its argument vector.

Three details that are easy to get wrong:

- **A parked connection is still read from.** Its commands go no further
  than the read buffer, but the read is how a disconnect is noticed.
  Without it, a value pushed to a queue would be handed to a socket that
  is already gone and lost. Anything pipelined behind the blocking
  command runs as soon as it is answered.
- **poll's timeout is shortened** to the nearest deadline, so a
  `BLPOP key 0.2` is answered on time rather than at the next
  one-second tick.
- **A blocking command inside MULTI never parks.** Nothing could feed
  it: the connection that would have to send the push is the one sitting
  inside `EXEC`. It answers with its timeout reply immediately, as Redis
  does.

## What this added

Twenty-three commands: `MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH`,
`RESET`, `CLIENT`, `SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`,
`PUNSUBSCRIBE`, `PUBLISH`, `PUBSUB`, `BLPOP`, `BRPOP`, `BLMOVE`,
`BRPOPLPUSH`, `BLMPOP`, `LMPOP`, `BZPOPMIN`, `BZPOPMAX`, `BZMPOP`, and
`ZMPOP`. The last four and `LMPOP`/`ZMPOP` came along because the
blocking machinery already had to parse their argument shape.

`INFO` gained `blocked_clients`, `watching_clients`, `pubsub_clients`,
`pubsub_channels`, `pubsub_patterns`, `total_messages_published`, and
`total_transactions`.

Still missing from this corner: sharded pub/sub (`SSUBSCRIBE` and
friends, which exist for cluster routing Klyro has no equivalent of),
keyspace notifications, and the `CLIENT` subcommands that reach into
*other* connections (`LIST`, `KILL`) — Klyro's connections live in the
event loop rather than in a registry the command layer can walk.
