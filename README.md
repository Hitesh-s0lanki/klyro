# Klyro

**The high-performance in-memory data server.**

An in-memory, Redis-style data server in C, with String, List, Hash, Set,
and Sorted Set data types. Started from
https://github.com/rairai77/cache22 (a bare skeleton, following
https://www.youtube.com/watch?v=FFxEoQyNQKM) and grown from there.

## Build

```sh
make
```

## Run

```sh
./klyro [port] [dump-file]   # defaults: port 7171, dump-file klyro.dump
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

## Commands

Generic (any type):

| Command | Reply |
|---|---|
| `PING` | `PONG` |
| `DEL key` | `OK` or `NOT_FOUND` |
| `EXPIRE key seconds` | `OK` or `NOT_FOUND` |
| `TTL key` | `TTL <seconds>` (`-1` = no expiry, `-2` = missing) |
| `TYPE key` | `STRING`/`LIST`/`HASH`/`SET`/`ZSET`, or `NONE` if missing |
| `KEYS` | one key per line, terminated by `END` |
| `DBSIZE` | `COUNT <n>` |
| `SAVE` | `OK` (writes the dump file immediately) |
| `QUIT` | `BYE`, then closes the connection |
| `SHUTDOWN` | `SHUTTING_DOWN`, then stops the server (saving first) |

String:

| Command | Reply |
|---|---|
| `SET key value` | `OK` (value is the rest of the line — may contain spaces) |
| `GET key` | `VALUE <value>` or `NOT_FOUND` |

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
src/
  klyro.h        - project identity (name/version/tagline)
  main.c         - entry point: wires everything together
  server.h/.c    - TCP networking + poll()-based event loop, connection I/O
  commands.h/.c  - command-line parsing and dispatch
  store.h/.c     - the keyspace: maps keys to typed values, with expiry
  persist.h/.c   - save/load the whole keyspace to a dump file
  types/         - the data type implementations
    list.h/.c    -   List (doubly linked list)
    hash.h/.c    -   Hash (field -> value map)
    set.h/.c     -   Set (unique members)
    zset.h/.c    -   Sorted Set (members ordered by score)
  util/          - generic infrastructure with no keyspace/protocol knowledge
    htable.h/.c  -   shared string-keyed hashtable (used by store/hash/set)
    strutil.h/.c -   shared line-parsing helpers (used by commands + persist)
```

Each concern lives in its own module so new features can be added as new
files without disturbing the others:

- A new command → add a branch in `commands.c` (and a helper in `store.c`
  if it needs new storage behavior).
- A new data type → add `src/types/<type>.h/.c` with its own storage +
  ops (reuse `util/htable.c` if it needs fast key lookup), add a
  `StoreType` + accessors in `store.c`, wire commands for it into
  `commands.c`, and a case in `persist.c`'s dump/load so it survives a
  restart.
- Pub/sub, replication, etc. → new modules alongside `server.c`/`store.c`,
  hooked in from `main.c`.

Includes are written root-relative to `src/` (e.g. `#include
"types/list.h"`) regardless of which file does the including — the
Makefile passes `-Isrc` so this resolves the same everywhere. Only
same-directory includes (e.g. `list.c` including its own `list.h`) skip
the prefix.

`Makefile` picks up any `.c` file dropped into `src/` automatically.

## Persistence format

The dump file is a simple text format (see `persist.c`): a `KLYRO-DUMP 1`
header line, then one record per key - `STRING key value`, or
`LIST|HASH|SET|ZSET key count` followed by `count` data lines - plus an
optional `EXPIREAT key unix-timestamp` line after a key's record if it
has a TTL. Saves are atomic (written to `<path>.tmp`, then renamed over
the real path), so a crash mid-save can't corrupt the existing dump.

## Known limitations

- Values can't contain `\n`, and a single reply is capped at 64 KiB.
- No authentication; do not expose this on an untrusted network.
- Sorted Set range/lookup ops are O(n) (a sorted array, not a skip list) —
  fine at moderate scale, not built for huge sets.
- Persistence is a full-keyspace snapshot (like Redis's RDB), not an
  append-only log — a `SIGKILL`/crash loses everything since the last
  save (on a normal exit, at most ~60s of changes).
