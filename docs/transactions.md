# Transactions

`MULTI`, `EXEC`, `DISCARD`, `WATCH`, `UNWATCH` and `RESET`, with the same
semantics Redis gives them. See [../README.md](../README.md) for the
command reference.

## What a transaction is here

`MULTI` opens one. Every command after it answers `QUEUED` instead of
running, until `EXEC` runs them all back to back, or `DISCARD` throws
them away.

```
MULTI                 +OK
SET k 1               +QUEUED
INCR k                +QUEUED
EXEC                  1) +OK
                      2) :2
```

Atomicity comes free from the architecture: the server is single
threaded, so the whole queue runs inside one turn of the event loop and
no other client's command can interleave.

**There is no rollback.** A command that fails inside `EXEC` reports its
error as one element of the reply array, and every other command still
runs. This is Redis's behaviour, and the reasoning is the same: a
command that fails at run time failed because it was wrong for the data,
which is a programming error, and unwinding would cost more than it is
worth.

```
SET str v
MULTI
SET before 1          +QUEUED
LPUSH str x           +QUEUED
SET after 1           +QUEUED
EXEC                  1) +OK
                      2) -WRONGTYPE ...
                      3) +OK
```

Both `before` and `after` are set afterwards.

## When a transaction refuses to run

Two things stop `EXEC` before it starts anything.

**A command that could never run.** If a queued command is not a command
at all, the error is reported at queue time *and* the transaction is
marked broken, so `EXEC` refuses it:

```
MULTI
NOSUCHCOMMAND         -ERR unknown command 'NOSUCHCOMMAND' ...
EXEC                  -EXECABORT Transaction discarded because of previous errors.
```

Nothing from that transaction runs. `DISCARD` clears the broken state so
the connection can start again.

**A watched key moved.** See below.

## WATCH

`WATCH` is optimistic locking, and it is what makes read-modify-write
safe without a lock:

```python
with r.pipeline() as pipe:
    pipe.watch("counter")
    current = int(pipe.get("counter"))
    pipe.multi()
    pipe.set("counter", current + 1)
    pipe.execute()      # raises WatchError if someone else got there first
```

If any watched key is modified between `WATCH` and `EXEC`, the
transaction does not run and `EXEC` replies with a null array, which
client libraries surface as an error to retry on.

The rules:

- **Any modification counts**, including one made by the watching
  connection itself. Redis is the same.
- **Reads never disturb a watch.** `GET`, `LRANGE`, `HGETALL`, and the
  rest leave it intact, whoever runs them.
- **A change inside a collection counts.** `LSET l 0 x` on a list that
  neither appears nor disappears still breaks a watch on `l`.
- **Deletes and expiries count**, whether the key was removed by `DEL`,
  by `FLUSHALL`, or by its TTL running out.
- **Watches are per connection.** One connection's `WATCH` never affects
  another's transaction.
- **`EXEC` and `DISCARD` both clear every watch**, as does `UNWATCH`,
  `RESET`, and closing the connection.
- **`WATCH` inside `MULTI` is an error.** The check happens at `EXEC`,
  which is already next, so a watch started mid-queue would be
  meaningless.

## How the watch check works

A per-key *stamp* rather than a callback registry.

`Store` keeps a `watched` map from key to `(stamp, watchers)`. `WATCH`
registers the key and hands the session the key's current stamp;
`Store::touch` moves the stamp whenever the key is modified; `EXEC`
compares each recorded stamp against the current one. Any mismatch and
the transaction is abandoned.

`watchers` is a refcount, so an entry disappears once no session cares
about that key. Only keys under active `WATCH` are in the map, so the
cost is bounded by how much `WATCH` is used, not by keyspace size.
`INFO clients` reports the current size as `watched_keys`.

The reason this is reliable is that every mutation funnels through one
place. That was not true before this work: collections handed out `&mut`
references through a single accessor used by readers and writers alike,
so the store could not tell a read from a write. See below.

## The bug this uncovered

While building the watch hook it turned out that **mutations that left a
collection non-empty never marked the store dirty at all**. `LPOP` on a
three-element list, `HDEL` of one field out of two, `SREM`, `ZREM`,
`LSET`, `LTRIM` - none of them moved the unsaved-change counter.

The consequence was real: the periodic autosave only writes when the
store is dirty, so those writes could be lost on a crash or `SIGKILL`,
and `INFO persistence` under-reported. It had been that way since the
collection accessors were written.

The cause was one accessor doing double duty. `get_existing_list` and
friends returned `&mut`, and were called both by readers (`LRANGE`) and
by writers (`LPOP`). Marking dirty on every call would have made reads
look like writes; marking on none of them is what actually happened.

The fix splits them, so the call site declares its intent:

| Accessor | Returns | Marks dirty | Moves the watch stamp |
|---|---|---|---|
| `read_list`, `read_hash`, ... | `&T` | no | no |
| `write_list`, `write_hash`, ... | `&mut T` | yes | yes |
| `get_or_create_list`, ... | `&mut T` | yes | yes |

The compiler found every call site that needed the write variant: after
renaming everything to `read_*`, each mutation failed to compile with
"types differ in mutability". Tests in
[../tests/persistence.rs](../tests/persistence.rs) now pin both halves -
that edits mark the store unsaved, and that reads do not.

## Deviations from Redis

**Arity is not checked at queue time.** Redis rejects
`MULTI; GET; EXEC` when `GET` is queued, and aborts the transaction.
Klyro queues it and reports the arity error as an element of the `EXEC`
reply instead. Only *unknown commands* are caught at queue time. The
reason is that arity lives inside each handler rather than in a table,
and duplicating it into one would create something that drifts.

**`UNWATCH` inside `MULTI` is queued**, as it is in Redis, and running
it inside `EXEC` is a no-op because `EXEC` clears the watches anyway.

## The memory type

The `MEM.*` family reaches the store through its own accessor rather
than the `read_*`/`write_*` pair, because a memory index is one value
whose commands mutate it in place. It reports changes explicitly with
`Store::mark_dirty(key)`, which moves the watch stamp exactly as
`write_*` does, so `WATCH ns` breaks on `MEM.ADD` and survives
`MEM.GET`.

`MEM.*` commands are routed by prefix rather than being listed by name,
so the queue-time check recognises the prefix too - otherwise a memory
command would be refused inside `MULTI` while working fine outside it.

## What is still missing

No `WAIT`, no `CLIENT NO-EVICT`/`CLIENT UNPAUSE`, and no scripting - a
Lua script is the other way Redis offers to make several operations
atomic. Blocking commands inside `MULTI` are not a concern yet, since
Klyro has none.
