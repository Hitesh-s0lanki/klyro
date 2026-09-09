# klyro-client

Official Node.js/TypeScript client for [Klyro](../../README.md), a
Redis-style in-memory data server. Promise-based, backed by
`net.Socket`, zero runtime dependencies.

This package is not published to npm. Use it locally, either from
within this repo or from another project via a relative/`file:`
dependency.

## Install

From within this directory:

```sh
npm install
npm run build
```

This compiles `src/` (TypeScript, strict mode) to `dist/`, which is
what `main`/`types` in `package.json` point at.

From another project, referencing it by path:

```sh
npm install file:../own-database/clients/node
```

(adjust the path to wherever this repo is checked out — `npm install`
will run the package's own `build` step is *not* automatic, so run
`npm run build` inside `clients/node` at least once first, or add a
`prepare` script if you want it built automatically on install).

## Quick start

```ts
import { KlyroClient, KlyroError, WrongTypeError } from "klyro-client";

const client = new KlyroClient({ host: "127.0.0.1", port: 7171 });
await client.connect();

// Strings
await client.set("greeting", "hello world");
console.log(await client.get("greeting")); // "hello world"

// Lists
await client.rpush("queue", "a", "b", "c");
console.log(await client.lrange("queue", 0, -1)); // ["a", "b", "c"]

// Hashes
await client.hset("user:1", "name", "Alice");
console.log(await client.hgetall("user:1")); // { name: "Alice" }

// Sets
await client.sadd("tags", "fast", "reliable");
console.log(await client.smembers("tags")); // Set { "fast", "reliable" }

// Sorted sets - pairs are given as [score, member] tuples
await client.zadd("leaderboard", [100, "alice"], [50, "bob"]);
console.log(await client.zrange("leaderboard", 0, -1)); // [["bob", 50], ["alice", 100]]

await client.close();
```

## API

One async method per server command, camelCased (e.g. `LPUSH` ->
`lpush`, `HGETALL` -> `hgetall`). A few naming notes:

- `TYPE` -> `typeOf(key)` (`type` reads oddly as a method name in
  JS/TS), resolving `null` for the server's `NONE` (missing key)
  instead of a string.
- Commands that reply `OK`/`NOT_FOUND` as a boolean-ish outcome -
  `del`, `expire`, `hdel`, `srem`, `zrem` - resolve to `boolean`
  (`true` for `OK`, `false` for `NOT_FOUND`) rather than throwing.
- `zadd(key, ...pairs)` takes `[score, member]` tuples, matching the
  wire order (`score member`).
- `scan(cursor, { match?, count? })` resolves `{ keys, cursor }`; call
  again with the returned `cursor` until it comes back `0`.

`connect()` and `close()` are explicit and async - the constructor
does not open a socket. Calls made concurrently on one client are
queued and sent one at a time, since the protocol has no request id to
match out-of-order replies to their calls.

## Error handling

Any `ERR ...` reply from the server throws a `KlyroError` (its
`message` is the raw server reply text). A command run against a key
holding the wrong data type (`ERR WRONGTYPE ...`) throws the more
specific `WrongTypeError`, a subclass of `KlyroError`:

```ts
import { KlyroError, WrongTypeError } from "klyro-client";

try {
  await client.set("mylist", "oops"); // mylist is actually a List
} catch (err) {
  if (err instanceof WrongTypeError) {
    // key exists with a different type
  } else if (err instanceof KlyroError) {
    // some other server-side usage error
  } else {
    throw err;
  }
}
```

Client-side argument validation (e.g. a key containing whitespace, or
a `SET` value containing a newline) throws a plain `TypeError` before
anything is sent over the wire - the protocol has no escaping
mechanism, so these can't be sent as-is.

## Tests

```sh
npm test
```

Builds the package, then runs `tests/client.test.ts` (compiled to
`dist/tests/`) via Node's built-in test runner (`node --test`) against
a real `klyro` server subprocess - no mocking. Each test starts its
own server on a unique port with a temp dump file, and cleans both up
afterward. If `target/release/klyro` doesn't exist yet, the test setup
runs `cargo build --release` from the repo root first.
