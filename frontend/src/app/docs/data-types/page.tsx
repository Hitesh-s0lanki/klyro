import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";

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

      <h2 id="generic">Generic (any type)</h2>
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
