# Klyro

**The high-performance in-memory data server.**

An in-memory, Redis-style data server in Rust, with String, List, Hash,
Set, and Sorted Set data types. Started from
https://github.com/rairai77/cache22 (a bare C skeleton, following
https://www.youtube.com/watch?v=FFxEoQyNQKM), grown into a full C
implementation, then migrated to Rust module-by-module (see
[docs/rust-migration.md](docs/rust-migration.md)) - the wire protocol
and on-disk dump format are unchanged throughout.

## Build

```sh
cargo build --release
```

## Test

```sh
cargo test
```

Runs the unit tests embedded in `src/` (parsing, glob matching, the
store, persistence round-trips) plus the integration suite under
[tests/](tests/): spawns real `klyro` server subprocesses, talks to
them over the actual TCP protocol, and checks every command, WRONGTYPE
errors, multi-value push/add, `KEYS`/`SCAN` pattern matching, and a
full persistence round-trip (save, kill, reload). 260 tests in all.
`cargo test` builds first, so a plain `cargo test` from a clean
checkout is enough.

## Run

```sh
cargo run --release -- [port] [dump-file]   # defaults: port 7171, dump-file klyro.dump
# or, after `cargo build --release`:
./target/release/klyro [port] [dump-file]
./target/release/klyro klyro.conf           # or a config file
./target/release/klyro --config klyro.conf 7200
```

Settings come from a config file, the command line, or both - command
line arguments are applied last, so they win. Copy
[klyro.conf.sample](klyro.conf.sample) to get started; it documents every
parameter at its default value. `CONFIG GET`/`CONFIG SET` read and change
the same settings on a running server, and `INFO` reports what the server
is doing. See [docs/configuration.md](docs/configuration.md).

On startup, Klyro loads `dump-file` if it exists. Data is saved back to
it on graceful shutdown (`SHUTDOWN` command, or `SIGINT`/`SIGTERM`), on
an explicit `SAVE` command, and automatically every 60s if anything
changed. Killing the process (`SIGKILL`, a crash, or power loss) loses
any changes since the last save.

## Run with Docker

```sh
docker build -t klyro .
docker run -d --name klyro -p 7171:7171 -v klyro-data:/data klyro
```

Or with Compose, which sets up the port and the volume for you:

```sh
docker compose up -d
```

The image is a two-stage build - a static musl binary from
`rust:1-alpine`, copied onto a bare Alpine runtime - so it comes out
around 15 MB. It runs as an unprivileged user (uid 10001), keeps the
dump file in the `/data` volume, and carries a healthcheck that
`PING`s the server over the real protocol.

`docker stop` sends `SIGTERM`, which Klyro handles by saving the dump
before exiting, so data survives a restart. Compose allows 30s for
that; raise `stop_grace_period` if a large keyspace needs longer.

Three environment variables configure the container:

| Variable | Default | Meaning |
| --- | --- | --- |
| `KLYRO_PORT` | `7171` | Port to listen on, inside the container |
| `KLYRO_DUMP` | `/data/klyro.dump` | Dump file path |
| `KLYRO_CONFIG` | unset | Config file to load, e.g. a mounted copy of `klyro.conf.sample` |

```sh
# a config file from the host, on a different port
docker run -d -p 6380:6380 \
    -e KLYRO_PORT=6380 \
    -e KLYRO_CONFIG=/etc/klyro/klyro.conf \
    -v $PWD/klyro.conf:/etc/klyro/klyro.conf:ro \
    -v klyro-data:/data \
    klyro
```

`KLYRO_PORT` is passed on the command line, so it overrides a `port`
line in `KLYRO_CONFIG`. Arguments given to `docker run` after the image
name bypass all three and go straight to the binary. See
[docs/docker.md](docs/docker.md) for the decisions behind the image.

## Talk to it

Any line-oriented TCP client works, e.g. `nc`:

