# Klyro

**Upstream origin:** https://github.com/rairai77/cache22 (abandoned skeleton, 3 commits, no activity since 2025-05-10)
**Inspiration:** https://www.youtube.com/watch?v=FFxEoQyNQKM — *"Coding a FAST Redis database server in C"*
**License:** none

## Summary

Klyro is an in-memory, Redis-style key/value data server in C. It started
from a bare, broken upstream skeleton called `cache22` (`main()` + header +
Makefile, no `mainloop` definition — didn't even build), which was fixed
and completed into a working server, then renamed to **Klyro** and split
into modules so features can be added one at a time. See
[../README.md](../README.md) for build/run/usage and the command
reference.

## Repo layout

| Path | Purpose |
|---|---|
| [src/klyro.h](../src/klyro.h) | Project identity: name, version, tagline. |
| [src/main.c](../src/main.c) | Entry point — parses the port argument and wires `commands` + `server` together. |
| [src/server.h](../src/server.h) / [src/server.c](../src/server.c) | TCP networking: non-blocking sockets, the `poll()`-based event loop, per-connection read/write buffering. Exposes `Conn`, `conn_reply`, `conn_request_close`, `server_stop` to the rest of the code — nothing outside this file touches a socket directly. |
| [src/commands.h](../src/commands.h) / [src/commands.c](../src/commands.c) | Line parsing and command dispatch for all five data types plus the generic key commands. The seam where new commands get added. |
| [src/store.h](../src/store.h) / [src/store.c](../src/store.c) | The keyspace: maps keys to a tagged value (`StoreType`: string/list/hash/set/zset) with optional per-key expiry (lazy on lookup + periodic active sweep), and per-type `get_or_create`/`get_existing` accessors. |
| [src/types/list.h](../src/types/list.h) / [src/types/list.c](../src/types/list.c) | List data type: a doubly linked list of strings. |
| [src/types/hash.h](../src/types/hash.h) / [src/types/hash.c](../src/types/hash.c) | Hash data type: a field → value map, built on `util/htable`. |
| [src/types/set.h](../src/types/set.h) / [src/types/set.c](../src/types/set.c) | Set data type: unique members, built on `util/htable` (values unused). |
| [src/types/zset.h](../src/types/zset.h) / [src/types/zset.c](../src/types/zset.c) | Sorted Set data type: a sorted array of (member, score), ordered by (score, member); O(n) lookup/range — simpler than Redis's skip list, adequate at this project's scale. |
| [src/util/htable.h](../src/util/htable.h) / [src/util/htable.c](../src/util/htable.c) | Shared string-keyed chaining hashtable (FNV-1a, resizes on load factor) — the building block behind the keyspace and behind Hash/Set. Lives in `util/`, not `types/`, since `store.c` also depends on it directly. |
| [src/util/strutil.h](../src/util/strutil.h) / [src/util/strutil.c](../src/util/strutil.c) | `trim`/`next_token`/`parse_int`/`parse_long`/`parse_double` - line-parsing helpers shared by `commands.c` (network protocol) and `persist.c` (dump file), both of which parse simple whitespace-delimited text lines. |
| [src/persist.h](../src/persist.h) / [src/persist.c](../src/persist.c) | Saves/loads the whole keyspace to a dump file: walks `store_foreach_entry`, writing a text record per key (plus `EXPIREAT` for keys with a TTL); on load, rebuilds the store and re-applies expiry as an absolute deadline so downtime is accounted for. Also owns the periodic autosave check (`persist_tick`, called from `commands_tick`). |
| [Makefile](../Makefile) | Builds `klyro` from every `.c` file under `src/` and `src/*/` (via `wildcard`), compiling with `-Isrc` so every file can use root-relative includes like `"types/list.h"` — dropping in a new module or subdirectory needs no Makefile edit. |
| [.gitignore](../.gitignore) | Standard C build-artifact ignores (`*.o`, `*.exe`, `*.dylib`, etc.), from upstream. |
| [README.md](../README.md) | Build/run instructions, command reference, project layout, known limitations. |

## Why this structure

Each module owns one concern and talks to the others only through its
header:

- `server.c` knows about sockets and buffers, nothing about the command
  protocol or storage.
- `commands.c` knows about the text protocol and command semantics,
  nothing about sockets (`conn_reply`/`conn_request_close` are opaque) or
  storage internals (`store_*`/type-specific `*_get`/`*_set` only).
- `store.c` knows about the keyspace and typing/expiry, not what's inside
  a `List`/`Hash`/`Set`/`Zset` — those are opaque pointers to it.
- `types/list.c`/`types/hash.c`/`types/set.c`/`types/zset.c` each know
  only their own data structure, nothing about the keyspace, networking,
  or protocol.
- `util/htable.c` is pure infrastructure — a string-keyed hashtable with
  no knowledge of what it's used for; `store.c`, `types/hash.c`, and
  `types/set.c` each wrap it for their own purpose instead of three
  separate chaining-hashtable implementations. `util/strutil.c` is the
  same idea for line parsing, shared by `commands.c` and `persist.c`.

`src/` has two subdirectories, one per genuine cluster: `types/` (the
four data-type modules) and `util/` (generic infrastructure with no
keyspace/protocol knowledge). `server.c`/`commands.c`/`store.c`/
`persist.c` stay flat at the top of `src/` — each is a single, distinct
concern, so nesting a lone `.c`/`.h` pair into its own directory would
add indirection without reducing clutter.

The intent is that later features slot in as new files without editing
the others: a new data type as `src/types/<type>.c` (+ a `StoreType`
case and accessors in `store.c`), persistence as `src/persist.c` hooked
from `main.c`, etc. — the Makefile picks up any new `.c` file under
`src/` or one level of subdirectory automatically.

## Status (2026-09-08)

- Upstream's build failure (`mainloop` declared but never defined,
  `scontinuation` defaulting to `false`) is fixed.
