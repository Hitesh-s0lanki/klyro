# Klyro: what a C → Rust migration would require

**Status: done (2026-09-09).** This started as a scoping note; the port
described below was then carried out on the `rust-migration` branch,
module by module in the order suggested here, validated at each step
against the same behavior the C version had. `src/` is now a Cargo
binary crate and [tests/](../tests/) is a Rust integration suite (see
the README's "Test" section) - the C sources and the old Python test
suite are gone. The wire protocol and the on-disk dump format are
unchanged, so old `klyro.dump` files still load. The rest of this
document is kept as-written for the reasoning behind each choice; see
[klyro.md](klyro.md) for the (now historical) C architecture and
[roadmap.md](roadmap.md) for the current gap list, which is
language-agnostic and still applies.

One deliberate deviation from the plan below: `SCAN`'s cursor is now a
position in a sorted snapshot of the keyspace rather than a raw
hashtable bucket index, since `std::collections::HashMap` doesn't
expose one - same user-visible contract (a soft batch-size hint, `0`
means done, same caveat about concurrent mutation), different internal
mechanism.

## Why this is tractable

- The codebase is small (~2000 lines across 11 `.c`/`.h` pairs) and
  already split into single-concern modules (`server`, `commands`,
  `store`, `persist`, `types/*`, `util/*`) that map close to 1:1 onto
  Rust modules.
- The integration suite in [../tests/](../tests/) talks to the server
  over raw TCP as a black box (line protocol, not a C API) — it needs
  **zero changes** and can validate a Rust rewrite directly, module by
  module, by pointing `klyro_helper.py` at the new binary.
- The persistence format ([persist.c](../src/persist.c)) is plain text.
  Keeping it byte-compatible means old `klyro.dump` files stay loadable
  and both binaries could round-trip the same dump during the
  transition.

## Toolchain / project setup

- A `Cargo.toml` (binary crate `klyro`) replaces the [Makefile](../Makefile).
  `make test` becomes `cargo build --release` + the existing
  `python3 -m unittest discover -s tests`.
- Decide on dependencies. A minimal port needs none beyond `std` — TCP,
  `HashMap`/`HashSet`/`VecDeque`, and string parsing are all in the
  standard library, matching this project's current "no dependencies"
  ethos (the C build has none, the Python tests use stdlib only). The
  one place a crate earns its keep is the event loop (see below).

## The one real architectural decision: the event loop

[server.c](../src/server.c) is a single-threaded `poll()` loop over
non-blocking sockets. Three Rust options, in order of how much the
rest of the code has to change:

1. **Reimplement with raw `poll()`/`epoll` via `libc`** — closest
   translation, same single-threaded model, `Store` needs no
   synchronization. Most faithful port.
2. **`mio`** — idiomatic Rust equivalent of the same readiness-based
   model, still single-threaded, still no `Arc<Mutex<_>>` needed. Likely
   the best fit.
3. **`tokio`/async, or thread-per-connection** — bigger shift: the
   store becomes shared mutable state (`Arc<Mutex<Store>>` or an actor),
   command dispatch becomes `async fn`, and the concurrency semantics
   actually change (today everything is serialized through one loop,
   which is part of why the code has no locking anywhere). Only worth
   it if concurrent throughput is a real goal, not just a language swap.

Everything else below assumes option 1 or 2 (same concurrency model, so
`store.c`'s design carries over largely unchanged).

## Module-by-module mapping

| C module | Rust shape |
|---|---|
| [main.c](../src/main.c) | `fn main()`, `std::env::args()` (or `clap` if arg parsing grows) |
| [server.h/.c](../src/server.c) | `mio`/`libc` event loop; `Conn` struct owns its read/write buffers instead of manual malloc'd buffers |
| [commands.h/.c](../src/commands.c) | Parse into a `Vec<&str>` (`split_whitespace`), dispatch via `match` on the command name instead of the current if/else chain |
| [store.h/.c](../src/store.c) | `HashMap<String, Entry>` where `Entry` is an `enum StoreType { Str(String), List(...), Hash(...), Set(...), Zset(...) }` with an `Option<Instant>` expiry — the tagged union becomes a real Rust enum, so `TYPE`/`WRONGTYPE` checks become exhaustive `match`es instead of manual tag comparisons |
| [types/list.c](../src/types/list.c) | `VecDeque<String>` (drop the hand-rolled doubly linked list entirely) |
| [types/hash.c](../src/types/hash.c) | `HashMap<String, String>` |
| [types/set.c](../src/types/set.c) | `HashSet<String>` |
| [types/zset.c](../src/types/zset.c) | `Vec<(String, f64)>` kept sorted by `(score, member)`, same O(n) approach noted as an acceptable limitation in the README — or a `BTreeMap`-based structure if this is the moment to fix the O(n) lookup |
| [util/htable.c](../src/util/htable.c) | **Deleted.** `std::collections::HashMap`/`HashSet` replace the hand-rolled FNV-1a chaining table used by `store`/`hash`/`set` |
| [util/strutil.c](../src/util/strutil.c) | **Deleted.** `str::trim`, `split_whitespace`, `str::parse::<i64>()`/`parse::<f64>()` replace `trim`/`next_token`/`parse_int`/`parse_long`/`parse_double` |
| [persist.h/.c](../src/persist.c) | Same text format via `std::fs` + `write!`/line parsing; keep the `<path>.tmp` + rename atomicity |

## Semantic shifts, not just syntax

- **No manual memory management.** Every `malloc`/`free` pair in
  `store.c`/`types/*.c` (and the bugs that come with getting one half of
  the pair wrong) disappears — ownership is expressed in the struct
  definitions instead. This is the biggest actual win, not just a
  language swap.
- **Error handling.** C's `NULL`-return/error-code conventions become
  `Option<T>`/`Result<T, E>` — every `store_get_existing`-style function
  and every `parse_*` helper gets a real type for "missing"/"malformed"
  instead of a sentinel.
- **No `Rc`/`RefCell`/`Arc`/`Mutex` needed** if the event loop stays
  single-threaded (option 1/2 above) — `Store` can be owned outright by
  the loop, matching the current design more closely than it might seem
  at first.

## Migration strategy

C and Rust can't share one binary without FFI, so this isn't a
file-by-file incremental replace in place — it's a new Cargo project
built up module by module, validated against the existing Python
integration suite at each step, with the C binary kept around as the
reference implementation until the Rust one passes the full suite.
Reasonable order, given the module dependency graph
(`util` → `types`/`store` → `commands`/`persist` → `server`/`main`):

1. `store` (as a Rust enum + `HashMap`, no networking yet) + one data
   type (e.g. String) with unit tests.
2. Remaining data types (List, Hash, Set, Zset).
3. `persist` (save/load), checked against real `.dump` files produced by
   the current C binary — this is the concrete compatibility test.
4. `commands` (dispatch, WRONGTYPE, variadic push/add).
5. `server` (the event loop) + `main`, then run the full `tests/`
   suite against the Rust binary unmodified.

## Out of scope for the port itself

Anything in [roadmap.md](roadmap.md) (RESP protocol, AOF, auth,
replication, `SCAN`, etc.) is an independent decision — worth doing in
Rust rather than C if it's happening anyway, but not required to
complete the language migration.