```sh
nc localhost 7171
SET foo bar
GET foo
LPUSH mylist a
LRANGE mylist 0 -1
HSET user name Alice
HGETALL user
SADD tags fast
ZADD board 100 alice
ZRANGE board 0 -1
QUIT
```

## Client libraries

Official clients, each a small dependency-free wrapper around the
protocol below (typed methods, one per command, tested against the
real server) - see [docs/client-libraries.md](docs/client-libraries.md)
for the design decisions behind them:

- [clients/python/](clients/python/) - sync, stdlib `socket` only
- [clients/node/](clients/node/) - async/Promise, TypeScript, zero runtime deps
- [clients/go/](clients/go/) - stdlib `net` only

## Commands

107 commands. Every reply is a single CRLF-terminated line, except the
multi-line replies marked below, which end with a lone `END` line.

### Generic (any type)

| Command | Reply |
|---|---|
| `PING` | `PONG` |
| `ECHO message` | `VALUE <message>` |
| `DEL key [key ...]` | `OK`/`NOT_FOUND` for one key, `DELETED <n>` for several |
| `UNLINK key [key ...]` | same as `DEL` |
| `EXISTS key [key ...]` | `COUNT <n>` (a repeated key counts once per repetition) |
| `EXPIRE key seconds` | `OK` or `NOT_FOUND` |
| `PEXPIRE key milliseconds` | `OK` or `NOT_FOUND` |
| `EXPIREAT key unix-time-seconds` | `OK` or `NOT_FOUND` |
| `PEXPIREAT key unix-time-milliseconds` | `OK` or `NOT_FOUND` |
| `PERSIST key` | `OK` if a TTL was removed, else `NOT_FOUND` |
| `TTL key` | `TTL <seconds>` (`-1` = no expiry, `-2` = missing) |
| `PTTL key` | `PTTL <milliseconds>` (same `-1`/`-2` convention) |
| `TYPE key` | `STRING`/`LIST`/`HASH`/`SET`/`ZSET`, or `NONE` if missing |
| `RENAME key newkey` | `OK`, or `NOT_FOUND` if the source is missing |
| `RENAMENX key newkey` | `OK`, `FALSE` if the destination exists, `NOT_FOUND` if the source is missing |
| `COPY source destination [REPLACE]` | `OK`, `FALSE` if the destination exists, `NOT_FOUND` if the source is missing |
| `RANDOMKEY` | `VALUE <key>` or `NOT_FOUND` |
| `KEYS [pattern]` | one matching key per line, then `END` (no pattern = every key) |
| `SCAN cursor [MATCH pattern] [COUNT count]` | matching keys, then a final `CURSOR <n>` line |
| `DBSIZE` | `COUNT <n>` (live keys only) |
| `FLUSHDB` / `FLUSHALL` | `OK` (one keyspace, so both do the same thing) |
| `INFO [section]` | `key:value` lines under `# Section` headers, then `END` |
| `CONFIG GET pattern` | one `parameter value` line per match, then `END` |
| `CONFIG SET parameter value` | `OK` or an `ERR` explaining the refusal |
| `CONFIG RESETSTAT` | `OK` (clears INFO's activity counters) |
| `SAVE` | `OK` (writes the dump file immediately) |
| `QUIT` | `BYE`, then closes the connection |
| `SHUTDOWN` | `SHUTTING_DOWN`, then stops the server (saving first) |

`COPY` is a deep copy: mutating the destination afterwards leaves the
source untouched. `RENAME` and `COPY` both carry the TTL across.

### Strings

| Command | Reply |
|---|---|
| `SET key value [NX\|XX] [EX seconds\|PX milliseconds\|KEEPTTL]` | `OK`, or `NOT_SET` when `NX`/`XX` isn't satisfied |
| `SETNX key value` | `OK` or `NOT_SET` |
| `SETEX key seconds value` / `PSETEX key milliseconds value` | `OK` |
| `GET key` | `VALUE <value>` or `NOT_FOUND` |
| `GETSET key value` | `VALUE <previous>` or `NOT_FOUND`; clears any TTL |
| `GETDEL key` | `VALUE <value>` or `NOT_FOUND`; removes the key |
| `GETEX key [EX seconds\|PX milliseconds\|PERSIST]` | `VALUE <value>` or `NOT_FOUND`; adjusts the TTL |
| `MGET key [key ...]` | one `VALUE <v>`/`NOT_FOUND` line per key, then `END` |
| `MSET key value [key value ...]` | `OK` (values are single tokens here) |
| `INCR key` / `DECR key` | `VALUE <n>` |
| `INCRBY key increment` / `DECRBY key increment` | `VALUE <n>` |
| `INCRBYFLOAT key increment` | `VALUE <n>` |
| `APPEND key value` | `LEN <n>` (new total length) |
| `STRLEN key` | `LEN <n>` (`0` if missing) |
| `GETRANGE key start end` | `VALUE <substring>` (inclusive; negative indices count from the end) |
| `SETRANGE key offset value` | `LEN <n>` (pads any gap before `offset` with spaces) |

`SET key value NX EX 30` is the atomic lock primitive: it takes the key
only if nobody holds it, and the lease expires on its own.

The whole rest of the line is the value, so values may contain spaces.
To keep Redis's argument order anyway, the trailing words are matched
against the option grammar and the longest suffix that parses as a
complete option list becomes the flags. One token always stays behind as
the value, so `SET k NX` still stores the literal string `NX`. The cost
is that a value whose last words spell valid options (`SET k done XX`)
loses them to the parser — use `SETEX`/`SETNX` for those.

`INCR`/`DECR`/`INCRBY`/`APPEND`/`SETRANGE` keep an existing TTL; `SET`
(without `KEEPTTL`), `GETSET`, and `MSET` clear it.

### Lists

| Command | Reply |
|---|---|
| `LPUSH key value [value ...]` / `RPUSH key value [value ...]` | `LEN <n>` |
| `LPUSHX key value [value ...]` / `RPUSHX key value [value ...]` | `LEN <n>`, or `NOT_FOUND` if the key doesn't exist |
| `LPOP key [count]` / `RPOP key [count]` | `VALUE <value>`/`NOT_FOUND` without a count; one value per line then `END` with one |
| `LLEN key` | `LEN <n>` |
| `LRANGE key start stop` | one value per line, then `END` |
| `LINDEX key index` | `VALUE <value>` or `NOT_FOUND` |
| `LSET key index value` | `OK`, or `NOT_FOUND` if the key or index is out of range |
| `LINSERT key BEFORE\|AFTER pivot value` | `LEN <n>`, or `NOT_FOUND` if the pivot is absent |
| `LREM key count value` | `REMOVED <n>` (`count > 0` from the head, `< 0` from the tail, `0` all) |
| `LTRIM key start stop` | `OK` (an empty range deletes the key) |
| `RPOPLPUSH source destination` | `VALUE <value>` or `NOT_FOUND` |
| `LMOVE source destination LEFT\|RIGHT LEFT\|RIGHT` | `VALUE <value>` or `NOT_FOUND` |

`RPOPLPUSH`/`LMOVE` may name the same list twice, which rotates it.

### Hashes

| Command | Reply |
|---|---|
| `HSET key field value` | `OK` (value is the rest of the line) |
| `HSETNX key field value` | `OK`, or `FALSE` if the field already exists |
| `HMSET key field value [field value ...]` | `OK` (values are single tokens here) |
| `HGET key field` | `VALUE <value>` or `NOT_FOUND` |
| `HMGET key field [field ...]` | one `VALUE <v>`/`NOT_FOUND` line per field, then `END` |
| `HDEL key field [field ...]` | `OK`/`NOT_FOUND` for one field, `DELETED <n>` for several |
| `HLEN key` | `LEN <n>` |
| `HEXISTS key field` | `TRUE` or `FALSE` |
| `HSTRLEN key field` | `LEN <n>` (`0` if missing) |
| `HKEYS key` / `HVALS key` | one field (or value) per line, then `END` |
| `HGETALL key` | alternating `field`/`value` lines, then `END` |
| `HINCRBY key field increment` | `VALUE <n>` |
| `HINCRBYFLOAT key field increment` | `VALUE <n>` |

### Sets

| Command | Reply |
|---|---|
| `SADD key member [member ...]` | `ADDED <n>` (newly added members only) |
| `SREM key member [member ...]` | `OK`/`NOT_FOUND` for one member, `DELETED <n>` for several |
| `SISMEMBER key member` | `TRUE` or `FALSE` |
| `SMISMEMBER key member [member ...]` | one `TRUE`/`FALSE` line per member, then `END` |
| `SCARD key` | `LEN <n>` |
| `SMEMBERS key` | one member per line, then `END` |
| `SPOP key [count]` | removes and returns members: `VALUE <m>`/`NOT_FOUND` without a count, a list with one |
| `SRANDMEMBER key [count]` | same shape, without removing; a negative count may repeat members |
| `SMOVE source destination member` | `OK` or `NOT_FOUND` |
| `SINTER key [key ...]` / `SUNION ...` / `SDIFF ...` | one member per line, then `END` |
| `SINTERSTORE destination key [key ...]` / `SUNIONSTORE ...` / `SDIFFSTORE ...` | `LEN <n>` |

A missing key counts as an empty set. `SDIFF` subtracts every later set
from the first, so it is not symmetric. A `STORE` variant whose result
is empty deletes the destination.

### Sorted sets

| Command | Reply |
|---|---|
| `ZADD key score member [score member ...]` | `ADDED <n>` (repositioning an existing member doesn't count) |
| `ZSCORE key member` | `VALUE <score>` or `NOT_FOUND` |
| `ZMSCORE key member [member ...]` | one `VALUE <score>`/`NOT_FOUND` line per member, then `END` |
| `ZINCRBY key increment member` | `VALUE <score>` (a missing member starts at 0) |
| `ZREM key member [member ...]` | `OK`/`NOT_FOUND` for one member, `DELETED <n>` for several |
| `ZCARD key` | `LEN <n>` |
| `ZRANK key member` / `ZREVRANK key member` | `RANK <n>` or `NOT_FOUND` |
| `ZRANGE key start stop` | `member score` per line, ascending, then `END` |
| `ZREVRANGE key start stop` | the same, descending |
| `ZRANGEBYSCORE key min max` | `member score` per line, then `END` |
| `ZREVRANGEBYSCORE key max min` | the same, descending (bounds high-first) |
| `ZCOUNT key min max` | `COUNT <n>` |
| `ZREMRANGEBYRANK key start stop` | `REMOVED <n>` |
| `ZREMRANGEBYSCORE key min max` | `REMOVED <n>` |
| `ZPOPMIN key [count]` / `ZPOPMAX key [count]` | `member score` per line, then `END` |

Score bounds accept a plain number, `-inf`/`+inf`, or a `(` prefix for
an exclusive bound (`ZCOUNT board (75 +inf`).

`ZADD` takes up to 128 score/member pairs per call; more than that (or a
score with no matching member) replies with an `ERR`.

### Rules that apply everywhere

A command against a key holding a different type replies
`ERR WRONGTYPE ...` (e.g. `LPUSH` on a key created by `SET`). Popping or
removing the last element of a collection deletes the key, same as Redis.

Values in `LPUSH`/`RPUSH`/`SADD`/`MSET`/`HMSET` and members in `ZADD`
are single whitespace-delimited tokens (no embedded spaces) — that's
what makes multiple values per call unambiguous. `SET`/`HSET`/`LSET`/
`LINSERT`/`LREM` values are the rest of the line and may contain spaces,
since those commands take exactly one value.

Commands that grew a variadic form kept their original single-argument
reply, so existing clients are unaffected: `DEL`/`HDEL`/`SREM`/`ZREM`
still answer `OK`/`NOT_FOUND` when given exactly one key, field, or
member, and only switch to `DELETED <n>` when given several.

## Project layout

```
Cargo.toml       - binary crate `klyro`; only dependency is `libc` (for poll())

src/
  main.rs        - entry point: wires everything together
  server.rs      - TCP networking + poll()-based event loop, connection I/O
  config.rs      - the tunables, the config-file parser, CONFIG's get/set surface
  stats.rs       - the counters INFO reports
  commands/      - command parsing and dispatch, grouped by data type
    mod.rs       -   the router plus reply helpers shared by the handlers
    generic.rs   -   any-type keys: DEL/EXISTS/EXPIRE/RENAME/COPY/SCAN/...
    string.rs    -   SET (with its option flags), the SETNX family, INCR*
    list.rs      -   push/pop, LINDEX/LSET/LINSERT/LREM/LTRIM, LMOVE
    hash.rs      -   HSET/HMSET/HMGET/HINCRBY/HKEYS/...
    set.rs       -   membership plus the SINTER/SUNION/SDIFF algebra
    zset.rs      -   ranks, score-range queries, ZINCRBY, the pops
    server.rs    -   PING/ECHO/INFO/CONFIG/SAVE/QUIT/SHUTDOWN
  store.rs       - the keyspace: maps keys to typed values, with expiry
  persist.rs     - save/load the whole keyspace to a dump file
  app.rs         - bundles Store + Persist + the running flag shared by the above
  types/         - the data type implementations
    list.rs      -   List (a VecDeque<String> alias + Redis-style range())
    hash.rs      -   Hash (a HashMap<String, String> alias)
    set.rs       -   Set (a HashSet<String> alias)
    zset.rs      -   Sorted Set (members ordered by (score, member))
  util/          - generic infrastructure with no keyspace/protocol knowledge
    strutil.rs   -   shared line-parsing helpers (used by commands + persist)
    glob.rs      -   glob pattern matching (used by KEYS/SCAN)
    rand.rs      -   xorshift PRNG (used by SPOP/SRANDMEMBER/RANDOMKEY)
    memory.rs    -   counting global allocator (used by INFO memory)

Dockerfile            - two-stage image build (static musl binary -> Alpine)
docker-compose.yml    - one service, published port, named volume for /data
docker-entrypoint.sh  - turns KLYRO_* env vars into Klyro's arguments
docker-healthcheck.sh - PINGs the server on whatever port it bound

tests/           - the integration suite (see "Test" above)
  common/mod.rs  -  starts/stops a klyro subprocess, speaks its protocol
  generic.rs     -  PING/DEL/EXPIRE/TTL/TYPE/KEYS/DBSIZE/SAVE/SHUTDOWN
  types.rs       -  String/List/Hash/Set/Zset ops, WRONGTYPE, empty-delete
  multi.rs       -  multi-value LPUSH/RPUSH/SADD/ZADD
  scan.rs        -  KEYS glob patterns, SCAN's resumable cursor
  persistence.rs -  save/kill/reload round-trip, TTL across a restart
  keyspace.rs    -  EXISTS/RENAME/COPY/RANDOMKEY/FLUSHDB, variadic DEL
  expiry.rs      -  PEXPIRE/EXPIREAT/PTTL/PERSIST/GETEX
  strings.rs     -  SET option flags, SETNX/SETEX/GETSET/MGET/INCRBY
  lists.rs       -  LINDEX/LSET/LINSERT/LREM/LTRIM/LMOVE, LPOP counts
  hashes.rs      -  HMSET/HMGET/HSETNX/HEXISTS/HKEYS/HINCRBY
  sets.rs        -  SINTER/SUNION/SDIFF and STORE forms, SPOP, SMOVE
  sortedsets.rs  -  ranks, score ranges, ZINCRBY, ZPOPMIN/MAX, ZREMRANGE*
  admin.rs       -  INFO sections and counters, CONFIG GET/SET, config files

clients/         - official client libraries (see "Client libraries" above)
  python/        - sync, stdlib `socket` only
  node/          - async/Promise, TypeScript, zero runtime deps
  go/            - stdlib `net` only
```

Each concern lives in its own module so new features can be added as new
files without disturbing the others:

- A new tunable → add a field, a `get` arm, and a `set` arm in
  `config.rs`, list its name in `PARAMETERS`, and read it where the
  hardcoded value used to be. `CONFIG GET`/`SET` pick it up with no
  further work.
- A new command → add a `match` arm in the `src/commands/` module for
  its data type, and list its name in the router in `commands/mod.rs`
  (plus a helper on `Store` in `store.rs` if it needs new storage
  behavior).
- A new data type → add `src/types/<type>.rs` with its own storage +
  ops, add a `StoreType` variant + `get_or_create_*`/`get_existing_*`
  accessors in `store.rs` (the `define_collection_accessors!` macro
  covers most of the boilerplate), add `src/commands/<type>.rs` and
  route to it from `commands/mod.rs`, and add a case to `persist.rs`'s
  dump/load so it survives a restart.
- Pub/sub, replication, etc. → new modules alongside `server.rs`/
  `store.rs`, hooked in from `main.rs`.

Single-threaded by design, same as the original: `server.rs` runs one
`poll()`-based event loop (via the `libc` crate, the closest match to
the original C version's model) and owns the `Store` outright, so
nothing needs `Arc`/`Mutex` - commands run to completion serialized
through that one loop.

Unlike the old Python test suite (which shared one server per test
class to cut process-spawn overhead), each Rust `#[test]` spawns its
own dedicated `klyro` subprocess: it starts in milliseconds, and
per-test isolation means `Drop` alone guarantees cleanup even when a
test panics, with no shared state or key-namespacing needed between
tests.

## Persistence format

The dump file is a simple text format (see `persist.rs`), byte-compatible
with the original C implementation: a `KLYRO-DUMP 1` header line, then
one record per key - `STRING key value`, or `LIST|HASH|SET|ZSET key
count` followed by `count` data lines - plus an optional `EXPIREAT key
unix-timestamp` line after a key's record if it has a TTL. Saves are
atomic (written to `<path>.tmp`, then renamed over the real path), so a
crash mid-save can't corrupt the existing dump.

## Known limitations

See [docs/redis-feature-gap.md](docs/redis-feature-gap.md) for the full
comparison against Redis. The ones worth knowing before you use this:

- Values can't contain `\n`, and a single reply is capped at 64 KiB —
  a reply past that cap is **silently truncated**, not reported.
- No transactions (`MULTI`/`EXEC`), pub/sub, scripting, or blocking
  commands (`BLPOP`), so no queues and no server-side atomic
  read-modify-write beyond what a single command does.
- Not RESP, so stock Redis clients can't connect — use the libraries in
  [clients/](clients/).
- No `maxmemory` or eviction policy: the dataset grows until the process
  runs out of memory. `INFO memory` reports how much is in use, but
  nothing acts on it.
- No authentication, ACLs, or TLS; do not expose this on an untrusted
  network.
- Sorted Set range/lookup ops are O(n) (a sorted array, not a skip list) —
  fine at moderate scale, not built for huge sets.
- Persistence is a full-keyspace snapshot (like Redis's RDB), not an
  append-only log — a `SIGKILL`/crash loses everything since the last
  save (on a normal exit, at most ~60s of changes).
- `SETRANGE` pads gaps with ASCII spaces, not zero bytes like Redis —
  kept for compatibility with the original C implementation's dump
  format and observed behavior, though Rust's `String` has no trouble
  representing an embedded `\0` byte.
- `INCR`/`DECR`/`APPEND`/`SETRANGE` cap the resulting string at 64 KiB
  and reply with an `ERR` past that, to keep a single `APPEND`/`SETRANGE`
  loop from growing a value without bound.
