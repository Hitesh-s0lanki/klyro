# Configuration and observability

How to configure a Klyro server and see what it is doing. See
[../klyro.conf.sample](../klyro.conf.sample) for a commented file
listing every parameter at its default, and
[../README.md](../README.md) for the command reference.

## Where settings come from

Three sources, applied in this order, so each one overrides the last:

1. **Defaults**, in `Config::default` in [../src/config.rs](../src/config.rs).
2. **A config file**, if one is named on the command line.
3. **Command-line arguments**, which win over the file.

```sh
klyro                                  # every default
klyro 7200                             # a port
klyro 7200 /var/lib/klyro/klyro.dump   # a port and a dump path
klyro klyro.conf                       # a config file
klyro --config klyro.conf              # the same, spelled out
klyro --config klyro.conf 7200         # a file, with the port overridden
```

A non-numeric first argument is read as a config file path, which is
what keeps the original `klyro [port] [dump-file]` form working
unchanged.

The file format is one `name value` pair per line. Everything after a
`#` is a comment; blank lines are ignored. A problem in the file stops
startup, and **every** problem is reported rather than only the first:

```
$ klyro broken.conf
broken.conf:
  line 1: invalid value for parameter `maxclients`
  line 2: unknown parameter `nonsense`
```

## Parameters

| Name | Default | Changeable at runtime | What it does |
|---|---|---|---|
| `bind` | `0.0.0.0` | no | Listener address |
| `port` | `7171` | no | Listener port |
| `dbfilename` | `klyro.dump` | yes | Snapshot path; a change redirects the *next* save |
| `save-interval` | `60` | yes | Seconds between autosave checks |
| `sweep-interval` | `1000` | yes | Milliseconds between active expired-key sweeps |
| `maxclients` | `10000` | yes | Connection ceiling |
| `max-string-bytes` | `536870912` | yes | Largest value `APPEND`/`SETRANGE` will produce |
| `proto-max-bulk-len` | `536870912` | yes | Largest bulk string a client may send |
| `client-output-buffer-limit` | `268435456` | yes | Unsent reply allowed to pile up before the connection is closed |
| `scan-default-count` | `10` | yes | The `COUNT` a `SCAN` uses when not given one |
| `zadd-max-pairs` | `128` | yes | Most score/member pairs in one `ZADD` |

`bind` and `port` are fixed because the listening socket is already
bound by the time a client could ask; `CONFIG SET` on either replies
`ERR parameter cannot be changed at runtime`.

## CONFIG

```
CONFIG GET *                        every parameter
CONFIG GET save*                    glob-matched, same syntax as KEYS
CONFIG GET maxclients dbfilename    several at once
CONFIG SET maxclients 128
CONFIG RESETSTAT                    clears INFO's activity counters
```

`CONFIG GET` replies with a map of parameter to value, and accepts
several patterns at once. Parameter names and the subcommand are both
case-insensitive. A refused `CONFIG SET` says which
of the three reasons applies: unknown parameter, immutable parameter, or
invalid value. Every numeric parameter is a size or an interval, so zero
and negative values are rejected.

Changes take effect immediately, with one deliberate exception:
`dbfilename` redirects the next save and leaves the existing file alone.

## INFO

`INFO` prints every section; `INFO <section>` prints one. Sections are
`server`, `clients`, `memory`, `persistence`, `stats`, and `keyspace`,
plus `all`/`default` as synonyms for everything. An unknown section
returns an empty body rather than an error, as Redis does.

The reply is a single bulk string of `# Section` headers over
`key:value` lines, which is the shape client libraries parse into a
dictionary - `r.info()` in redis-py returns a dict straight off it.

```
# Server
klyro_version:0.1.0
redis_version:7.0.0
process_id:19357
tcp_bind:0.0.0.0
tcp_port:7171
uptime_in_seconds:412
uptime_in_days:0

# Clients
connected_clients:1
maxclients:10000
rejected_connections:0

# Memory
used_memory:5235
used_memory_human:5.11K
used_memory_peak:5377
used_memory_peak_human:5.25K

# Persistence
dbfilename:klyro.dump
changes_since_last_save:9
last_save_time:1789030422
last_save_status:ok
total_saves:1
save_interval_seconds:60

# Stats
total_connections_received:1
total_commands_processed:14
keyspace_hits:4
keyspace_misses:3
expired_keys:0

# Keyspace
db0:keys=8,expires=1
string:4
list:1
hash:1
set:1
zset:1
```

### What the numbers actually mean

**Memory is real, not estimated.** Klyro installs a counting global
allocator (see [../src/util/memory.rs](../src/util/memory.rs)) that keeps
a running byte total, so `used_memory` is the process's live allocation
total rather than a walk of the keyspace. The cost is one relaxed atomic
add and one subtract per allocation. Note it covers the whole process,
connection buffers included, not the keyspace alone.

**The hit ratio counts read commands only.** `keyspace_hits` and
`keyspace_misses` move for the commands listed in `READ_COMMANDS` in
[../src/commands/mod.rs](../src/commands/mod.rs) and nothing else, so a
write's internal lookup never lands in the ratio. The dispatcher measures
the delta in the store's lookup counters across a single command, which
keeps the accounting in one place instead of spread across 107 handlers.
Internal type checks deliberately use a non-counting lookup, otherwise
every read would register as two.

**`redis_version` is a compatibility shim.** Client libraries gate
command availability on it, so INFO reports the Redis release whose
command shapes Klyro implements. It is not a claim to be that server;
`klyro_version` sits right above it.

**The keyspace section is the one O(n) part of INFO.** It walks the
keyspace to count keys, keys with a TTL, and the per-type breakdown.
Everything else is O(1).

**`CONFIG RESETSTAT` clears activity, not state.** Connection totals,
command totals, and the hit ratio go back to zero. Uptime, the current
client count, and the save record stay, because they describe the server
now rather than what has accumulated.

## Limits that are enforced

`maxclients` is the one parameter that turns clients away. Past the
ceiling, a new connection is told so and closed, rather than being
dropped silently:

```
$ nc localhost 7171
-ERR max number of clients reached
```

`INFO clients` counts how often that has happened in
`rejected_connections`.

`client-output-buffer-limit` is the other one that closes a connection:
a reply that outgrows it ends the connection with an error, which is
what replaced the old protocol's silent 64 KiB truncation.

## What is still missing

- No `CONFIG REWRITE`, so a runtime change is not written back to the
  config file and does not survive a restart.
- No `maxmemory` and no eviction policy. `INFO memory` reports usage, but
  nothing acts on it.
- No `CLIENT LIST`/`CLIENT KILL`, no `SLOWLOG`, no `LATENCY`, no
  `COMMAND`, no `MONITOR`, and no per-command statistics.
- No logging beyond the startup and shutdown lines, and no `loglevel`.

See [redis-feature-gap.md](redis-feature-gap.md) for the full comparison
against Redis.
