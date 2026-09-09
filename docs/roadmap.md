# Klyro roadmap: what's missing

A gap analysis against a real Redis-like server, as of 2026-09-09. Not
all of this is necessarily worth building — some items (clustering,
replication) are here for completeness, not because a single-node
educational project needs them. See [klyro.md](klyro.md) for the
current architecture and [../README.md](../README.md) for the current
command/feature set.

## Protocol & transport

- **Text line protocol, not RESP.** Values can't contain `\n`, single
  replies are capped at 64 KiB, no binary-safety — a real client
  library couldn't talk to it, only a raw TCP client (`nc`/telnet).
- No transactions (`MULTI`/`EXEC`).
- No pub/sub (`SUBSCRIBE`/`PUBLISH`).
- No official client library or CLI (`klyro-cli`).

## Data model

- Only 5 basic types — no Streams, Bitmaps, HyperLogLog, Geospatial.
- `LPUSH`/`RPUSH`/`SADD`/`ZADD` values must be single tokens (no
  embedded spaces) so multiple values per call stay unambiguous - a
  value with spaces has to go through `SET`/`HSET` instead.
- ~~No key pattern matching~~ **Done (2026-09-09).** `KEYS pattern` and
  `SCAN cursor [MATCH pattern] [COUNT count]` are in - see the README's
  generic command table. No `EXPIRE NX/XX` flags still.
- ~~No numeric/string ergonomics~~ **Done (2026-09-09).** `INCR`/`DECR`/
  `APPEND`/`GETRANGE`/`SETRANGE` are in - see the README's String
  command table.
- Sorted Set is O(n) (sorted array + linear scan), not a skip list -
  fine at moderate scale, not built for large sets.

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
  until the process runs out of memory.
- No metrics/observability - no `INFO` command, no stats (hit rate,
  ops/sec, memory usage), no logging beyond startup/shutdown lines.
- No config file - only two CLI args (port, dump path); everything else
  (autosave interval, max connections, ...) is hardcoded.

## Concurrency & scale

- Single-threaded - same core design as real Redis, but Redis has
  optional I/O threading; this has none.
- No connection limits - nothing stops a client from opening many
  connections.

## Software engineering

- ~~No automated test suite~~ **Done (2026-09-09).** See
  [tests/](../tests/) and `make test` — 52 integration tests covering
  every command, WRONGTYPE, multi-value push/add, and a full
  persistence round-trip. Still no CI (nothing runs `make test`
  automatically on push).
- No license file.

## Suggested next steps (roughly smallest/lowest-risk first)

1. ~~**Automated test suite**~~ Done — see above.
2. ~~**String/numeric ergonomics**~~ Done — see above.
3. ~~**`SCAN`/`KEYS pattern`**~~ Done — see above.
4. **Values with embedded spaces in List/Set/Zset** - would need a
   protocol change (e.g. quoting or length-prefixing), which is really
   a stepping stone toward...
5. **A binary-safe protocol (RESP-like)** - the biggest rewrite here;
   touches `server.c`'s read/parse loop and every command's argument
   parsing. Worth doing once the value-added by items 2-3 is in place.
6. **Persistence hardening (AOF)**, **auth**, **replication** - larger,
   separable efforts; not blocking anything else on this list.
