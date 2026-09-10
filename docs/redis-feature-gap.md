# Klyro vs Redis: feature gap analysis

Command-level and subsystem-level comparison of Klyro against Redis 7.x,
as of 2026-09-10. Complements [roadmap.md](roadmap.md), which covers the
same ground at the architecture level. This document is the detailed
inventory: what exists, what is missing, and which gaps actually block
real workloads.

**Status, 2026-09-10:** Tiers 1, 2, and 3 are built. Klyro speaks RESP,
so stock Redis clients work, and it now has transactions, pub/sub, and
the blocking pops - which between them are why most people reach for
Redis at all. It implements **130 commands**, up from 39. Redis
implements roughly **240**. The gap that remains is not mainly in
count: what is left is subsystem-shaped, not command-shaped, and Tier 4
is where it lives. Sections below are marked **Done** where they have
been closed. See [command-expansion.md](command-expansion.md) and
[connection-state.md](connection-state.md) for the decisions behind
that work.

---

## 1. Blocking gaps (a real workload cannot be built without these)

These are ranked by how often they are the reason someone reaches for
Redis in the first place.

### 1.1 `SET` has no options — no distributed locks — **Done**

`SET key value [NX|XX] [EX seconds|PX milliseconds|KEEPTTL]` is in, so
`SET key value NX EX 30` now takes a lock and its lease atomically.
`SETNX`, `SETEX`, `PSETEX`, `GETSET`, `GETDEL`, and `GETEX` are in too.

`EXAT`, `PXAT`, and the `GET` flag are in too. The suffix-matching
workaround the old line protocol needed is gone: RESP delimits
arguments, so the options parse at fixed positions exactly as Redis
documents them.

### 1.2 No RESP protocol — no real client compatibility — **Done**

Klyro speaks RESP2 and RESP3, negotiated with `HELLO`. redis-py,
go-redis, and ioredis all connect and pass a command sweep with no
adapter; see [resp-protocol.md](resp-protocol.md) for the design and
[client-libraries.md](client-libraries.md) for the verified list.

Everything that hung off this is fixed with it:

- **Values are binary-safe.** Keys and values are `Vec<u8>` end to end,
  so they may hold spaces, newlines, and NUL bytes.
- **Collection members may contain spaces.** `LPUSH`/`SADD`/`ZADD` no
  longer need single-token values.
- **Replies are never silently truncated.** A reply that outgrows
  `client-output-buffer-limit` closes the connection with an error
  rather than returning a partial answer that looks complete.
- **`SET`'s flags parse at fixed positions**, since RESP delimits
  arguments; the old suffix-matching guess is gone.
- **The three hand-written client libraries are retired**, because
  stock clients replace them.

RESP3 push messages and `RESET` arrived with pub/sub. `CLIENT` covers
the subcommands that describe the calling connection - `ID`, `GETNAME`,
`SETNAME`, `SETINFO`, `INFO` - but not `LIST` or `KILL`, which reach
into other connections.

### 1.3 No transactions — **Done**

`MULTI`, `EXEC`, `DISCARD`, `WATCH`, and `UNWATCH` are in, along with
`RESET`. A queue runs with nothing interleaved, which costs nothing to
guarantee on a single-threaded server; `WATCH` is the optimistic-locking
primitive, implemented with a version counter per watched key rather
than by marking other connections dirty. `EXEC` does not roll back a
command that fails at run time, matching Redis, but an error that can be
caught while queueing aborts the whole transaction with `EXECABORT`.
See [connection-state.md](connection-state.md).

Two deviations, both documented in section 6: a successful write that
changed nothing still aborts a watcher, and an expiring key does not.

### 1.4 No scripting or functions

No `EVAL`, `EVALSHA`, `SCRIPT LOAD`, or the 7.0 `FUNCTION` API. Scripting
is the usual escape hatch for atomic read-modify-write, so its absence
compounds 1.1 and 1.3.

### 1.5 No pub/sub — **Done**, except keyspace notifications

`SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`, `PUBLISH`, and
`PUBSUB CHANNELS`/`NUMSUB`/`NUMPAT` are in, and RESP3's push type is
implemented with them. A RESP2 subscriber is restricted to the subscribe
commands, `PING`, `RESET`, and `QUIT`, as in Redis, because RESP2 has no
marker separating a delivered message from a reply; RESP3 lifts the
restriction.

