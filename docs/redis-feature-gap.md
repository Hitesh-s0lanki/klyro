# Klyro vs Redis: feature gap analysis

Command-level and subsystem-level comparison of Klyro against Redis 7.x,
as of 2026-09-10. Complements [roadmap.md](roadmap.md), which covers the
same ground at the architecture level. This document is the detailed
inventory: what exists, what is missing, and which gaps actually block
real workloads.

**Status, 2026-09-10:** Tiers 1 and 2 are built, and Tier 3 is under
way: the protocol rewrite and transactions are done. Klyro speaks RESP, so stock Redis clients work, and
implements **117 commands**, up from 39. Redis implements roughly
**240**. The gap that remains is not mainly in count: the load-bearing
pieces left are protocol- and subsystem-shaped, not command-shaped.
Sections below are marked **Done** where they have been closed. See
[command-expansion.md](command-expansion.md) for the decisions behind
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

Still missing: RESP3 push messages (nothing to push without pub/sub),
and `RESET`/`CLIENT`.

### 1.3 No transactions — **Done**

`MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH` and `RESET` are in, with
Redis's semantics: queued commands run back to back on the single
event-loop thread, a run-time error inside `EXEC` is reported per
command rather than rolling anything back, and `WATCH` gives optimistic
locking that aborts `EXEC` if a watched key moved. See
[transactions.md](transactions.md).

One deviation: arity is not checked at queue time, only at `EXEC`. Redis
catches it earlier. Unknown commands *are* caught at queue time and
abort the transaction with `EXECABORT`.

Building this uncovered a durability bug: mutations that left a
collection non-empty never marked the store dirty, so the autosave could
skip them. Fixed by splitting the collection accessors into read and
write halves - see the same document.

### 1.4 No scripting or functions

No `EVAL`, `EVALSHA`, `SCRIPT LOAD`, or the 7.0 `FUNCTION` API. Scripting
is the usual escape hatch for atomic read-modify-write, so its absence
compounds 1.1 and 1.3.

### 1.5 No pub/sub and no keyspace notifications

No `SUBSCRIBE`, `UNSUBSCRIBE`, `PUBLISH`, `PSUBSCRIBE`, `SSUBSCRIBE`
(sharded pub/sub), and no `notify-keyspace-events`. Rules out fan-out
messaging, cache-invalidation broadcasts, and expiry-driven callbacks.

### 1.6 No blocking commands — no work queues

No `BLPOP`, `BRPOP`, `BLMOVE`, `BLMPOP`, `BZPOPMIN`, `BZPOPMAX`. A
Redis-backed job queue is normally a blocking pop; without it, consumers
must busy-poll. The single-threaded `poll()` loop in
[server.rs](../src/server.rs) has no concept of a parked client, so this
needs a per-key waiter registry before any of these can be added.

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

Still missing from this family: `CONFIG REWRITE`, `CLIENT LIST`,
`CLIENT KILL`, `COMMAND`, `MONITOR`, `SLOWLOG`, `LATENCY`,
`MEMORY USAGE`, `DEBUG`, `LASTSAVE`, `TIME`, `RESET`, `ROLE`, `WAIT`,
and per-command statistics.

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
| `SET` (with `NX`/`XX`/`EX`/`PX`/`KEEPTTL`), `SETNX`, `SETEX`, `PSETEX`, `GET`, `GETSET`, `GETDEL`, `GETEX`, `MGET`, `MSET`, `INCR`, `DECR`, `INCRBY`, `DECRBY`, `INCRBYFLOAT`, `APPEND`, `STRLEN`, `GETRANGE`, `SETRANGE` | `MSETNX`, `SUBSTR`, `LCS`, and `SET`'s `GET` flag |

`MSET` and `HMSET` take single-token values, unlike `SET`/`HSET`, since
there is no other way to tell the pairs apart in a line protocol.

### 2.3 Lists

| Implemented | Missing |
|---|---|
| `LPUSH`, `RPUSH`, `LPUSHX`, `RPUSHX`, `LPOP` (with `count`), `RPOP` (with `count`), `LLEN`, `LRANGE`, `LINDEX`, `LSET`, `LINSERT`, `LREM`, `LTRIM`, `RPOPLPUSH`, `LMOVE` | `LPOS`, `LMPOP`, plus all blocking variants (`BLPOP`, `BRPOP`, `BLMOVE`, `BLMPOP`) |

The blocking variants are the real remaining gap here: they are what a
Redis-backed job queue is built on, and they need connection-parking
machinery the event loop doesn't have yet (see 1.6).

### 2.4 Hashes

| Implemented | Missing |
|---|---|
| `HSET`, `HSETNX`, `HMSET`, `HGET`, `HMGET`, `HDEL`, `HLEN`, `HEXISTS`, `HKEYS`, `HVALS`, `HGETALL`, `HINCRBY`, `HINCRBYFLOAT`, `HSTRLEN` | `HRANDFIELD`, `HSCAN`, and the 7.4 per-field TTL family (`HEXPIRE`, `HPEXPIRE`, `HTTL`, `HPERSIST`) |

`HSET` still takes exactly one field/value pair, because its value is
the rest of the line; `HMSET` is the variadic spelling.

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
| `ZADD`, `ZSCORE`, `ZMSCORE`, `ZINCRBY`, `ZREM`, `ZCARD`, `ZCOUNT`, `ZRANGE`, `ZREVRANGE`, `ZRANGEBYSCORE`, `ZREVRANGEBYSCORE`, `ZRANK`, `ZREVRANK`, `ZREMRANGEBYRANK`, `ZREMRANGEBYSCORE`, `ZPOPMIN`, `ZPOPMAX` | `ZRANGEBYLEX`, `ZREMRANGEBYLEX`, `ZLEXCOUNT`, `ZRANGESTORE`, `ZMPOP`, `BZPOPMIN`, `BZPOPMAX`, `ZRANDMEMBER`, `ZUNION`, `ZUNIONSTORE`, `ZINTER`, `ZINTERCARD`, `ZINTERSTORE`, `ZDIFF`, `ZDIFFSTORE`, `ZSCAN` |

