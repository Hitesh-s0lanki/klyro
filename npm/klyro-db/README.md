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

Then talk to it with the Redis client you already use:

```js
import Redis from "ioredis";

const r = new Redis({ host: "localhost", port: 7171 });
await r.set("greeting", "hello");
console.log(await r.get("greeting"));
```

## What this package contains

Nothing but a launcher. The server is a native binary, published as one
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
