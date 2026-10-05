import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeTabs, type CodeTab } from "@/components/ui/CodeTabs";

const examples: Record<string, CodeTab[]> = {
  generic: [
    { label: "TypeScript", lang: "ts", code: `const exists = await db.exists("session:42");
await db.expire("session:42", 900);
const ttl = await db.ttl("session:42");` },
    { label: "Python", lang: "python", code: `exists = db.exists("session:42")
db.expire("session:42", 900)
ttl = db.ttl("session:42")` },
    { label: "Go", lang: "go", code: `exists, err := db.Exists(ctx, "session:42").Result()
if err != nil { log.Fatal(err) }
if err := db.Expire(ctx, "session:42", 15*time.Minute).Err(); err != nil { log.Fatal(err) }
ttl, err := db.TTL(ctx, "session:42").Result()` },
    { label: "redis-cli", lang: "resp", code: `EXISTS session:42
EXPIRE session:42 900
TTL session:42` },
  ],
  strings: [
    { label: "TypeScript", lang: "ts", code: `await db.set("session:42", "active", "EX", 900);
await db.incr("metrics:requests");
const status = await db.get("session:42");` },
    { label: "Python", lang: "python", code: `db.set("session:42", "active", ex=900)
db.incr("metrics:requests")
status = db.get("session:42")` },
    { label: "Go", lang: "go", code: `if err := db.Set(ctx, "session:42", "active", 15*time.Minute).Err(); err != nil { log.Fatal(err) }
requests, err := db.Incr(ctx, "metrics:requests").Result()
status, err := db.Get(ctx, "session:42").Result()` },
    { label: "redis-cli", lang: "resp", code: `SET session:42 active EX 900
INCR metrics:requests
GET session:42` },
  ],
  lists: [
    { label: "TypeScript", lang: "ts", code: `await db.lpush("jobs", "generate-report");
const job = await db.brpop("jobs", 5);` },
    { label: "Python", lang: "python", code: `db.lpush("jobs", "generate-report")
job = db.brpop("jobs", timeout=5)` },
    { label: "Go", lang: "go", code: `if err := db.LPush(ctx, "jobs", "generate-report").Err(); err != nil { log.Fatal(err) }
job, err := db.BRPop(ctx, 5*time.Second, "jobs").Result()` },
    { label: "redis-cli", lang: "resp", code: `LPUSH jobs generate-report
BRPOP jobs 5` },
  ],
  hashes: [
    { label: "TypeScript", lang: "ts", code: `await db.hset("user:42", { name: "Ari", plan: "pro" });
const profile = await db.hgetall("user:42");` },
    { label: "Python", lang: "python", code: `db.hset("user:42", mapping={"name": "Ari", "plan": "pro"})
profile = db.hgetall("user:42")` },
    { label: "Go", lang: "go", code: `if err := db.HSet(ctx, "user:42", "name", "Ari", "plan", "pro").Err(); err != nil { log.Fatal(err) }
profile, err := db.HGetAll(ctx, "user:42").Result()` },
    { label: "redis-cli", lang: "resp", code: `HSET user:42 name Ari plan pro
HGETALL user:42` },
  ],
  sets: [
    { label: "TypeScript", lang: "ts", code: `await db.sadd("online-users", "42", "73");
const online = await db.sismember("online-users", "42");
const users = await db.smembers("online-users");` },
    { label: "Python", lang: "python", code: `db.sadd("online-users", "42", "73")
online = db.sismember("online-users", "42")
users = db.smembers("online-users")` },
    { label: "Go", lang: "go", code: `if err := db.SAdd(ctx, "online-users", "42", "73").Err(); err != nil { log.Fatal(err) }
online, err := db.SIsMember(ctx, "online-users", "42").Result()
users, err := db.SMembers(ctx, "online-users").Result()` },
    { label: "redis-cli", lang: "resp", code: `SADD online-users 42 73
SISMEMBER online-users 42
SMEMBERS online-users` },
  ],
  zsets: [
    { label: "TypeScript", lang: "ts", code: `await db.zadd("leaderboard", 980, "user:42", 860, "user:73");
const leaders = await db.zrevrange("leaderboard", 0, 9, "WITHSCORES");` },
    { label: "Python", lang: "python", code: `db.zadd("leaderboard", {"user:42": 980, "user:73": 860})
leaders = db.zrevrange("leaderboard", 0, 9, withscores=True)` },
    { label: "Go", lang: "go", code: `if err := db.ZAdd(ctx, "leaderboard", redis.Z{Score: 980, Member: "user:42"}, redis.Z{Score: 860, Member: "user:73"}).Err(); err != nil { log.Fatal(err) }
leaders, err := db.ZRevRangeWithScores(ctx, "leaderboard", 0, 9).Result()` },
    { label: "redis-cli", lang: "resp", code: `ZADD leaderboard 980 user:42 860 user:73
ZREVRANGE leaderboard 0 9 WITHSCORES` },
  ],
};

