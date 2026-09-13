# klyro-db

**The high-performance in-memory data server**, installable from npm.

Klyro is an in-memory, Redis-style data server written in Rust, with
String, List, Hash, Set, and Sorted Set types - plus **Memory**, a
retrieval structure for AI agents that indexes text and embeddings
together and ranks by keyword relevance, semantic similarity, recency,
and importance in one query.

It speaks RESP, the Redis wire protocol, so **any Redis client library
works**: ioredis, redis-py, go-redis, and `redis-cli` all connect with
no adapter.

## Run it

```sh
npx klyro-db                      # port 7171, dump file klyro.dump
npx klyro-db 7200                 # a different port
npx klyro-db klyro.conf           # a config file
```

Or install it, which puts a `klyro` command on your PATH:

```sh
npm install -g klyro-db
klyro 7200 data.dump
```

## JavaScript and TypeScript

Install the package in your application with `npm install klyro-db`.
With the server running, import the typed client:

```ts
import { createClient, type KlyroClientOptions } from "klyro-db";

const options: KlyroClientOptions = { host: "127.0.0.1", port: 7171 };
const r = createClient(options);
await r.set("greeting", "hello");
const greeting: string | null = await r.get("greeting");
console.log(greeting);
await r.quit();
```

Declarations are bundled; no `@types/klyro-db` package is needed.
Node.js TypeScript projects should include `@types/node` in their dev dependencies.
JavaScript can use the same API without the type annotations, or use
`const { createClient } = require("klyro-db")`.

`createClient()` connects to `127.0.0.1:7171` by default. It returns an
ioredis client and accepts ioredis connection options. Use `lazyConnect: true`
to defer connecting until `await r.connect()`. Importing the package does
not start the database server. This API runs in Node.js, not in browsers.

The client exposes ioredis command types; commands that Klyro does not
implement still return server errors. All 15 implemented `MEM.*` commands have typed helpers on `r.memory`.
Raw commands remain available through `r.call(...)`.

## What this package contains

A CLI launcher, a client API, and TypeScript declarations. The server is a native binary, published as one
package per platform - `klyro-db-darwin-arm64` and its siblings - and
npm installs the single one that matches your machine. There is no
compiler involved and no `postinstall` download.

Builds exist for macOS and Linux on x64 and arm64. Klyro's event loop is
`poll(2)` and it saves its dump out of a POSIX signal handler, so there
is no Windows build; use [the Docker
image](https://github.com/Hitesh-s0lanki/klyro#run-with-docker) or WSL.

## Data

On startup Klyro loads the dump file if it exists, and writes it back on
a graceful shutdown (`SHUTDOWN`, `SIGINT`, or `SIGTERM`), on `SAVE`, and
every 60 seconds if anything changed. `SIGKILL` or a crash loses
whatever changed since the last save.

## Documentation

Everything - the command reference, the configuration parameters, the
memory commands - is in [the repository](https://github.com/Hitesh-s0lanki/klyro).

MIT licensed.

## Typed memory commands

```ts
import { createClient, type MemoryHit } from "klyro-db";

const db = createClient();
await db.memory.create("notes", { mode: "HYBRID", dim: 2 });
const id = await db.memory.add("notes", {
  text: "User prefers PostgreSQL",
  vector: new Float32Array([1, 0]),
  meta: { kind: "preference" },
  importance: 0.8,
});
const hits: MemoryHit[] = await db.memory.query("notes", {
  text: "PostgreSQL",
  vector: [1, 0],
  fusion: "RRF",
  topK: 5,
  filters: [{ field: "kind", op: "EQ", value: "preference" }],
  withMeta: true,
  withScores: true,
});
console.log(hits[0]?.meta?.get("kind"));
await db.quit();
```

| Helper | Server command |
| --- | --- |
| `memory.create(key, options)` | `MEM.CREATE` |
| `memory.info(key)` | `MEM.INFO` |
| `memory.config(key, options)` | `MEM.CONFIG` |
| `memory.card(key)` | `MEM.CARD` |
| `memory.add(key, options)` | `MEM.ADD` |
| `memory.get(key, id, options?)` | `MEM.GET` |
| `memory.mget(key, id, ...ids)` | `MEM.MGET` |
| `memory.del(key, id, ...ids)` | `MEM.DEL` |
| `memory.setMeta(key, id, metadata)` | `MEM.SETMETA` |
| `memory.delMeta(key, id, field, ...fields)` | `MEM.DELMETA` |
| `memory.expire(key, id, seconds)` | `MEM.EXPIRE` |
| `memory.scan(key, cursor, options?)` | `MEM.SCAN` |
| `memory.search(key, text, options?)` | `MEM.SEARCH` |
| `memory.vsearch(key, vector, options?)` | `MEM.VSEARCH` |
| `memory.query(key, options)` | `MEM.QUERY` |

The declarations describe every helper's inputs and decoded replies.
Record text is omitted with `noText`; metadata, vectors and component scores
are optional unless requested. Metadata is returned as a `Map`.
`created_at`, `updated_at` and `pttl` use milliseconds; TTL inputs and
`halflife` use seconds. `expire(..., 0)` clears the record deadline.
`scan` returns `{ cursor, ids }`; continue until the cursor is `"0"`.

Vector inputs accept number arrays, Float32Array, or little-endian float32
Buffers. Returned vectors are Float32Array (normalized for cosine indexes),
or null when requested for a record with no vector.

Use `db.memoryBuffer` for the same helpers with lossless Buffer IDs, text,
and metadata keys/values. Ordinary `db.memory` decodes those fields as UTF-8.
Both APIs propagate server errors, including failed NX/XX conditions.
Memory helpers execute individual commands; raw MEM commands in ioredis
pipelines use the underlying raw reply shapes.