Still missing here: `notify-keyspace-events`, so no expiry-driven or
write-driven callbacks - the key-write signal the rest of this work
added is the hard half of it, and the event vocabulary is the rest.
Sharded pub/sub (`SSUBSCRIBE`, `SPUBLISH`) exists to route messages
across cluster slots, which Klyro has no equivalent of.

### 1.6 No blocking commands — **Done**

`BLPOP`, `BRPOP`, `BLMOVE`, `BRPOPLPUSH`, `BLMPOP`, `BZPOPMIN`,
`BZPOPMAX`, and `BZMPOP` are in, so a Redis-backed job queue works
without busy-polling. The event loop parks a connection, stops feeding
it commands, and re-runs the blocked command from the top whenever a key
it waits on is written; waiters on one key are served oldest first.
`poll`'s timeout is shortened to the nearest deadline so a sub-second
timeout is honoured. `LMPOP` and `ZMPOP`, the non-blocking commands that
share the same argument shape, came with them.

### 1.7 No authentication, ACLs, or TLS

No `AUTH`, no `HELLO`, no `ACL` command family, no `requirepass`, no TLS
listener. The server binds `0.0.0.0` in
[server.rs](../src/server.rs) with full read/write access to anyone who
can reach the port, and there is no protected-mode equivalent.

### 1.8 No `INFO` / `CONFIG` — the server is unobservable — **Done**

`INFO` (six sections), `CONFIG GET`/`SET`/`RESETSTAT`, `ECHO`, and a
config file are in; see [configuration.md](configuration.md). Memory is
measured by a counting global allocator rather than estimated, and the
hit ratio counts read commands only. Nine parameters that used to be
constants in the source are now settable, seven of them at runtime.

`INFO` also reports `blocked_clients`, `watching_clients`,
`pubsub_clients`, `pubsub_channels`, `pubsub_patterns`,
`total_messages_published`, and `total_transactions` now.

Still missing from this family: `CONFIG REWRITE`, `CLIENT LIST`,
`CLIENT KILL`, a real `COMMAND` table, `MONITOR`, `SLOWLOG`, `LATENCY`,
`MEMORY USAGE`, `DEBUG`, `LASTSAVE`, `TIME`, `ROLE`, `WAIT`, and
per-command statistics.

---

## 2. Missing commands, by type

Implemented commands are listed for contrast; everything under
"Missing" is absent.

### 2.1 Generic / keyspace

| Implemented | Missing |
|---|---|
| `DEL`, `UNLINK`, `EXISTS`, `EXPIRE`, `PEXPIRE`, `EXPIREAT`, `PEXPIREAT`, `PERSIST`, `TTL`, `PTTL`, `TYPE`, `RENAME`, `RENAMENX`, `COPY`, `RANDOMKEY`, `KEYS`, `SCAN`, `DBSIZE`, `FLUSHDB`, `FLUSHALL`, `INFO`, `CONFIG`, `ECHO` | `MOVE`, `TOUCH`, `EXPIRETIME`, `PEXPIRETIME`, `SORT`, `SORT_RO`, `DUMP`, `RESTORE`, `MIGRATE`, `OBJECT ENCODING/FREQ/IDLETIME/REFCOUNT`, `SELECT`, `SWAPDB` |

Notes:

- **`EXPIRE` still takes no `NX`/`XX`/`GT`/`LT` flags.**
- **There is still only one database.** No `SELECT`, so no logical
  separation; `FLUSHDB` and `FLUSHALL` are the same operation.
- **`DBSIZE` now filters expired keys**, so it agrees with `KEYS`.

### 2.2 Strings

| Implemented | Missing |
|---|---|
| `SET` (with `NX`/`XX`/`EX`/`PX`/`EXAT`/`PXAT`/`KEEPTTL`/`GET`), `SETNX`, `SETEX`, `PSETEX`, `GET`, `GETSET`, `GETDEL`, `GETEX`, `MGET`, `MSET`, `MSETNX`, `INCR`, `DECR`, `INCRBY`, `DECRBY`, `INCRBYFLOAT`, `APPEND`, `STRLEN`, `GETRANGE`, `SUBSTR`, `SETRANGE` | `LCS` |

### 2.3 Lists