export const metadata: Metadata = {
  title: "Data type commands",
  description: "The 130 Redis-shaped commands Klyro implements across data structures, transactions, pub/sub, blocking operations, and the generic keyspace.",
};

export default function DataTypesPage() {
  return (
    <>
      <DocHeader
        eyebrow="Reference"
        title="Data type commands"
        lead="Klyro implements 145 commands: 130 Redis-shaped commands plus the 15-command MEM.* family. Matching RESP reply shapes let standard client libraries decode the results."
      />

      <p>
        Choose a language once and every tabbed example in the documentation
        follows that choice. The preference is stored only in this browser.
      </p>
      <CodeTabs
        className="my-6"
        tabs={[
          { label: "TypeScript", lang: "ts", code: `import { createClient } from "klyro-db";

const db = createClient();
// Run commands, then close with: await db.quit();` },
          { label: "Python", lang: "python", code: `from klyro_db import Klyro

db = Klyro()
# Run commands, then close with: db.close()` },
          { label: "Go", lang: "go", code: `import (
  "context"
  "log"
  "time"

  klyro "github.com/Hitesh-s0lanki/klyro/go"
  "github.com/redis/go-redis/v9"
)

ctx := context.Background()
db := klyro.NewClient(nil)
defer db.Close()` },
          { label: "redis-cli", lang: "bash", code: `redis-cli -p 7171` },
        ]}
      />

      <h2 id="generic">Generic (any type)</h2>
      <CodeTabs tabs={examples.generic} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["PING [message]", "PONG, or the message"],
          ["ECHO message", "The message"],
          ["HELLO [protover]", "Server info; HELLO 3 switches to RESP3"],
          ["DEL key [key ...] / UNLINK key [key ...]", "Number of keys removed"],
          ["EXISTS key [key ...]", "How many exist; a repeated key counts each time"],
          ["EXPIRE key seconds / PEXPIRE key ms", "1 if the TTL was set, 0 if the key is missing"],
          ["EXPIREAT key unix-seconds / PEXPIREAT key unix-ms", "1 or 0"],
          ["PERSIST key", "1 if a TTL was removed, else 0"],
          ["TTL key / PTTL key", "Time left, -1 no expiry, -2 missing"],
          ["TYPE key", "string, list, hash, set, zset, memory, or none"],
          ["RENAME key newkey / RENAMENX key newkey", "OK, or 1/0 for the NX form"],
          ["COPY source destination [REPLACE]", "1 if copied, else 0"],
          ["RANDOMKEY", "A key, or nil if the keyspace is empty"],
          ["KEYS pattern", "Array of matching keys"],
          ["SCAN cursor [MATCH pattern] [COUNT count]", "[next-cursor, [keys...]]"],
          ["DBSIZE", "Number of live keys"],
          ["FLUSHDB / FLUSHALL", "OK; one keyspace, so both do the same thing"],
          ["INFO [section]", "One text blob of # Section headers over key:value lines"],
          ["CONFIG GET pattern [pattern ...]", "Map of parameter to value"],
          ["CONFIG SET parameter value", "OK, or an error explaining the refusal"],
          ["CONFIG RESETSTAT", "OK; clears INFO's activity counters"],
          ["SAVE", "OK; writes the dump file immediately"],
          ["QUIT / SHUTDOWN", "OK, then closes the connection or stops the server"],
        ]}
      />
      <p>
        <code>COPY</code> is a deep copy: mutating the destination afterwards
        leaves the source untouched. <code>RENAME</code> and <code>COPY</code>{" "}
        both carry the TTL across.
      </p>

      <h2 id="strings">Strings</h2>
      <CodeTabs tabs={examples.strings} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["SET key value [NX|XX] [GET] [EX s|PX ms|EXAT ts|PXAT ts|KEEPTTL]", "OK, or nil when NX/XX is not satisfied"],
          ["SETNX key value", "1 if written, else 0"],
          ["SETEX key seconds value / PSETEX key ms value", "OK"],
          ["GET key", "The value, or nil"],
          ["GETSET key value", "The previous value, or nil; clears any TTL"],
          ["GETDEL key", "The value, or nil; removes the key"],
          ["GETEX key [EX s|PX ms|PERSIST]", "The value, or nil; adjusts the TTL"],
          ["MGET key [key ...]", "Array of values, nil per missing key"],
          ["MSET key value [key value ...]", "OK"],
          ["MSETNX key value [key value ...]", "1 if all were written, 0 if any key existed"],
          ["INCR / DECR / INCRBY / DECRBY key [n]", "The new value"],
          ["INCRBYFLOAT key n", "The new value, as text"],
          ["APPEND key value", "The new length"],
          ["STRLEN key", "The length, 0 if missing"],
          ["GETRANGE key start end / SUBSTR key start end", "The substring; inclusive, negatives count from the end"],
          ["SETRANGE key offset value", "The new length; pads any gap with NUL bytes"],
        ]}
      />
      <Callout variant="tip" title="The lock primitive">
        <p>
          <code>SET key value NX EX 30</code> takes the key only if nobody holds
          it, and the lease expires on its own. <code>INCR</code>,{" "}
          <code>DECR</code>, <code>INCRBY</code>, <code>APPEND</code>, and{" "}
          <code>SETRANGE</code> keep an existing TTL; <code>SET</code> without{" "}
          <code>KEEPTTL</code>, <code>GETSET</code>, and <code>MSET</code> clear
          it.
        </p>
      </Callout>

      <h2 id="lists">Lists</h2>
      <CodeTabs tabs={examples.lists} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["LPUSH / RPUSH key value [value ...]", "The new length"],
          ["LPUSHX / RPUSHX key value [value ...]", "The new length, or 0 if the key does not exist"],
          ["LPOP / RPOP key [count]", "One value or nil; with a count, an array"],
          ["LLEN key", "The length"],
          ["LRANGE key start stop", "Array of values"],
          ["LINDEX key index", "The value, or nil"],
          ["LSET key index value", "OK, or an error if the key or index is out of range"],
          ["LINSERT key BEFORE|AFTER pivot value", "The new length, -1 if the pivot is absent, 0 if the key is"],
          ["LREM key count value", "How many were removed"],
          ["LTRIM key start stop", "OK; an empty range deletes the key"],
          ["RPOPLPUSH source destination", "The moved value, or nil"],
          ["LMOVE source destination LEFT|RIGHT LEFT|RIGHT", "The moved value, or nil"],
        ]}
      />

      <h2 id="hashes">Hashes</h2>
      <CodeTabs tabs={examples.hashes} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["HSET key field value [field value ...]", "Number of fields added"],
          ["HSETNX key field value", "1 if written, 0 if the field exists"],
          ["HMSET key field value [field value ...]", "OK"],
          ["HGET key field", "The value, or nil"],
          ["HMGET key field [field ...]", "Array of values, nil per missing field"],
          ["HDEL key field [field ...]", "Number of fields removed"],
          ["HLEN key / HSTRLEN key field", "The field count / the value's length"],
          ["HEXISTS key field", "1 or 0"],
          ["HKEYS key / HVALS key", "Array of fields, or of values"],
          ["HGETALL key", "Map of field to value"],
          ["HINCRBY key field n / HINCRBYFLOAT key field n", "The new value"],
        ]}
      />

      <h2 id="sets">Sets</h2>
      <CodeTabs tabs={examples.sets} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["SADD key member [member ...]", "Number of members newly added"],
          ["SREM key member [member ...]", "Number removed"],
          ["SISMEMBER key member", "1 or 0"],
          ["SMISMEMBER key member [member ...]", "Array of 1/0, one per member"],
          ["SCARD key / SMEMBERS key", "The member count / the members"],
          ["SPOP key [count]", "Removes and returns one member or nil; with a count, a set"],
          ["SRANDMEMBER key [count]", "The same without removing; a negative count may repeat members"],
          ["SMOVE source destination member", "1 if moved, else 0"],
          ["SINTER / SUNION / SDIFF key [key ...]", "Set of members"],
          ["SINTERSTORE / SUNIONSTORE / SDIFFSTORE dest key [key ...]", "Size of the stored result"],
        ]}
      />
      <p>
        A missing key counts as an empty set. <code>SDIFF</code> subtracts every
        later set from the first, so it is not symmetric. A <code>STORE</code>{" "}
        variant whose result is empty deletes the destination.
      </p>

      <h2 id="sorted-sets">Sorted sets</h2>
      <CodeTabs tabs={examples.zsets} className="my-6" />
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["ZADD key score member [score member ...]", "Number of members newly added"],
          ["ZSCORE key member / ZMSCORE key member [member ...]", "The score, or nil"],
          ["ZINCRBY key increment member", "The new score; a missing member starts at 0"],
          ["ZREM key member [member ...]", "Number removed"],
          ["ZCARD key", "The member count"],
          ["ZRANK / ZREVRANK key member", "The 0-based rank, or nil"],
          ["ZRANGE / ZREVRANGE key start stop [WITHSCORES]", "Members by score"],
          ["ZRANGEBYSCORE / ZREVRANGEBYSCORE key min max [WITHSCORES]", "Members inside the score window"],
          ["ZCOUNT key min max", "How many fall inside the window"],
          ["ZREMRANGEBYRANK / ZREMRANGEBYSCORE key start stop", "Number removed"],
          ["ZPOPMIN / ZPOPMAX key [count]", "The popped members with their scores"],
        ]}
      />
      <p>
        Score bounds accept a plain number, <code>-inf</code>/<code>+inf</code>,
        or a <code>(</code> prefix for an exclusive bound, as in{" "}
        <code>ZCOUNT board (75 +inf</code>.
      </p>

      <h2 id="rules">Rules that apply everywhere</h2>
      <ul>
        <li>
          A command against a key holding a different type replies{" "}
          <code>WRONGTYPE</code>, for instance <code>LPUSH</code> on a key
          created by <code>SET</code>.
        </li>
        <li>
          Removing the last element of a collection deletes the key, same as
          Redis.
        </li>
        <li>
          Keys, values, fields, and members are arbitrary bytes: they may contain
          spaces, newlines, and NUL bytes. So are a memory record&rsquo;s text
          and metadata.
        </li>
        <li>
          Calling an unimplemented Redis command returns{" "}
          <code>ERR unknown command</code>. See{" "}
          <a href="/docs/limitations">limitations</a> for what is missing.
        </li>
      </ul>
    </>
  );
}