Score-range queries now cover the leaderboard, rate-limiter, and
delayed-queue patterns. Bounds accept `-inf`/`+inf` and the `(`
exclusive prefix.

Still missing: the set-algebra equivalents (`ZUNIONSTORE` and friends),
the lexicographic range family, `ZADD`'s `NX`/`XX`/`GT`/`LT`/`CH`/`INCR`
flags, and a `WITHSCORES` toggle on `ZRANGE` — it always emits scores,
which is a deviation from Redis rather than an omission.

### 2.7 Variadic inconsistency — **Done**

`DEL`, `HDEL`, `SREM`, and `ZREM` are now variadic alongside `SADD`,
`LPUSH`, `RPUSH`, and `ZADD`. Each kept its original single-argument
reply (`OK`/`NOT_FOUND`) and only switches to `DELETED <n>` when given
more than one, so the existing client libraries keep working unchanged.

`HSET` stays single-pair by design: its value is the rest of the line,
so a variadic form would be ambiguous. `HMSET` is the variadic
spelling.

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
- **No `maxmemory` and no eviction policy.** Redis offers eight
  (`allkeys-lru`, `volatile-ttl`, `allkeys-lfu`, ...). Klyro grows until
  the OS kills it, which makes it unusable as a bounded cache — the most
  common Redis deployment shape of all. `INFO memory` now reports real
  usage from a counting allocator, so the measurement half of this is
  done; the policy half is not.

---

## 5. Performance characteristics that will not scale

These are correctness-adjacent: they work, but degrade badly with size.

| Area | Current | Redis |
|---|---|---|
| Expired-key sweep | `Store::sweep_expired` scans the **entire** keyspace every second | Samples 20 random keys from the volatile set, adaptively |
| `SCAN` | `Store::scan` collects and **sorts every live key on each call** — O(N log N) per call, O(N² log N) for a full iteration | O(1) amortized per call via reverse-binary bucket cursor |
| `KEYS` | Clones every key into a `Vec` before filtering | Streams matches, still O(N) but no full copy |
| Sorted set | `Vec<(String, f64)>` with linear `find_index`; `add`/`rem` are O(N) | Skip list + hash map, O(log N) |
| Event loop | `poll()`, rebuilding the pollfd array each iteration — O(N) per tick in connection count | `epoll`/`kqueue`, O(ready) |
| Threading | Strictly single-threaded | Single-threaded command execution plus optional I/O threads |
| Connections | Unbounded; nothing enforces a limit | `maxclients`, with a graceful rejection |
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
2b. ~~Collection edits did not mark the store dirty.~~ **Fixed**, and it
   was the worst bug found so far: `LPOP`, `HDEL`, `SREM`, `ZREM`,
   `LSET` and `LTRIM` could be lost on a crash because the autosave
   never saw them. See [transactions.md](transactions.md).
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
9. **`HELLO` reports `id: 0` for every connection**, since connections
   are not individually identified. Nothing Klyro implements uses the
   client id.

## 7. Suggested build order

Ordered by (value delivered / effort), not by size.

**Tier 1 — cheap, high value, no architectural change — Done**

All eight items are built; see [command-expansion.md](command-expansion.md).

**Tier 2 — the two that unlock real workloads — Done**

9. ~~`SET` with `NX`/`XX`/`EX`/`PX`/`KEEPTTL`, plus the `SETNX` family.~~
   **Done** — Klyro can hold a lock now.
10. ~~`INFO` and `CONFIG GET`/`SET`, plus a config file.~~ **Done** —
    Klyro can be operated now. See [configuration.md](configuration.md).

**Tier 3 — the protocol rewrite and what it unblocks**

11. ~~RESP2 parser and serializer, replacing the line protocol.~~
    **Done**, with RESP3 as well. See
    [resp-protocol.md](resp-protocol.md). Stock clients work, values are
    binary-safe, and the silent truncation is gone.
12. ~~`MULTI`/`EXEC`/`WATCH`.~~ **Done** — see
    [transactions.md](transactions.md). Per-connection state now lives
    in `session.rs`, and the watched-key registry in `store.rs`.
13. **Pub/sub, then blocking commands** (`BLPOP` and friends). Now the
    top of the list. Both need the event loop to be able to park a
    connection and wake it, which is the one piece of machinery neither
    RESP nor transactions brought with them. RESP3's push type is
    already specified in `resp.rs`'s design, though unimplemented.
14. **Scripting (`EVAL`).** The other way Redis makes several
    operations atomic, and the one transactions do not cover.

**Tier 4 — separable large efforts**

15. `maxmemory` + LRU/LFU eviction. Required for cache use, and the
    measurement half is already done: `INFO memory` reports real usage
    from the counting allocator.
16. AOF with `appendfsync`, plus forked `BGSAVE`.
17. Replication (`REPLICAOF`, `PSYNC`), then Sentinel, then Cluster.
18. `AUTH`/ACL, then TLS.
19. Streams; skip-list sorted set; `epoll`/`kqueue`; bitmaps and HLL.

**Cheap cleanups worth doing along the way:** `SCAN`'s cursor still
sorts the whole keyspace per call (item in section 5), and the expired-key
sweep is still a full scan rather than Redis's sampling.
