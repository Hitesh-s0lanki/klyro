import assert from "node:assert/strict";
import { test } from "node:test";
import { KlyroClient, KlyroError, WrongTypeError } from "../src/index.js";
import { startServer, type TestServer } from "./support.js";

async function withClient(fn: (client: KlyroClient, server: TestServer) => Promise<void>): Promise<void> {
  const server = await startServer();
  const client = new KlyroClient({ port: server.port, timeoutMs: 2000 });
  await client.connect();
  try {
    await fn(client, server);
  } finally {
    await client.close().catch(() => {});
    await server.stop();
  }
}

test("generic: PING/DEL/EXPIRE/TTL/TYPE/KEYS/DBSIZE/SAVE", async () => {
  await withClient(async (client) => {
    await client.ping(); // resolves without throwing

    assert.equal(await client.typeOf("missing"), null);
    assert.equal(await client.ttl("missing"), -2);
    assert.equal(await client.del("missing"), false);

    await client.set("k1", "v1");
    assert.equal(await client.typeOf("k1"), "STRING");
    assert.equal(await client.ttl("k1"), -1);

    assert.equal(await client.expire("k1", 100), true);
    const ttl1 = await client.ttl("k1");
    assert.ok(ttl1 > 0 && ttl1 <= 100, `expected ttl in (0, 100], got ${ttl1}`);
    assert.equal(await client.expire("missing", 10), false);

    await client.set("k2", "v2");
    assert.deepEqual((await client.keys()).sort(), ["k1", "k2"]);
    assert.equal(await client.dbsize(), 2);

    assert.equal(await client.del("k1"), true);
    assert.equal(await client.del("k1"), false);
    assert.equal(await client.dbsize(), 1);

    await client.save(); // resolves without throwing
  });
});

test("generic: KEYS glob pattern matching", async () => {
  await withClient(async (client) => {
    await client.set("foo", "1");
    await client.set("foobar", "2");
    await client.set("baz", "3");

    assert.deepEqual((await client.keys("foo*")).sort(), ["foo", "foobar"]);
    assert.deepEqual(await client.keys("ba?"), ["baz"]);
    assert.deepEqual(await client.keys("nomatch*"), []);
  });
});

test("generic: a key that looks like an error line is not misread", async () => {
  await withClient(async (client) => {
    // A key like "ERRlog" starts with "ERR" but has no space after it -
    // unlike a real "ERR <message>" reply line - so it must come back
    // as ordinary data, not be thrown as a KlyroError.
    await client.set("ERRlog", "x");
    assert.deepEqual(await client.keys("ERRlog"), ["ERRlog"]);
  });
});

test("generic: SCAN cursor pagination covers the whole keyspace", async () => {
  await withClient(async (client) => {
    const expected = new Set<string>();
    for (let i = 0; i < 25; i++) {
      const key = `scankey${i}`;
      expected.add(key);
      await client.set(key, String(i));
    }

    let cursor = 0;
    const seen = new Set<string>();
    let iterations = 0;
    do {
      const result = await client.scan(cursor, { count: 5 });
      for (const k of result.keys) seen.add(k);
      cursor = result.cursor;
      iterations++;
      assert.ok(iterations < 1000, "SCAN did not terminate");
    } while (cursor !== 0);

    assert.deepEqual(seen, expected);
  });
});

test("string: SET/GET/INCR/DECR/APPEND/GETRANGE/SETRANGE", async () => {
  await withClient(async (client) => {
    assert.equal(await client.get("s"), null);
    await client.set("s", "hello world");
    assert.equal(await client.get("s"), "hello world");

    assert.equal(await client.incr("counter"), 1);
    assert.equal(await client.incr("counter"), 2);
    assert.equal(await client.decr("counter"), 1);

    await assert.rejects(
      () => client.incr("s"),
      (err: unknown) => {
        assert.ok(err instanceof KlyroError);
        assert.ok(!(err instanceof WrongTypeError));
        assert.match((err as Error).message, /not an integer/);
        return true;
      },
    );

    assert.equal(await client.append("s", "!!!"), "hello world!!!".length);
    assert.equal(await client.get("s"), "hello world!!!");
    assert.equal(await client.append("newstr", "abc"), 3);

    assert.equal(await client.getrange("s", 0, 4), "hello");
    assert.equal(await client.getrange("s", -3, -1), "!!!");
    assert.equal(await client.getrange("s", 1000, 2000), "");

    const len = await client.setrange("s", 6, "there");
    assert.equal(len, "hello there!!!".length);
    assert.equal(await client.get("s"), "hello there!!!");

    const padLen = await client.setrange("padded", 3, "xyz");
    assert.equal(padLen, 6);
    assert.equal(await client.get("padded"), "   xyz");
  });
});

test("list: LPUSH/RPUSH/LPOP/RPOP/LLEN/LRANGE, multi-value push, empty-delete", async () => {
  await withClient(async (client) => {
    assert.equal(await client.lpush("l", "a", "b", "c"), 3);
    assert.deepEqual(await client.lrange("l", 0, -1), ["c", "b", "a"]);

    await client.del("l");
    assert.equal(await client.rpush("l", "a", "b", "c"), 3);
    assert.deepEqual(await client.lrange("l", 0, -1), ["a", "b", "c"]);

    assert.equal(await client.llen("l"), 3);
    assert.equal(await client.lpop("l"), "a");
    assert.equal(await client.rpop("l"), "c");
    assert.equal(await client.llen("l"), 1);
    assert.equal(await client.lpop("l"), "b");

    // list is now empty -> key deleted
    assert.equal(await client.lpop("l"), null);
    assert.equal(await client.typeOf("l"), null);

    await assert.rejects(() => client.lpush("l"), TypeError);
  });
});

