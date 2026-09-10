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

- **Text line protocol, not RESP.** Values can't contain `\n`, single
  replies are capped at 64 KiB, no binary-safety — a real client
  library couldn't talk to it, only a raw TCP client (`nc`/telnet).
- No transactions (`MULTI`/`EXEC`).
- No pub/sub (`SUBSCRIBE`/`PUBLISH`).
- No official client library or CLI (`klyro-cli`).

## Data model

- Only 5 basic types — no Streams, Bitmaps, HyperLogLog, Geospatial.
- ~~No command coverage beyond the basics~~ **Done (2026-09-10).** The
  command set went from 39 to 105: the full expiry family, keyspace
  operations (`EXISTS`/`RENAME`/`COPY`/`FLUSHDB`), `SET` option flags,
  the `SETNX`/`SETEX` family, multi-key string access, list random
  access and trimming, the hash ergonomics, the set algebra, and sorted
  set ranks and score ranges. See
  [redis-feature-gap.md](redis-feature-gap.md) for what remains.
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
  2026-09-10).** See [tests/](../tests/) and `cargo test` — 260 tests
  covering every command, WRONGTYPE, multi-value push/add,
  `KEYS`/`SCAN` pattern matching, INFO/CONFIG, config-file loading, and
  a full persistence round-trip.
  Still no CI (nothing runs `cargo test` automatically on push).
- No license file.

## Suggested next steps (roughly smallest/lowest-risk first)

1. ~~**Automated test suite**~~ Done — see above.
2. ~~**String/numeric ergonomics**~~ Done — see above.
3. ~~**`SCAN`/`KEYS pattern`**~~ Done — see above.
4. ~~**Command coverage**~~ Done - see above.
5. ~~**`INFO`/`CONFIG` plus a config file**~~ Done - see above.
6. **Values with embedded spaces in List/Set/Zset** - would need a
   protocol change (e.g. quoting or length-prefixing), which is really
   a stepping stone toward...
7. **A binary-safe protocol (RESP-like)** - the biggest rewrite here;
   touches `server.rs`'s read/parse loop and every command's argument
   parsing. Also the fix for the silently truncated 64 KiB reply and for
   `SET`'s suffix-matched option flags. Worth doing next, now that the
   command set it would carry is broad.
8. **Client library catch-up** - the Python/Node/Go clients still only
   cover the original 39 commands. Best done after the protocol
   settles, so the work isn't paid for twice.
9. **Persistence hardening (AOF)**, **auth**, **replication** - larger,
   separable efforts; not blocking anything else on this list.