| Implemented | Missing |
|---|---|
| `LPUSH`, `RPUSH`, `LPUSHX`, `RPUSHX`, `LPOP` (with `count`), `RPOP` (with `count`), `LLEN`, `LRANGE`, `LINDEX`, `LSET`, `LINSERT`, `LREM`, `LTRIM`, `RPOPLPUSH`, `LMOVE`, `LMPOP`, `BLPOP`, `BRPOP`, `BLMOVE`, `BRPOPLPUSH`, `BLMPOP` | `LPOS` |

The blocking variants are in, so a work queue no longer has to
busy-poll. `LPOS` is the only list command left.

### 2.4 Hashes

| Implemented | Missing |
|---|---|
| `HSET`, `HSETNX`, `HMSET`, `HGET`, `HMGET`, `HDEL`, `HLEN`, `HEXISTS`, `HKEYS`, `HVALS`, `HGETALL`, `HINCRBY`, `HINCRBYFLOAT`, `HSTRLEN` | `HRANDFIELD`, `HSCAN`, and the 7.4 per-field TTL family (`HEXPIRE`, `HPEXPIRE`, `HTTL`, `HPERSIST`) |

`HSET` and `HMSET` are both variadic now: RESP delimits arguments, so
several field/value pairs in one command are unambiguous.

### 2.5 Sets

| Implemented | Missing |
|---|---|
| `SADD`, `SREM`, `SISMEMBER`, `SMISMEMBER`, `SCARD`, `SMEMBERS`, `SPOP`, `SRANDMEMBER`, `SMOVE`, `SINTER`, `SINTERSTORE`, `SUNION`, `SUNIONSTORE`, `SDIFF`, `SDIFFSTORE` | `SINTERCARD`, `SSCAN` |

The algebra commands clone each source set to work on them together,
because the store hands out one mutable borrow at a time. Fine at this
scale, worth revisiting if sets get large.

### 2.6 Sorted sets

| Implemented | Missing |
|---|---|
| `ZADD`, `ZSCORE`, `ZMSCORE`, `ZINCRBY`, `ZREM`, `ZCARD`, `ZCOUNT`, `ZRANGE`, `ZREVRANGE`, `ZRANGEBYSCORE`, `ZREVRANGEBYSCORE`, `ZRANK`, `ZREVRANK`, `ZREMRANGEBYRANK`, `ZREMRANGEBYSCORE`, `ZPOPMIN`, `ZPOPMAX`, `ZMPOP`, `BZPOPMIN`, `BZPOPMAX`, `BZMPOP` | `ZRANGEBYLEX`, `ZREMRANGEBYLEX`, `ZLEXCOUNT`, `ZRANGESTORE`, `ZRANDMEMBER`, `ZUNION`, `ZUNIONSTORE`, `ZINTER`, `ZINTERCARD`, `ZINTERSTORE`, `ZDIFF`, `ZDIFFSTORE`, `ZSCAN` |

Score-range queries now cover the leaderboard, rate-limiter, and
delayed-queue patterns. Bounds accept `-inf`/`+inf` and the `(`
exclusive prefix.

Still missing: the set-algebra equivalents (`ZUNIONSTORE` and friends),
the lexicographic range family, `ZADD`'s `NX`/`XX`/`GT`/`LT`/`CH`/`INCR`
flags, and a `WITHSCORES` toggle on `ZRANGE` — it always emits scores,
which is a deviation from Redis rather than an omission.

### 2.7 Variadic inconsistency — **Done**

`DEL`, `HDEL`, `SREM`, and `ZREM` are now variadic alongside `SADD`,
`LPUSH`, `RPUSH`, and `ZADD`, and each replies with a count, as Redis
does. `HSET` is variadic too, now that RESP delimits arguments rather
than the value running to the end of the line.

### 2.8 Transactions, pub/sub, and connection

| Implemented | Missing |
|---|---|
| `MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH`, `RESET` | — |
| `SUBSCRIBE`, `UNSUBSCRIBE`, `PSUBSCRIBE`, `PUNSUBSCRIBE`, `PUBLISH`, `PUBSUB CHANNELS`/`NUMSUB`/`NUMPAT` | `SSUBSCRIBE`, `SUNSUBSCRIBE`, `SPUBLISH`, `PUBSUB SHARDCHANNELS`/`SHARDNUMSUB` |
| `HELLO`, `PING`, `ECHO`, `QUIT`, `CLIENT ID`/`GETNAME`/`SETNAME`/`SETINFO`/`INFO` | `AUTH`, `SELECT`, `CLIENT LIST`/`KILL`/`PAUSE`/`NO-EVICT`, `MONITOR` |

