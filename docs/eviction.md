# maxmemory and eviction

How Klyro stays inside a memory budget: what it measures, when it acts,
which key it drops, and what a client sees when there is nothing left to
drop. Written 2026-09-10.

Until this landed, Klyro grew until the operating system killed it,
which ruled out the most common Redis deployment shape of all - a cache
with a ceiling. The measurement half was already done: a counting global
allocator in [util/memory.rs](../src/util/memory.rs) has reported real
process memory to `INFO` since the configuration work. What was missing
was the policy half, and that is what this describes. See
[configuration.md](configuration.md) for the parameters in context and
[klyro.md](klyro.md) for the architecture around them.

---

## 1. The three parameters

| Parameter | Default | Meaning |
|---|---|---|
| `maxmemory` | `0` | Bytes the process may hold. `0` is no limit. |
| `maxmemory-policy` | `noeviction` | Which keys may go, and which goes first. |
| `maxmemory-samples` | `5` | Keys drawn per eviction round. |

All three are settable from the config file and with `CONFIG SET`, so a
running server can be given a ceiling, or have one lifted, without a
restart.

`maxmemory` accepts Redis's size suffixes, because it is the one
parameter people write by hand and `100mb` is what they reach for:

```
maxmemory 512mb
maxmemory-policy allkeys-lru
```

`k`, `m`, and `g` are powers of a thousand; `kb`, `mb`, and `gb` are
powers of 1024. That reads backwards, and it is what every Redis config
in existence means, so Klyro means it too. `CONFIG GET maxmemory` always
answers in plain bytes.

## 2. The eight policies

The first half of the name says which keys are eligible, the second
says which of them goes first.

| Policy | Eligible | Victim |
|---|---|---|
| `noeviction` | none | none - writes are refused instead |
| `allkeys-lru` | every key | least recently used |
| `allkeys-lfu` | every key | least frequently used |
| `allkeys-random` | every key | any |
| `volatile-lru` | keys with a TTL | least recently used |
| `volatile-lfu` | keys with a TTL | least frequently used |
| `volatile-random` | keys with a TTL | any |
| `volatile-ttl` | keys with a TTL | expiring soonest |

`noeviction` is the default, and deliberately so. Silently dropping data
a client believes it stored is a worse surprise than an error naming the
reason, and a server that has never been given a policy has never been
told which of its keys are expendable.

A `volatile-*` policy on a keyspace where nothing carries a TTL behaves
exactly like `noeviction`: there is no eligible key, so nothing is
evicted and the write is refused. That is Redis's behaviour and it is
worth knowing before choosing one.

## 3. What gets measured

The number compared against `maxmemory` is the allocator's total for the
whole process, not the size of the keyspace. Two consequences follow,
and Redis shares both:

- **A limit below what the process needs at rest can never be
  satisfied.** Client buffers, the reply being built, and the runtime
  itself are all inside the number. Setting `maxmemory` to a few
  hundred kilobytes evicts the entire keyspace and then still reports
  OOM.
- **Evicting reclaims values, not the index.** A freed value leaves the
  allocator's total immediately, but the keyspace's hash table does not
  shrink when entries leave it, so a database that grew large and was
  then evicted down holds an empty table of the size it once needed.

## 4. When eviction runs

Before every command, not only before a write.

That looks wasteful and is not. A server over its limit is over it
whether the next command reads or writes, and waiting for a write to
notice would leave it over for as long as the traffic stayed read-only.
The check itself is a comparison against an atomic counter, which is
what makes running it that often affordable. Redis does the same, in
`processCommand`.

What the command *is* decides only what happens when eviction falls
short. The commands that could grow the keyspace are refused with

```
OOM command not allowed when used memory > 'maxmemory'.
```

and everything else goes through. The line is drawn at what a command
does to the total, not at whether it writes: `DEL`, `LPOP`, `EXPIRE`,
and `FLUSHALL` all change the keyspace and none of them can grow it, so
all of them keep working on a full server. That is not a detail - it is
the only way out of the state. The list lives in
[keyspec.rs](../src/commands/keyspec.rs) beside the table of which keys
each command writes, and Redis marks the same set with its `denyoom`
flag.