- Renamed `cache22` → **Klyro**, binary `cache22` → `klyro`, split the
  single `cache22.c`/`hashtable.c` pair into modules, then added four
  data types: List, Hash, Set, and Sorted Set — each in its own module,
  sharing the new `htable.c` building block.
- Added `WRONGTYPE` errors (a command against a key of a different type)
  and Redis's "an emptied collection's key stops existing" behavior
  (`LPOP`/`RPOP`/`HDEL`/`SREM`/`ZREM` delete the key once it's empty).
- Made `LPUSH`/`RPUSH`/`SADD`/`ZADD` variadic (multiple values/pairs per
  call), matching Redis; `SADD`/`ZADD` now reply with a count of newly
  added members (`ADDED <n>`) instead of the old single-value `ADDED`/
  `EXISTS`/`OK` replies.
- Added persistence (`persist.c`): a full-keyspace text dump, loaded on
  startup and saved on `SAVE`, graceful shutdown, and a 60s autosave
  when dirty. Extracted `strutil.c` (`trim`/`next_token`/`parse_*`) out
  of `commands.c` so `persist.c` could reuse the same line-parsing
  helpers instead of duplicating them.
- Verified: `make` builds cleanly with no warnings; scripted clients
  exercised every command across all five types (including WRONGTYPE,
  empty-collection deletion, and multi-value push/add) and a full
  persistence round-trip (save → `SIGKILL` → reload, including a key
  that expired during the simulated downtime, and a second round-trip
  through a graceful `SHUTDOWN` save) — all got the expected results.
- Reorganized the 22 flat files in `src/` into `src/types/` (the four
  data-type modules) and `src/util/` (the two generic infrastructure
  modules), leaving `main.c`/`server.c`/`commands.c`/`store.c`/
  `persist.c` at the top level. Switched every cross-directory
  `#include` to be root-relative to `src/` (e.g. `"types/list.h"`) and
  added `-Isrc` to the Makefile's compile flags so that resolves
  regardless of which file does the including; same-directory includes
  (e.g. `list.c` including its own `list.h`) were left unprefixed.
  Re-ran the full scripted regression suite (types, WRONGTYPE,
  multi-value, persistence round-trip) after the move — unchanged.

## Notes / observations

- The unused `int32`/`int16`/`int8` typedefs from the original upstream
  skeleton (all secretly `unsigned`, despite the signed-looking names)
  were dropped during the restructure — they were dead code, never
  referenced anywhere.
- Sorted Set uses a sorted array with linear-scan lookup/insert, not a
  skip list — simpler and correct, but O(n) rather than O(log n); fine
  until sets get large.
- Debugging the persistence load path surfaced a real, non-obvious bug:
  `trim()` (in the original single-file `commands.c`) never needed to
  strip a trailing `\n` because `server.c` always strips it first when
  splitting the network stream into lines — so `trim()` only ever saw
  `\r`. `persist.c` reads lines with `fgets()`, which *does* leave the
  `\n` in the buffer, so every `parse_int`/`parse_long` call on a
  count/timestamp field silently failed (trailing `\n` isn't part of the
  number), silently skipping every List/Hash/Set/Zset/EXPIREAT record on
  load - only `STRING` records loaded, and even those kept a stray `\n`
  in the value. Fixed by having `trim()` strip `\n` too. A related bug
  in the same area: `persist_tick`'s `last_check` started at `0`, so the
  very first tick always looked like ">60s since the last save" and
  fired an autosave immediately instead of waiting for the interval.
- No tests, no CI, no auth — see [../README.md](../README.md)'s "Known
  limitations".
