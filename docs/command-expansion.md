# Command set expansion, 2026-09-10

Klyro went from 39 commands to 107, closing Tier 1 and Tier 2 in
[redis-feature-gap.md](redis-feature-gap.md). The command work is below;
the `INFO`/`CONFIG` half is written up separately in
[configuration.md](configuration.md). This document records the
decisions that were not obvious, so the next person doesn't have to
re-derive them. See [../README.md](../README.md) for the resulting
command reference.

## What was added

| Area | Commands |
|---|---|
| Keyspace | `EXISTS`, `UNLINK`, `RENAME`, `RENAMENX`, `COPY`, `RANDOMKEY`, `FLUSHDB`, `FLUSHALL`, `ECHO` |
| Expiry | `PEXPIRE`, `EXPIREAT`, `PEXPIREAT`, `PTTL`, `PERSIST`, `GETEX` |
| Strings | `SET` option flags, `SETNX`, `SETEX`, `PSETEX`, `GETSET`, `GETDEL`, `MGET`, `MSET`, `INCRBY`, `DECRBY`, `INCRBYFLOAT`, `STRLEN` |
| Lists | `LINDEX`, `LSET`, `LINSERT`, `LREM`, `LTRIM`, `LPUSHX`, `RPUSHX`, `RPOPLPUSH`, `LMOVE`, and `count` on `LPOP`/`RPOP` |
| Hashes | `HSETNX`, `HMSET`, `HMGET`, `HEXISTS`, `HKEYS`, `HVALS`, `HSTRLEN`, `HINCRBY`, `HINCRBYFLOAT` |
| Sets | `SPOP`, `SRANDMEMBER`, `SMOVE`, `SMISMEMBER`, `SINTER`, `SUNION`, `SDIFF` and their `STORE` forms |
| Sorted sets | `ZINCRBY`, `ZMSCORE`, `ZRANK`, `ZREVRANK`, `ZREVRANGE`, `ZRANGEBYSCORE`, `ZREVRANGEBYSCORE`, `ZCOUNT`, `ZREMRANGEBYRANK`, `ZREMRANGEBYSCORE`, `ZPOPMIN`, `ZPOPMAX` |
| Server | `INFO`, `CONFIG GET`/`SET`/`RESETSTAT`, `ECHO` |

Plus variadic `DEL`, `HDEL`, `SREM`, and `ZREM`.

## Decisions

### Backwards compatibility was a hard constraint — and then it wasn't

> **Superseded.** The RESP rewrite that followed
> ([resp-protocol.md](resp-protocol.md)) changed every reply shape and
> retired the three hand-written clients. The reasoning below is kept
> because it explains why the command set looked the way it did in
> between, not because it still holds.

Three client libraries in `clients/` parsed exact reply strings, and 121
tests asserted them. Every existing reply shape was preserved, which
shaped two choices:

- **Variadic commands kept their single-argument reply.** `DEL key`
  still answers `OK`/`NOT_FOUND`; only `DEL key key` switches to
  `DELETED <n>`. An arity-dependent reply shape is not a design anyone
  would choose from scratch, but the alternative was breaking three
  clients and a dozen tests for a cosmetic gain.
- **`HSET` stayed single-pair.** Its value is the rest of the line, so a
  variadic form would be ambiguous. `HMSET` is the variadic spelling,
  with single-token values.

All 121 original tests still pass unmodified.

### `SET`'s flags are matched as a trailing suffix

> **Superseded** by the RESP rewrite, which gave arguments real
> boundaries. The episode is worth keeping for what the smoke test
> caught.

`SET key value` takes the whole rest of the line as the value, so values
may contain spaces. Redis puts the option flags *after* the value
(`SET k v NX EX 30`), which in a protocol with no argument boundaries is
indistinguishable from a four-word value.

The first implementation put the flags before the value
(`SET k NX EX 30 v`). It parsed unambiguously and was rejected after a
smoke test: typing Redis's documented order silently stored
`"token NX EX 30"` as the value, with no error. Silent wrong data is
worse than a rejected command.

What shipped instead: tokenize the line after the key, then walk
candidate split points from the left, and take the first suffix that
parses as a *complete* option list. At least one token always stays
behind as the value.