The sharded spellings route by cluster slot, which Klyro has no
equivalent of; the `CLIENT` subcommands that are missing are the ones
that reach into connections other than the caller's.

---

## 3. Missing data types

| Type | Redis commands | Typical use |
|---|---|---|
| Bitmaps | `SETBIT`, `GETBIT`, `BITCOUNT`, `BITPOS`, `BITOP`, `BITFIELD` | Compact per-user daily-activity flags |
| HyperLogLog | `PFADD`, `PFCOUNT`, `PFMERGE` | Approximate unique counts in fixed memory |
| Streams | `XADD`, `XREAD`, `XRANGE`, `XGROUP`, `XACK`, `XAUTOCLAIM`, ... | Append-only log with consumer groups |
| Geospatial | `GEOADD`, `GEOSEARCH`, `GEODIST`, `GEOPOS` | Radius / bounding-box lookups |
| JSON, Search, Bloom, Time-series | module commands | Redis Stack features |

Streams are the largest of these and the one with no workaround using
existing types.

---

## 4. Durability, replication, and scale

- **Snapshot-only persistence.** [persist.rs](../src/persist.rs)
  autosaves every 60s when the store is dirty. There is no append-only
  log, so a `SIGKILL` or crash loses up to a minute of writes. No
  `appendfsync` equivalent, no AOF rewrite.
- **`SAVE` blocks the world.** `save_inner` walks the whole keyspace on
  the single event-loop thread. Redis's `BGSAVE` forks and
  copy-on-writes; here every client stalls for the duration of the dump.
  There is no `BGSAVE`, no `BGREWRITEAOF`, and no `LASTSAVE`.
- **Text dump format, not RDB.** Larger on disk, slower to parse, and
  not interchangeable with any Redis tooling.
- **No replication.** No `REPLICAOF`/`SLAVEOF`, no replication backlog,
  no `PSYNC`, no `WAIT`. So no read scaling, no failover, no hot standby.
- **No Sentinel and no Cluster.** Single process, single node, bounded by
  one machine's RAM and one core. No hash slots, no `CLUSTER` command
  family, no `MOVED`/`ASK` redirection.
- ~~**No `maxmemory` and no eviction policy.**~~ **Done (2026-09-10).**
  All eight of Redis's policies (`allkeys-lru`, `volatile-ttl`,
  `allkeys-lfu`, ...), chosen by sampling `maxmemory-samples` keys per
  round as Redis does. Writes that could grow the keyspace are refused
  with `OOM` once eviction cannot free enough; everything that can only
  shrink it still runs. `INFO` reports the limit, the policy, and an
  `evicted_keys` count. See [eviction.md](eviction.md).

---

## 5. Performance characteristics that will not scale

These are correctness-adjacent: they work, but degrade badly with size.

| Area | Current | Redis |
|---|---|---|
| Expired-key sweep | `Store::sweep_expired` scans the **entire** keyspace every second | Samples 20 random keys from the volatile set, adaptively |
| Eviction | Samples `maxmemory-samples` keys per round from an O(1) key index | The same, plus a pool carrying good candidates between rounds |
| `SCAN` | `Store::scan` collects and **sorts every live key on each call** — O(N log N) per call, O(N² log N) for a full iteration | O(1) amortized per call via reverse-binary bucket cursor |
| `KEYS` | Clones every key into a `Vec` before filtering | Streams matches, still O(N) but no full copy |
| Sorted set | `Vec<(String, f64)>` with linear `find_index`; `add`/`rem` are O(N) | Skip list + hash map, O(log N) |
| Event loop | `poll()`, rebuilding the pollfd array each iteration — O(N) per tick in connection count | `epoll`/`kqueue`, O(ready) |
| Threading | Strictly single-threaded | Single-threaded command execution plus optional I/O threads |
| Connections | Bounded by `maxclients`, with a graceful rejection | The same |
| Encodings | One representation per type | listpack / intset / ziplist compaction for small collections |

The `SCAN` implementation deserves special mention: because the cursor is
an index into a freshly sorted snapshot, its cost per call grows with the
keyspace, which defeats the entire purpose of `SCAN` (bounded work per
call so the server stays responsive).

---

## 6. Correctness and semantics deviations

Places where a command exists but behaves differently from Redis:

1. ~~Silent reply truncation at 64 KiB.~~ **Fixed** by the RESP
   rewrite. A reply that outgrows `client-output-buffer-limit` now
   closes the connection with an error instead.
