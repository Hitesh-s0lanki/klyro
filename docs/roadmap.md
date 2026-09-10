# Klyro roadmap: what's missing

A gap analysis against a real Redis-like server, as of 2026-09-09. Not
all of this is necessarily worth building — some items (clustering,
replication) are here for completeness, not because a single-node
educational project needs them. See [klyro.md](klyro.md) for the
current architecture and [../README.md](../README.md) for the current
command/feature set. For the detailed command-level inventory - every
missing Redis command grouped by type, plus semantics deviations and a
suggested build order - see [redis-feature-gap.md](redis-feature-gap.md).

## Protocol & transport

- ~~Text line protocol, not RESP~~ **Done (2026-09-10).** Klyro speaks
  RESP2 and RESP3. Values are binary-safe, replies are never silently
  truncated, and redis-py, go-redis, and ioredis all work unmodified.
  See [resp-protocol.md](resp-protocol.md).
- ~~No transactions (`MULTI`/`EXEC`)~~ **Done (2026-09-10).** With
  `WATCH` for optimistic locking, and `RESET`.
- ~~No pub/sub (`SUBSCRIBE`/`PUBLISH`)~~ **Done (2026-09-10).** Channel
  and pattern subscriptions, RESP3 push frames, and the blocking pops
  (`BLPOP` and family) that needed the same connection-parking
  machinery. See [connection-state.md](connection-state.md).
- ~~No official client library or CLI~~ **Moot (2026-09-10).** Any
  Redis client works, `redis-cli` included, so the three hand-written
  clients were retired. See [client-libraries.md](client-libraries.md).

## Data model

- Only 5 basic types — no Streams, Bitmaps, HyperLogLog, Geospatial.
- ~~No command coverage beyond the basics~~ **Done (2026-09-10).** The
  command set went from 39 to 105: the full expiry family, keyspace
  operations (`EXISTS`/`RENAME`/`COPY`/`FLUSHDB`), `SET` option flags,
  the `SETNX`/`SETEX` family, multi-key string access, list random
  access and trimming, the hash ergonomics, the set algebra, and sorted
  set ranks and score ranges. See
  [redis-feature-gap.md](redis-feature-gap.md) for what remains.
- ~~Values must be single tokens~~ **Done (2026-09-10).** RESP
  length-prefixes every argument, so any value may contain spaces,
  newlines, or NUL bytes.
- ~~No key pattern matching~~ **Done (2026-09-09).** `KEYS pattern` and
  `SCAN cursor [MATCH pattern] [COUNT count]` are in - see the README's
  generic command table. No `EXPIRE NX/XX` flags still.
- ~~No numeric/string ergonomics~~ **Done (2026-09-09).** `INCR`/`DECR`/
  `APPEND`/`GETRANGE`/`SETRANGE` are in - see the README's String
  command table.
- Sorted Set is O(n) (sorted array + linear scan), not a skip list -
  fine at moderate scale, not built for large sets.
- No `ZUNIONSTORE`/`ZINTERSTORE`, no lexicographic ranges, and no
  `ZADD` flags.

## Durability & persistence

- Snapshot-only persistence (like Redis's RDB), no append-only log
  (AOF) - a crash or `SIGKILL` loses everything since the last save
  (worst case ~60s of writes on a normal exit).
- No replication - no master/replica, so no failover and no read
  scaling.
- No clustering/sharding - single process, single node, bounded by one
  machine's RAM and one CPU core.

## Security & operations

- No authentication or ACLs - anyone who can reach the port has full
  read/write access.
- No TLS.
- No memory limits or eviction policies (LRU/LFU) - the dataset grows
  until the process runs out of memory. `INFO memory` measures it, but
  nothing acts on the measurement.
- ~~No metrics/observability~~ **Done (2026-09-10).** `INFO` reports
  six sections, including real memory use from a counting allocator and
  a read-command hit ratio. Still no logging beyond startup/shutdown
  lines, and no per-command statistics.
- ~~No config file~~ **Done (2026-09-10).** Nine parameters, settable
  from a file or the command line, seven of them changeable at runtime
  with `CONFIG SET`. See [configuration.md](configuration.md). No
  `CONFIG REWRITE` yet, so a runtime change doesn't survive a restart.

## Concurrency & scale

- Single-threaded - same core design as real Redis, but Redis has
  optional I/O threading; this has none.
- ~~No connection limits~~ **Done (2026-09-10).** `maxclients` turns
  extra connections away with a message; `INFO clients` counts how often.

## Software engineering

- ~~No automated test suite~~ **Done (2026-09-09, extended
  2026-09-10).** See [tests/](../tests/) and `cargo test` — 461 tests
  covering every command, WRONGTYPE, multi-value push/add,
  `KEYS`/`SCAN` pattern matching, INFO/CONFIG, config-file loading,
  RESP framing, transactions, pub/sub, the blocking pops, and a full
  persistence round-trip.
- ~~No CI~~ **Done (2026-09-10).** `.github/workflows/ci.yml` runs
  `cargo fmt --check`, `cargo clippy -D warnings`, the test suite, and
  a release build on every push and pull request, plus the site's
  typecheck and build. The Docker workflow still only runs on a merge
  to main.
- ~~No license file~~ **Done (2026-09-10).** MIT.

## Beyond Redis compatibility

Klyro's memory structures for AI agents - Search, Vector, and Hybrid
retrieval as a sixth native data type behind a `MEM.*` command family -
are **built** as of 2026-09-10: 15 commands, BM25 keyword ranking,
brute-force vector search over three metrics, weighted and rank-based
fusion, metadata filters, per-record TTL, and dump format 3. See
[memory-structures.md](memory-structures.md) for the design and what
remains (an embedding provider, an approximate vector index, SDKs, and
a REST gateway). That work is independent of everything above and does
not block, or wait on, any of it.

## Suggested next steps (roughly smallest/lowest-risk first)

1. ~~**Automated test suite**~~ Done — see above.
2. ~~**String/numeric ergonomics**~~ Done — see above.
3. ~~**`SCAN`/`KEYS pattern`**~~ Done — see above.
4. ~~**Command coverage**~~ Done - see above.
5. ~~**`INFO`/`CONFIG` plus a config file**~~ Done - see above.
6. ~~**Values with embedded spaces in List/Set/Zset**~~ Done - see
   above.
7. ~~**A binary-safe protocol (RESP-like)**~~ Done - see above.
8. ~~**Transactions (`MULTI`/`EXEC`/`WATCH`)**~~ Done - see above.
9. ~~**Pub/sub, then blocking commands**~~ Done - see above. All three
   landed together, because all three needed the same thing: per-
   connection state, and an event loop that can park a connection.
10. **`maxmemory` with an eviction policy** - now the top of the list,
    and what stands between Klyro and use as a bounded cache. `INFO`
    already reports real memory use from a counting allocator, so the
    measurement half is done.
11. **Persistence hardening (AOF)**, **auth**, **replication** - larger,
    separable efforts; not blocking anything else on this list.
