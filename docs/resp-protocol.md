# The RESP rewrite

Klyro's wire protocol was replaced with RESP, the protocol Redis speaks,
on 2026-09-10. This is what changed and why. See
[client-libraries.md](client-libraries.md) for how to connect and
[redis-feature-gap.md](redis-feature-gap.md) for what is still missing.

## What it bought

The old protocol was a bespoke text format: one command per line, values
delimited by spaces, replies like `OK` and `VALUE hello`. Four problems
went away at once.

**Stock clients work.** redis-py, go-redis, and ioredis all connect and
pass a full command sweep with no adapter. The three hand-written client
libraries in `clients/` existed only to paper over the old protocol, and
have been removed.

**Values are binary-safe.** Keys and values are `Vec<u8>` end to end, so
a value may hold spaces, newlines, NUL bytes, or arbitrary binary. The
old protocol could not represent any of those: a newline ended the
command, and a space ended an argument.

**Collection members may contain spaces.** `LPUSH`, `SADD`, and `ZADD`
used to require single-token values, because that was the only way to
tell multiple arguments apart. RESP length-prefixes every argument, so
the restriction is gone.

**Replies are never silently truncated.** The old write path capped a
reply at 64 KiB and dropped the overflow with no error, so a `SMEMBERS`
over a large set returned a partial answer that looked complete. Now a
reply that outgrows `client-output-buffer-limit` closes the connection
with an error instead.

It also removed the `SET` flag ambiguity. With no argument boundaries,
the old parser had to guess where a value ended and its options began;
now `SET key value NX EX 30` parses exactly as Redis documents it.

## How it works

`src/resp.rs` holds both halves.

**Requests.** A connection's read buffer is handed to `parse_request`
until it stops yielding whole commands. A buffer starting with `*` is a
RESP array of bulk strings, which is what client libraries send.
Anything else is an *inline command*: a bare line, split on whitespace
with shell-style quoting. Redis accepts both, and the inline form is
what keeps `nc` and `telnet` usable. A partial frame consumes nothing
and waits for more bytes, so a request split across TCP segments
reassembles correctly.

Lines may end with `\r\n` or a bare `\n`, as Redis's do. That
leniency is what makes `echo PING | nc host 7171` work: a shell sends no
carriage return.

**Replies.** Commands return a `Reply`, which describes *what* the
answer is rather than how it is spelled: `Integer`, `Bulk`, `Nil`,
`Array`, `Map`, `Set`, `Double`, `ScoredMembers`. The encoder turns that
into the right bytes for the connection's negotiated protocol version.
Nothing writes to a socket from inside a command handler, which is why a
command's result can be tested as a value.

## RESP2 and RESP3

A connection starts at RESP2 and switches when a client sends `HELLO 3`.
`HELLO 4` is refused with `NOPROTO`, the reply clients are built to
downgrade on.

The typed aggregates RESP3 adds each have a RESP2 spelling, and the
encoder falls back automatically:

| Reply | RESP2 | RESP3 |
|---|---|---|
| `Map` (`HGETALL`, `CONFIG GET`, `HELLO`) | flat array of alternating keys and values | `%` map |
| `Set` (`SMEMBERS`, `SPOP` with count, `SINTER`/`SUNION`/`SDIFF`) | `*` array | `~` set |
| `Double` (`ZSCORE`, `ZINCRBY`, `ZMSCORE`) | bulk string | `,` double |
| `Nil` | `$-1` null bulk | `_` null |
| `ScoredMembers` (`WITHSCORES`, `ZPOPMIN`/`ZPOPMAX` with count) | flat member, score, member, score | array of `[member, score]` pairs |

There is deliberately no boolean variant. Every Klyro command whose
answer reads as true/false is one Redis answers with an integer 0 or 1
in both versions, so adding `#t`/`#f` would have diverged from Redis
rather than matched it.

## Reply types match Redis

This is the part that makes stock clients work, and the part that is
easy to get subtly wrong. A client decides how to decode a reply from
the command it sent, so `LLEN` must be an integer and not a bulk string,
`GET` on a missing key must be null and not an empty bulk, and `LPOP key
count` on a missing key must be a *null* array rather than an empty one.
All 107 commands were gone through against Redis's documented reply
type, and the redis-py, go-redis, and ioredis sweeps check the ones that
matter in practice.

Error strings match too, since clients pattern-match on the first word:
`WRONGTYPE ...`, `ERR value is not an integer or out of range`,
`ERR no such key`, `ERR syntax error`, and
`ERR wrong number of arguments for '<command>' command`.

## The dump format changed with it

A line-oriented dump file cannot hold a value containing a newline, so
the format is now length-prefixed and versioned `KLYRO-DUMP 2`. Every
blob is written as its length, then exactly that many bytes.

Version 1 dumps still load, so an existing dump survives the upgrade;
they are rewritten as version 2 on the next save. The version 1 reader
also honours the old `EXPIREAT` lines.

## Compatibility notes

The protocol change is not backwards compatible, and nothing tried to
pretend otherwise:

- **Reply shapes all changed.** `SET` answers `+OK` instead of `OK`,
  `DEL` answers an integer instead of `OK`/`NOT_FOUND`, and the
  `END`-terminated multi-line replies are now arrays. The arity-based
  reply shapes that kept the old clients working (`DEL` answering
  `OK` for one key and `DELETED n` for several) are gone; every one of
  these commands now returns a count, as Redis does.
- **`TYPE` is lowercase**, matching Redis: `string`, not `STRING`.
- **`SETRANGE` pads with NUL bytes**, not spaces. The old behaviour was
  a workaround for a text protocol that could not carry a NUL.
- **`max-string-bytes` now defaults to 512 MB** rather than 64 KiB,
  which was a limit inherited from the old line length.
- **Scores print in shortest round-trip form.** `ZSCORE` on a score of
  100 returns `100`, not `100.000000`.

## What is still missing

RESP3's push messages are unimplemented, because Klyro has no pub/sub
or client-side caching to push. `RESET` and `CLIENT` are absent. The
event loop still parks nothing, so the blocking commands cannot be built
until it can - see the gap analysis.
