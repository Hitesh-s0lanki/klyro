# Klyro vs Redis: feature gap analysis

Command-level and subsystem-level comparison of Klyro against Redis 7.x,
as of 2026-09-10. Complements [roadmap.md](roadmap.md), which covers the
same ground at the architecture level. This document is the detailed
inventory: what exists, what is missing, and which gaps actually block
real workloads.

**Status, 2026-09-10:** Tier 1 and all of Tier 2 are now built.
Klyro implements **107 commands**, up from 39. Redis implements roughly
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

One deviation to know about. The value is still the rest of the line, so
there are no argument boundaries to separate a trailing flag from the
value. The parser matches the trailing words against the option grammar
and takes the longest suffix that parses as a complete option list,
always leaving at least one token as the value. Redis's argument order
therefore works, but a value whose last words spell valid options
(`SET k done XX`) loses them. The RESP rewrite in 1.2 removes the
ambiguity for good; until then `SETEX`/`SETNX` are the unambiguous
spellings.

The `GET` flag on `SET` is still missing.

### 1.2 No RESP protocol — no real client compatibility

The wire format is a bespoke text-line protocol. Consequences:

- No off-the-shelf client (`redis-py`, `ioredis`, `go-redis`, `Jedis`,
  `Lettuce`) can connect. The three hand-written clients in
  [clients/](../clients/) exist only because of this.
- Values are not binary-safe. A value containing `\n` corrupts the
  stream; a List/Set/Zset member containing a space is unparseable,
  because [strutil.rs](../src/util/strutil.rs) splits on single spaces
  with no quoting or length prefix.
- Replies are truncated, silently. `Conn::reply` in
  [server.rs](../src/server.rs) caps the write buffer at 64 KiB and
  drops the overflow with no error. A `SMEMBERS` or `KEYS` over a large
  keyspace returns a partial answer that looks complete. This is the
  most dangerous item on the list, because it is the only one that
  returns wrong data rather than an error.
- No RESP3, so no push messages, no client-side caching / tracking, no
  attribute or map reply types.

### 1.3 No transactions

`MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH` are all absent. There is
no way to group commands atomically and no optimistic-locking primitive.

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

1. **Silent reply truncation at 64 KiB** — see 1.2. A partial reply is
   indistinguishable from a complete one. Still open, and still the most
   dangerous item in this document.
2. ~~`DBSIZE` counts expired-but-unswept keys.~~ **Fixed.** `Store::size`
   now filters on liveness, so `DBSIZE` agrees with `KEYS`.
3. **`SET`'s option flags are matched as a trailing suffix** rather than
   at fixed argument positions, because the value is the rest of the
   line. Redis's argument order works; a value whose last words spell
   valid options does not. See 1.1.
4. **`SETRANGE` pads with spaces.** Redis pads with null bytes. Padding
   with `0x20` produces a value that differs from Redis for the same
   input.
5. **`ZADD` caps at 128 score/member pairs** (`MAX_ZADD_PAIRS`); Redis
   has no such limit.
6. **String length capped at 64 KiB** (`MAX_STRING_LEN`); Redis allows
   512 MB.
7. **Expiry uses `SystemTime`**, so a wall-clock adjustment shifts every
   TTL. Redis uses a monotonic-corrected clock.
8. **Scores display at 6 significant digits** but persist at 17, so a
   score read back through `ZSCORE` may be rounded relative to what the
   dump file holds.
9. **`TTL` returns `-2`/`-1` as a `TTL <n>` line**, not an integer reply;
   this is a protocol-shape difference, harmless in isolation.
10. **`SPOP`/`SRANDMEMBER`/`RANDOMKEY` use a xorshift PRNG** seeded from
    the wall clock. Fine for spreading picks around, not suitable
    anywhere randomness needs to be unpredictable.

---

## 7. Suggested build order

Ordered by (value delivered ÷ effort), not by size.

**Tier 1 — cheap, high value, no architectural change — Done**

All eight items are built: `EXISTS`; variadic `DEL`/`HDEL`/`SREM`/
`ZREM`; `STRLEN`/`INCRBY`/`DECRBY`/`INCRBYFLOAT`/`MGET`/`MSET`;
`HEXISTS`/`HKEYS`/`HVALS`/`HMGET`/`HMSET`/`HINCRBY`/`HINCRBYFLOAT`/
`HSETNX`/`HSTRLEN`; `PERSIST`/`PTTL`/`PEXPIRE`/`EXPIREAT`/`PEXPIREAT`/
`RENAME`/`RENAMENX`/`COPY`/`FLUSHDB`/`FLUSHALL`/`RANDOMKEY`/`UNLINK`;
`LINDEX`/`LSET`/`LREM`/`LTRIM`/`LINSERT`/`LPUSHX`/`RPUSHX`/`RPOPLPUSH`/
`LMOVE` and `LPOP`/`RPOP` counts; the set algebra plus `SPOP`/
`SRANDMEMBER`/`SMOVE`/`SMISMEMBER`; and `ZINCRBY`/`ZRANK`/`ZREVRANK`/
`ZREVRANGE`/`ZRANGEBYSCORE`/`ZREVRANGEBYSCORE`/`ZCOUNT`/`ZMSCORE`/
`ZPOPMIN`/`ZPOPMAX`/`ZREMRANGEBYRANK`/`ZREMRANGEBYSCORE`.

**Tier 2 — the two that unlock real workloads — Done**

9. ~~`SET` with `NX`/`XX`/`EX`/`PX`/`KEEPTTL`, plus `SETNX`/`SETEX`/
   `GETSET`/`GETDEL`.~~ **Done** — Klyro can hold a lock now.
10. ~~`INFO` and `CONFIG GET`/`SET`, plus a config file.~~ **Done** —
    Klyro can be operated now. See [configuration.md](configuration.md).

**Tier 3 — the protocol rewrite (everything downstream depends on it).
This is now the top of the list.**

11. RESP2 parser and serializer in `server.rs`, replacing the line
    protocol. Fixes binary safety, the space-in-value restriction, the
    silent truncation, and `SET`'s suffix-matched flags, and makes every
    stock Redis client work. Retire the three hand-written clients in
    `clients/` afterward.
12. `MULTI`/`EXEC`/`WATCH` on top of RESP.
13. Pub/sub, then blocking commands (`BLPOP` and friends), both of which
    need the connection-state machinery RESP forces you to build anyway.

**Tier 4 — separable large efforts**

14. `maxmemory` + LRU/LFU eviction. Required for cache use.
15. AOF with `appendfsync`, plus forked `BGSAVE`.
16. Replication (`REPLICAOF`, `PSYNC`), then Sentinel, then Cluster.
17. `AUTH`/ACL, then TLS.
18. Streams; skip-list sorted set; `epoll`/`kqueue`; bitmaps and HLL.

**Not on the ladder, but owed:** the three client libraries in
[clients/](../clients/) still cover only the original 39 commands. They
keep working — every variadic extension preserved its single-argument
reply — but none of the 66 new commands is reachable from Python, Node,
or Go yet.
