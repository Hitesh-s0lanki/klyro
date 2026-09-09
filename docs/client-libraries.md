# Client libraries: what's required

A scoping note for building official Klyro clients (Node.js/JS/TS, Python,
and more), as of 2026-09-09. See [../README.md](../README.md) for the
current command/protocol reference and [rust-migration.md](rust-migration.md)
for the (separate, independent) server-side migration note.

**Status: Python, Node.js/TypeScript, and Go clients are built** (see
[clients/python/](../clients/python/), [clients/node/](../clients/node/),
[clients/go/](../clients/go/) — each has its own README, is
dependency-free beyond its language's standard library, and is tested
against the real compiled server). The design decisions below (reply
shapes, error mapping, sync-vs-async, fail-fast reconnection, per-language
scope) are what those three clients actually implement; the rest of this
document (naming, publishing, additional languages) is still forward-looking
planning.

## What every client needs, regardless of language

The wire protocol ([server.rs](../src/server.rs), [commands.rs](../src/commands.rs))
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
- **Full current command surface.** A client should be built against the
  actual dispatch table in [commands.rs](../src/commands.rs), not just
  the README, in case the two ever drift.
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
  (`MAX_MSG` in [server.rs](../src/server.rs)) — clients don't need to do
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

**Node.js / TypeScript** — done, see [clients/node/](../clients/node/)
(one package covers both, since TS compiles to plain JS and ships
`.d.ts` types for TS consumers):
- `net.Socket`-based transport, Promise API, one command method per
  server command (typed request args, typed return values per the reply
  shapes above).
- Built via `tsc` to `dist/`, which the package's `main`/`types` point
  at; ESM-only (Node ≥18).
- Tests: spawn a real `klyro` subprocess (same approach as
  [tests/common/mod.rs](../tests/common/mod.rs)) and exercise every
  command against it — no mocking, since the whole point is protocol
  fidelity. Run via Node's built-in test runner (`node --test`), no
  external test framework dependency.

**Python** — done, see [clients/python/](../clients/python/):
- `socket`-based transport (stdlib only, matching the project's existing
  "no dependencies" pattern in [tests/](../tests/)), sync client.
- Packaged via `pyproject.toml`, type-hinted (ships a `py.typed` marker).
- Same subprocess-based test strategy, reusing the spawn/probe logic
  already in `tests/common/mod.rs` rather than re-inventing it.

**Go** — done, see [clients/go/](../clients/go/):
- `net`-based transport (stdlib only), a `Dial()`-first API idiomatic
  for a small Go TCP client, internally mutex-guarded for safe
  concurrent use of one `*Client`.
- Packaged as its own module (`go.mod`), zero dependencies.
- Same subprocess-based test strategy via the standard `testing`
  package.

**More languages, if/when wanted:** the protocol is simple enough (one
TCP socket + line framing) that Rust, Java, Ruby, or PHP clients are
each a small, self-contained port of the same design — worth doing on
demand rather than upfront.

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