test("hash: HSET/HGET/HDEL/HLEN/HGETALL", async () => {
  await withClient(async (client) => {
    await client.hset("h", "name", "Alice");
    await client.hset("h", "role", "admin user"); // value w/ spaces
    assert.equal(await client.hget("h", "name"), "Alice");
    assert.equal(await client.hget("h", "role"), "admin user");
    assert.equal(await client.hget("h", "missing"), null);
    assert.equal(await client.hlen("h"), 2);
    assert.deepEqual(await client.hgetall("h"), { name: "Alice", role: "admin user" });

    assert.equal(await client.hdel("h", "role"), true);
    assert.equal(await client.hdel("h", "role"), false);
    assert.equal(await client.hlen("h"), 1);

    assert.equal(await client.hdel("h", "name"), true);
    assert.equal(await client.typeOf("h"), null); // emptied -> deleted
  });
});

test("set: SADD/SREM/SISMEMBER/SCARD/SMEMBERS, multi-value add", async () => {
  await withClient(async (client) => {
    assert.equal(await client.sadd("s", "a", "b", "c"), 3);
    assert.equal(await client.sadd("s", "b", "d"), 1); // "b" is a duplicate
    assert.equal(await client.scard("s"), 4);
    assert.deepEqual(await client.smembers("s"), new Set(["a", "b", "c", "d"]));
    assert.equal(await client.sismember("s", "a"), true);
    assert.equal(await client.sismember("s", "zzz"), false);

    assert.equal(await client.srem("s", "a"), true);
    assert.equal(await client.srem("s", "a"), false);
    assert.equal(await client.scard("s"), 3);
  });
});

test("zset: ZADD/ZSCORE/ZREM/ZCARD/ZRANGE, multi-pair add", async () => {
  await withClient(async (client) => {
    assert.equal(await client.zadd("z", [100, "alice"], [50, "bob"], [75, "carol"]), 3);
    assert.equal(await client.zadd("z", [200, "alice"]), 0); // repositioning doesn't count

    assert.equal(await client.zscore("z", "alice"), 200);
    assert.equal(await client.zscore("z", "missing"), null);
    assert.equal(await client.zcard("z"), 3);

    assert.deepEqual(await client.zrange("z", 0, -1), [
      ["bob", 50],
      ["carol", 75],
      ["alice", 200],
    ]);

    assert.equal(await client.zrem("z", "bob"), true);
    assert.equal(await client.zrem("z", "bob"), false);
    assert.equal(await client.zcard("z"), 2);
  });
});

test("WRONGTYPE errors are thrown as WrongTypeError", async () => {
  await withClient(async (client) => {
    await client.set("str", "value");
    await assert.rejects(
      () => client.lpush("str", "x"),
      (err: unknown) => {
        assert.ok(err instanceof WrongTypeError);
        assert.ok(err instanceof KlyroError);
        assert.match((err as Error).message, /^ERR WRONGTYPE/);
        return true;
      },
    );

    await client.sadd("myset", "a");
    await assert.rejects(() => client.get("myset"), WrongTypeError);
    await assert.rejects(() => client.hget("myset", "f"), WrongTypeError);
    await assert.rejects(() => client.zrange("myset", 0, -1), WrongTypeError);
  });
});

test("non-WRONGTYPE ERR replies map to KlyroError (not WrongTypeError)", async () => {
  await withClient(async (client) => {
    await client.set("notnum", "abc");
    await assert.rejects(
      () => client.incr("notnum"),
      (err: unknown) => {
        assert.ok(err instanceof KlyroError);
        assert.ok(!(err instanceof WrongTypeError));
        return true;
      },
    );

    const tooMany: Array<[number, string]> = Array.from({ length: 129 }, (_, i) => [i, `m${i}`]);
    await assert.rejects(
      () => client.zadd("bigz", ...tooMany),
      (err: unknown) => {
        assert.ok(err instanceof KlyroError);
        assert.match((err as Error).message, /too many score\/member pairs/);
        return true;
      },
    );
  });
});

test("client-side validation rejects whitespace in tokens and newlines in values", async () => {
  await withClient(async (client) => {
    await assert.rejects(() => client.set("bad key", "v"), TypeError);
    await assert.rejects(() => client.set("key", "line1\nline2"), TypeError);
    await assert.rejects(() => client.sadd("s", "has space"), TypeError);
    await assert.rejects(() => client.zadd("z", [1, "has space"]), TypeError);
    await assert.rejects(() => client.get(""), TypeError);
  });
});

test("close() ends the connection; commands before connect() or after close() throw", async () => {
  const server = await startServer();
  try {
    const client = new KlyroClient({ port: server.port, timeoutMs: 2000 });
    await assert.rejects(() => client.get("x"), /not connected/);

    await client.connect();
    await client.set("x", "1");
    await client.close();
    await assert.rejects(() => client.get("x"), /not connected|closed/);

    await client.close(); // idempotent
  } finally {
    await server.stop();
  }
});

test("quit() closes the connection and shutdown() stops the server", async () => {
  const server = await startServer();
  try {
    const client = new KlyroClient({ port: server.port, timeoutMs: 2000 });
    await client.connect();
    await client.set("k", "v");
    await client.quit();
    await assert.rejects(() => client.ping(), /not connected/);

    const client2 = new KlyroClient({ port: server.port, timeoutMs: 2000 });
    await client2.connect();
    await client2.shutdown();
    await server.waitForExit(3000);
  } finally {
    await server.stop();
  }
});