2. ~~`DBSIZE` counts expired-but-unswept keys.~~ **Fixed.** `Store::size`
   filters on liveness, so `DBSIZE` agrees with `KEYS`.
3. ~~`SET`'s option flags are matched as a trailing suffix.~~ **Fixed**
   by the RESP rewrite; they parse at fixed positions now.
4. ~~`SETRANGE` pads with spaces.~~ **Fixed.** It pads with NUL bytes,
   as Redis does, now that values are byte-oriented.
5. **`ZADD` caps at 128 score/member pairs** (`zadd-max-pairs`); Redis
   has no such limit. It is at least configurable.
6. **Expiry uses `SystemTime`**, so a wall-clock adjustment shifts every
   TTL. Redis uses a monotonic-corrected clock.
7. **`SPOP`/`SRANDMEMBER`/`RANDOMKEY` use a xorshift PRNG** seeded from
   the wall clock. Fine for spreading picks around, not suitable
   anywhere randomness needs to be unpredictable.
8. **`INFO` reports a `redis_version`** of 7.0.0, because client
   libraries gate command availability on it. It names the Redis release
   whose command shapes Klyro implements, not a claim to be that server;
   `klyro_version` sits beside it.
9. ~~`HELLO` reports `id: 0` for every connection.~~ **Fixed.**
   Connections carry an id now, which `HELLO`, `CLIENT ID`, and the
   pub/sub and blocking registries all use.
10. **A write that changed nothing still aborts a watching
    transaction.** `SET k v` over an identical value, or a `DEL` of a
    missing key, bumps the watched-key version; Redis signals only on a
    real modification. The error is in the safe direction - the
    transaction retries rather than running on a stale read.
11. **A key expiring does not abort a watching transaction**, which
    matches Redis 6 and later but not earlier versions.
12. **`CLIENT` covers only the calling connection.** `LIST` and `KILL`
    would need a connection registry the command layer can walk;
    connections live in the event loop instead.

## 7. Suggested build order

Ordered by (value delivered / effort), not by size.

**Tier 1 — cheap, high value, no architectural change — Done**

All eight items are built; see [command-expansion.md](command-expansion.md).

**Tier 2 — the two that unlock real workloads — Done**

9. ~~`SET` with `NX`/`XX`/`EX`/`PX`/`KEEPTTL`, plus the `SETNX` family.~~
   **Done** — Klyro can hold a lock now.
10. ~~`INFO` and `CONFIG GET`/`SET`, plus a config file.~~ **Done** —
    Klyro can be operated now. See [configuration.md](configuration.md).

**Tier 3 — the protocol rewrite and what it unblocks — Done**

11. ~~RESP2 parser and serializer, replacing the line protocol.~~
    **Done**, with RESP3 as well. See
    [resp-protocol.md](resp-protocol.md). Stock clients work, values are
    binary-safe, and the silent truncation is gone.
12. ~~`MULTI`/`EXEC`/`WATCH`.~~ **Done.**
13. ~~Pub/sub, then blocking commands~~ (`BLPOP` and friends).
    **Done.** All three needed the same thing - per-connection state,
    and an event loop that can park a connection and wake it - which is
    why they landed together. See
    [connection-state.md](connection-state.md).

**Tier 4 — separable large efforts**

14. ~~`maxmemory` + LRU/LFU eviction.~~ **Done.** See section 4 and
    [eviction.md](eviction.md).
15. AOF with `appendfsync`, plus forked `BGSAVE`.
16. Replication (`REPLICAOF`, `PSYNC`), then Sentinel, then Cluster.
17. `AUTH`/ACL, then TLS.
18. Streams; skip-list sorted set; `epoll`/`kqueue`; bitmaps and HLL.

**Now the top of the list:** item 17, `AUTH`. It is the smallest of the
four remaining subsystems and the one whose absence is hardest to work
around - the server binds `0.0.0.0` with full read/write access to
anyone who can reach the port, so today the only safe deployment is one
nobody else can route to.

**Cheap cleanups worth doing along the way:** `SCAN`'s cursor still
sorts the whole keyspace per call (item in section 5); the expired-key
sweep is still a full scan rather than Redis's sampling, which the
eviction sampler makes a small job now; and `notify-keyspace-events` is
also small, because the write signal it needs already exists, in
[keyspec.rs](../src/commands/keyspec.rs).