An eviction signals the watch registry, so a transaction holding a
`WATCH` on an evicted key aborts rather than running against a read that
is no longer true.

## 5. Sampling, not sorting

Klyro picks a victim the way Redis does: draw `maxmemory-samples` keys
at random, take the best one by the policy's ordering, repeat until the
process is back under the limit.

The alternative - an exact answer - would mean a structure ordered by
last access and updated on every read, which costs something on every
command in exchange for accuracy that does not show up in a hit rate.
Five samples land within a couple of percent of true LRU on realistic
traffic. Raising `maxmemory-samples` narrows that gap and costs more per
eviction.

Sampling needs a random key in constant time, and `std`'s `HashMap` has
no indexable bucket to draw from. So [store.rs](../src/store.rs) keeps
two vectors of keys beside the map: every key, and the subset carrying
an expiry. Each entry remembers its slot in both, and `insert_entry` and
`remove_entry` - the only two places the map grows or shrinks - keep
them in step, repairing the slot of whatever `swap_remove` moved into a
hole. The second vector is what makes the `volatile-*` policies practical:
without it, a keyspace where one key in a thousand carries a TTL would
spend a thousand draws finding one eligible candidate.

Two things came free with that index. `RANDOMKEY` used to collect the
whole keyspace into a vector to pick one element from it, and now draws
directly. `INFO`'s count of keys with an expiry used to walk every key,
and now walks only the ones it is counting.

## 6. Tracking recency and frequency

Every entry carries three small fields:

- `at` - the store's logical clock when the key was last accessed. A
  counter, not a wall time, so LRU ordering is exact and a clock
  adjustment cannot reorder it. Redis uses a 24-bit clock in seconds and
  accepts the resolution loss; there is no reason to copy that here.
- `freq` - the LFU counter.
- `decayed_at` - the minute `freq` was last brought up to date.

`freq` is Redis's logarithmic counter, and the shape of it is the point.
It starts at 5, so a key written a moment ago is not the first victim of
the command that created it. It rises probabilistically, with odds that
fall as it climbs, so one byte can distinguish a key read ten times from
one read ten million times. And it halves once per idle minute, so a key
that was hot an hour ago does not outlive one that is hot now. Without
that decay, LFU would be a record of history rather than of demand, and
the first keys to get hot would never leave.

The two knobs around it - the log factor and the decay interval - are
constants here rather than parameters. They are the two nobody turns.

The counter is one byte, so a cold keyspace has every key sitting on the
same value with nothing else to separate them. LFU therefore breaks a
tie on the counter by age, which makes it behave like LRU exactly where
it has no frequency information to go on.

## 7. What INFO reports

`INFO memory` gained three lines:

```
maxmemory:536870912
maxmemory_human:512.00M
maxmemory_policy:allkeys-lru
```

and `INFO stats` gained one:

```
evicted_keys:1043
```

kept apart from `expired_keys` on purpose. An expiry is what the client
asked for; an eviction is the server overruling it. A rising
`evicted_keys` with a flat `keyspace_hits` ratio means the cache is
sized about right; rising together means it is too small.

`CONFIG RESETSTAT` clears the eviction count, as it does the rest of the
activity counters.

## 8. What this does not do

- **No `maxmemory-clients`.** A client's output buffer counts toward
  `maxmemory` but has no separate ceiling of its own beyond
  `client-output-buffer-limit`.
- **No eviction pool.** Redis keeps a pool of the best candidates seen
  across rounds, so a good victim spotted in one round is not forgotten
  by the next. Klyro samples fresh each round, which is slightly worse
  per eviction and much simpler.
- **No `OBJECT FREQ` or `OBJECT IDLETIME`.** The tracking exists; the
  commands that expose it do not.
- **Eviction metadata is not persisted.** A key loaded from a dump
  starts with a fresh access record, so the first eviction pass after a
  restart has only the current session to go on. Redis's RDB is the same
  by default.
- **The counter is not the keyspace.** Section 3 covers this; it is
  repeated here because it is the one thing that surprises people.
