# Client libraries: what's required

A scoping note for building official Klyro clients (Node.js/JS/TS, Python,
and more), as of 2026-09-09. Planning only — nothing here has been built
yet. See [../README.md](../README.md) for the current command/protocol
reference and [rust-migration.md](rust-migration.md) for the (separate,
independent) server-side migration note.

## What every client needs, regardless of language

The wire protocol ([server.c](../src/server.c), [commands.c](../src/commands.c))
is simple enough that a client is mostly a thin wrapper, but a handful of
details have to be gotten right or the client will silently misbehave:

- **Transport.** Plain TCP, no TLS, no auth, one connection per client
  instance. Requests are `\n`-terminated lines; a stray `\r` before it is
  tolerated (`trim()` strips both) but not required.
- **Replies come in two shapes:**
  - *Single-line* — most commands. Either data (`VALUE ...`, `OK`,
    `LEN <n>`, `COUNT <n>`, `TTL <n>`, `ADDED <n>`, `TRUE`/`FALSE`, a type
    name, `PONG`, `BYE`, `SHUTTING_DOWN`) or `ERR ...` (usage errors,
    `WRONGTYPE`).
  - *Multi-line* — `KEYS`, `LRANGE`, `HGETALL`, `SMEMBERS`, `ZRANGE`: zero
    or more data lines followed by a literal `END` line. **Important
    subtlety:** on an error (bad usage, `WRONGTYPE`), these commands reply
    with a single `ERR ...` line and *no* trailing `END` — a client that
    always waits for `END` on these commands will hang. The correct rule:
    if the first line of a multi-line reply starts with `ERR`, that's the
    whole reply; otherwise keep reading until a line equal to `END`.
  - Every reply line from the server is `\r\n`-terminated; requests only
    need `\n`.
- **Full current command surface.** [commands.c](../src/commands.c) already
  implements more than the README documents — `INCR`/`DECR`/`APPEND`/
  `GETRANGE`/`SETRANGE` exist in the code but aren't in the command table
  yet. A client should be built against the actual dispatch table in
  `commands.c`, not just the README, and the README gap should get fixed
  alongside (or before) writing clients.
- **Injection safety.** `SET`/`HSET` values are "rest of the line" (may
  contain spaces) but **must not contain `\n`** — the protocol has no
  escaping, so a value with an embedded newline would be parsed as a
  second command. List/Set/Zset values and keys are whitespace-delimited
  tokens, so they additionally can't contain *any* whitespace without
  getting silently split into extra arguments. Every client needs to
  validate this client-side (reject or escape) rather than trust the
  caller — this is the one real security-shaped requirement in an
  otherwise trusted-network protocol (see the README's "no authentication;
  do not expose this on an untrusted network" limitation).
- **Error mapping.** Each language needs one exception/error type carrying
  the raw `ERR ...` text, with `WRONGTYPE` distinguishable (either a
  subtype or a checkable field) since it's the one error callers are
  likely to branch on.
- **Reply size limit.** A single reply is capped at 64 KiB server-side
  (`MAX_MSG` in [server.c](../src/server.c)) — clients don't need to do
  anything special, but large `GET`/`LRANGE`/etc. results can legitimately
  hit `ERR line too long`-style limits and that's worth surfacing in docs,
  not swallowing.

## Cross-language design decisions to settle once

- **Sync vs. async API shape.** Node's idiomatic shape is Promise-based
  (backed by `net.Socket`); Python's is the open question — a sync
  `socket`-based client is simplest and enough for scripts/tests, an
  `asyncio` client is more idiomatic for server-side use. Could ship sync
  first and add async later without breaking the sync API.
- **Command pipelining.** The server reads and dispatches lines as they
  arrive and writes replies in the same order, so a client *could*
  pipeline (fire several commands before reading replies) for throughput.
  Simpler and safer to start with strictly request-then-await-reply
  (one in-flight command at a time) and revisit only if benchmarks call
  for it.
- **Connection lifecycle.** Auto-reconnect on drop, or fail fast and make
  reconnect the caller's job? Given there's no auth/session state to
  restore, fail-fast is the safer default — reconnect logic that retries
  writes silently can duplicate non-idempotent commands (`LPUSH`, `INCR`).
- **Naming.** One consistent package name pattern across registries, e.g.
  `klyro-client` on both npm and PyPI (needs a quick availability check on
  each registry before committing to it).

## Per-language scope

**Node.js / TypeScript** (one package covers both, since TS compiles to
plain JS and ships `.d.ts` types for TS consumers):
- `net.Socket`-based transport, Promise API, one command method per
  server command (typed request args, typed return values per the reply
  shapes above).
- Build via `tsc` to a `dist/` the package's `main`/`types` point at;
  ESM-only is enough for Node ≥16 (CJS interop via `require`d dual build
  adds tooling for little benefit at this size).
- Tests: spawn a real `klyro` subprocess (same approach as
  [tests/klyro_helper.py](../tests/klyro_helper.py)) and exercise every
  command against it — no mocking, since the whole point is protocol
  fidelity.

**Python:**
- `socket`-based transport (stdlib only, matching the project's existing
  "no dependencies" pattern in [tests/](../tests/)), sync client first.
- Packaged via `pyproject.toml`, type-hinted.
- Same subprocess-based test strategy, reusing the spawn/probe logic
  already in `tests/klyro_helper.py` rather than re-inventing it.

**More languages, if/when wanted:** the protocol is simple enough (one
TCP socket + line framing) that Go, Rust, Java, Ruby, or PHP clients are
each a small, self-contained port of the same design — worth doing on
demand rather than upfront. A Go client would be a natural next pick if
one more is wanted, since it's commonly reached for alongside
infra/server tooling like this.

## Shared testing strategy (avoids N drifting test suites)

Rather than each language's client growing its own hand-written
expectations, define one command → expected-reply fixture (e.g. a JSON or
text table: command line in, expected reply lines out, per command
including edge cases like empty results and `WRONGTYPE`) that every
client's test suite runs through against a live `klyro` subprocess. Keeps
the clients honest against each other and against the server as commands
get added.

## Repo layout

Following the existing per-concern convention (`src/types/`, `src/util/`,
one doc per topic in `docs/`): a `clients/<lang>/` directory per client
(`clients/node/`, `clients/python/`, ...), each self-contained with its
own package manifest, source, tests, and README — same pattern the server
side already uses for keeping concerns independent.

## Explicitly out of scope for now

- Publishing to npm/PyPI — needs registry accounts/credentials and is an
  external, visible action; a decision for whenever a client is actually
  ready to ship, not part of building it.
- Async Python client, connection pooling, pipelining — worth revisiting
  once a sync/single-connection client exists and there's a concrete
  reason (a real throughput need) to justify the added complexity.