- `SET k v NX EX 30` → value `v`, flags `NX EX 30`. Redis order works.
- `SET k hello big world` → no valid option suffix, so the whole thing
  is the value.
- `SET k NX` → the split loop stops before consuming everything, so this
  stores the literal string `NX`.
- `SET k v EX abc` → `EX` needs a number, so the suffix doesn't parse
  and `v EX abc` is the value. A typo'd expiry reads as data, not an
  error.

The remaining hole is a value whose last words spell valid options
(`SET k done XX`). `SETEX`/`SETNX` are the unambiguous spellings, and
the RESP rewrite removes the ambiguity entirely.

`tokens_with_offsets` in [../src/util/strutil.rs](../src/util/strutil.rs)
exists for this: it pairs each token with its byte offset, so the value
can be sliced back out of the original line with its internal spacing
intact. `next_token` alone can't, because it collapses runs of spaces.

### `commands.rs` became `commands/`

The single file was 758 lines and would have passed 2,000. It is now a
router in `commands/mod.rs` plus one module per data type, mirroring
`types/`. Dispatch is a `match` on the command name that forwards to the
module, not a table, because the "last argument is the rest of the line"
rule means no two commands share an arity signature.

### Set algebra clones its inputs

`Store` hands out one `&mut` collection at a time, and `SINTER` needs
several at once. `collect_sets` in
[../src/commands/set.rs](../src/commands/set.rs) clones each source set.
Fine at this scale; worth revisiting if sets get large.

### Randomness

`SPOP`, `SRANDMEMBER`, and `RANDOMKEY` need a random pick.
[../src/util/rand.rs](../src/util/rand.rs) is a thread-local xorshift64*
seeded from the wall clock, rather than a new crate dependency: the
project builds with `libc` alone and nothing here is
security-sensitive. It is not suitable anywhere randomness needs to be
unpredictable.

## Corrections made along the way

- **`DBSIZE` counted expired-but-unswept keys.** `Store::size` returned
  `map.len()` with no liveness filter, so it could exceed what `KEYS`
  returned. It now filters, matching Redis.
- **`Store::exists` was dead code.** It was implemented and marked
  `#[allow(dead_code)]` because no command reached it. `EXISTS` now
  does.
- **`Store::ttl` and `pttl_ms` share one path**, so the second- and
  millisecond-granularity answers can't drift apart.

## Testing

260 tests, up from 121. The new integration files mirror the command
modules: `keyspace.rs`, `expiry.rs`, `strings.rs`, `lists.rs`,
`hashes.rs`, `sets.rs`, `sortedsets.rs`, `admin.rs`. Set and hash iteration order is
unspecified, so those tests sort before comparing.

Verified separately: a full save/reload cycle against data written by
the new commands, including a TTL set through `SET ... NX EX 300`,
round-trips through the unchanged dump format.

## Later: configuration and observability

A second pass added `INFO`, `CONFIG`, and a config file. Three decisions
from it worth recording here:

- **Memory is measured, not estimated.** A counting global allocator
  gives `INFO memory` the process's real live-allocation total for one
  relaxed atomic add and subtract per allocation. The alternative, an
  O(n) walk of the keyspace, would have been both slower and less
  accurate.
- **The hit ratio is measured as a delta around dispatch.** The store
  counts every lookup; the dispatcher records the change across one
  command and attributes it only if the command is read-only. That put
  the accounting in one place instead of across 107 handlers. Internal
  type checks had to switch to a non-counting lookup, or every read
  would have registered as two - which is exactly what the first
  version did, caught by a smoke test.
- **Config-file errors are all reported at once.** A file with three
  mistakes prints three lines and exits, rather than making the operator
  fix one, restart, and find the next.

## What happened next

The RESP rewrite landed immediately after this work and changed the
protocol out from under it: every reply became a typed RESP value, the
arity-dependent reply shapes were dropped for plain counts, `HSET`
became variadic after all, and the client libraries were retired rather
than caught up. See [resp-protocol.md](resp-protocol.md).

Still missing from this area: `CONFIG REWRITE`, so a runtime
`CONFIG SET` is not written back to the config file and does not survive
a restart.
