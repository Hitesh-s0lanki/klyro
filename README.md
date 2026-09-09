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
full persistence round-trip (save, kill, reload). `cargo test` builds
first, so a plain `cargo test` from a clean checkout is enough.

## Run

```sh
cargo run --release -- [port] [dump-file]   # defaults: port 7171, dump-file klyro.dump
# or, after `cargo build --release`:
./target/release/klyro [port] [dump-file]
```

On startup, Klyro loads `dump-file` if it exists. Data is saved back to
it on graceful shutdown (`SHUTDOWN` command, or `SIGINT`/`SIGTERM`), on
an explicit `SAVE` command, and automatically every 60s if anything
changed. Killing the process (`SIGKILL`, a crash, or power loss) loses
any changes since the last save.

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

Generic (any type):

| Command | Reply |
|---|---|
| `PING` | `PONG` |
| `DEL key` | `OK` or `NOT_FOUND` |
| `EXPIRE key seconds` | `OK` or `NOT_FOUND` |
| `TTL key` | `TTL <seconds>` (`-1` = no expiry, `-2` = missing) |
| `TYPE key` | `STRING`/`LIST`/`HASH`/`SET`/`ZSET`, or `NONE` if missing |
| `KEYS [pattern]` | one matching key per line, terminated by `END` (no pattern = every key) |
| `SCAN cursor [MATCH pattern] [COUNT count]` | matching keys, then a final `CURSOR <n>` line |
| `DBSIZE` | `COUNT <n>` |
| `SAVE` | `OK` (writes the dump file immediately) |
| `QUIT` | `BYE`, then closes the connection |
| `SHUTDOWN` | `SHUTTING_DOWN`, then stops the server (saving first) |

`KEYS`/`SCAN`'s `pattern` is a glob: `*` matches any run of characters,
`?` matches exactly one, `[abc]`/`[a-z]`/`[^abc]` match a character
class. `SCAN` starts with cursor `0`; keep passing back the `CURSOR`
value from each reply until it comes back `0` again, which means the
whole keyspace has been covered (matching Redis's own convention).
`COUNT` (default 10) is how many keys each call examines; `MATCH` is
applied after that, so a call can return fewer than `COUNT` keys (even
none) while the cursor is still non-zero - keep iterating until it is
`0`. The cursor is a position in a sorted snapshot of the live
keyspace, taken fresh on each call, so - unlike Redis - inserts or
deletes between two `SCAN` calls can cause a key to be skipped or
repeated; fine for interactive/dev use.

String:

| Command | Reply |
|---|---|
| `SET key value` | `OK` (value is the rest of the line — may contain spaces) |
| `GET key` | `VALUE <value>` or `NOT_FOUND` |
| `INCR key` / `DECR key` | `VALUE <n>` (missing key starts at 0; errors if the current value isn't an integer) |
| `APPEND key value` | `LEN <n>` (new total length; creates the key if missing) |
| `GETRANGE key start end` | `VALUE <substring>` (inclusive range; negative indices count from the end; out-of-range is an empty value, not an error) |
| `SETRANGE key offset value` | `LEN <n>` (new total length; pads any gap before `offset` with spaces) |

`INCR`/`DECR`/`APPEND`/`SETRANGE` mutate a string in place and preserve
any existing `EXPIRE` — unlike `SET`, which always clears it.

List (ordered values):

| Command | Reply |
|---|---|
| `LPUSH key value [value ...]` / `RPUSH key value [value ...]` | `LEN <n>` (new length, after pushing all given values) |
| `LPOP key` / `RPOP key` | `VALUE <value>` or `NOT_FOUND` |
| `LLEN key` | `LEN <n>` |
| `LRANGE key start stop` | one value per line, then `END` (inclusive range; negative indices count from the end) |

`LPUSH key a b c` pushes each value to the head in turn (so the list ends
up `c b a`), matching Redis; `RPUSH key a b c` pushes to the tail (`a b c`).

Hash (field → value):

| Command | Reply |
|---|---|
| `HSET key field value` | `OK` |
| `HGET key field` | `VALUE <value>` or `NOT_FOUND` |
| `HDEL key field` | `OK` or `NOT_FOUND` |
| `HLEN key` | `LEN <n>` |
| `HGETALL key` | alternating `field`/`value` lines, then `END` |

Set (unique members):

| Command | Reply |
|---|---|
| `SADD key member [member ...]` | `ADDED <n>` (count of members that were newly added, excluding duplicates) |
| `SREM key member` | `OK` or `NOT_FOUND` |
| `SISMEMBER key member` | `TRUE` or `FALSE` |
| `SCARD key` | `LEN <n>` |
| `SMEMBERS key` | one member per line, then `END` |

Sorted Set (members ordered by score):

| Command | Reply |
|---|---|
| `ZADD key score member [score member ...]` | `ADDED <n>` (count of members newly added; repositioning an existing member's score doesn't count) |
| `ZSCORE key member` | `VALUE <score>` or `NOT_FOUND` |
| `ZREM key member` | `OK` or `NOT_FOUND` |
| `ZCARD key` | `LEN <n>` |
| `ZRANGE key start stop` | `member score` per line, ascending by score, then `END` |

`ZADD` takes up to 128 score/member pairs per call; more than that (or a
score with no matching member) replies with an `ERR`.

A command against a key holding a different type replies
`ERR WRONGTYPE ...` (e.g. `LPUSH` on a key created by `SET`). Popping or
removing the last element of a collection deletes the key, same as Redis.

Values in `LPUSH`/`RPUSH`/`SADD` and members in `ZADD` are single
whitespace-delimited tokens (no embedded spaces) — that's what makes
multiple values per call unambiguous. `SET`/`HSET` values are still the
rest of the line and may contain spaces, since those commands take
exactly one value.

## Project layout

```
Cargo.toml       - binary crate `klyro`; only dependency is `libc` (for poll())

src/
  main.rs        - entry point: wires everything together
  server.rs      - TCP networking + poll()-based event loop, connection I/O
  commands.rs    - command-line parsing and dispatch
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

tests/           - the integration suite (see "Test" above)
  common/mod.rs  -  starts/stops a klyro subprocess, speaks its protocol
  generic.rs     -  PING/DEL/EXPIRE/TTL/TYPE/KEYS/DBSIZE/SAVE/SHUTDOWN
  types.rs       -  String/List/Hash/Set/Zset ops, WRONGTYPE, empty-delete
  multi.rs       -  multi-value LPUSH/RPUSH/SADD/ZADD
  scan.rs        -  KEYS glob patterns, SCAN's resumable cursor
  persistence.rs -  save/kill/reload round-trip, TTL across a restart

clients/         - official client libraries (see "Client libraries" above)
  python/        - sync, stdlib `socket` only
  node/          - async/Promise, TypeScript, zero runtime deps
  go/            - stdlib `net` only
```

Each concern lives in its own module so new features can be added as new
files without disturbing the others:

- A new command → add a `match` arm in `commands.rs` (and a helper on
  `Store` in `store.rs` if it needs new storage behavior).
- A new data type → add `src/types/<type>.rs` with its own storage +
  ops, add a `StoreType` variant + `get_or_create_*`/`get_existing_*`
  accessors in `store.rs` (the `define_collection_accessors!` macro
  covers most of the boilerplate), wire commands for it into
  `commands.rs`, and a case in `persist.rs`'s dump/load so it survives a
  restart.
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

- Values can't contain `\n`, and a single reply is capped at 64 KiB.
- No authentication; do not expose this on an untrusted network.
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
