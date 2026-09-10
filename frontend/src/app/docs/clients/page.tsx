import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";

export const metadata: Metadata = {
  title: "Client libraries",
  description: "Klyro speaks RESP, so any Redis client works. Verified clients, raw command calls, and how to send vectors from each language.",
};

export default function ClientsPage() {
  return (
    <>
      <DocHeader
        eyebrow="Integrations"
        title="Client libraries"
        lead="Klyro speaks RESP, the Redis wire protocol, so any Redis client library works. There is nothing Klyro-specific to install."
      />

      <h2 id="verified">Verified clients</h2>
      <p>These were run against Klyro over its real socket, and all of them pass:</p>
      <RefTable
        head={["Client", "Language", "RESP2", "RESP3"]}
        rows={[
          ["redis-py 8.1", "Python", "Yes", "Yes"],
          ["go-redis v9", "Go", "Yes", "Yes"],
          ["ioredis 5", "Node.js", "Yes", "n/a"],
        ]}
      />
      <p>
        Others — Jedis, Lettuce, StackExchange.Redis, redis-rs — should work too;
        they are simply untested here.
      </p>

      <h2 id="basics">The basics</h2>
      <CodeTabs
        tabs={[
          {
            label: "Python",
            lang: "python",
            code: `import redis

r = redis.Redis(host="localhost", port=7171, decode_responses=True)
r.set("greeting", "hello")
print(r.get("greeting"))

# The distributed-lock primitive
if r.set("lock:job", "token", nx=True, ex=30):
    ...`,
          },
          {
            label: "Go",
            lang: "go",
            code: `import "github.com/redis/go-redis/v9"

r := redis.NewClient(&redis.Options{Addr: "localhost:7171"})
r.Set(ctx, "greeting", "hello", 0)
value, err := r.Get(ctx, "greeting").Result()`,
          },
          {
            label: "Node.js",
            lang: "js",
            code: `import Redis from "ioredis";

const r = new Redis({ host: "localhost", port: 7171 });
await r.set("greeting", "hello");
console.log(await r.get("greeting"));`,
          },
        ]}
        className="my-6"
      />

      <h2 id="raw-commands">Sending MEM.* commands</h2>
      <p>
        The memory family has no dedicated method in any Redis client, so use the
        raw-command call each one provides. This is the one thing worth
        memorising per language:
      </p>
      <RefTable
        head={["Client", "Raw command call"]}
        rows={[
          ["redis-py", "r.execute_command(\"MEM.QUERY\", key, ...)"],
          ["ioredis", "r.call(\"MEM.QUERY\", key, ...)"],
          ["go-redis", "r.Do(ctx, \"MEM.QUERY\", key, ...)"],
          ["redis-rs", "redis::cmd(\"MEM.QUERY\").arg(key).query(&mut con)"],
        ]}
      />

      <h2 id="sending-vectors">Sending vectors</h2>
      <p>
        <code>VEC</code> expects raw little-endian float32 bytes, four per
        dimension. Every language already has that shape; it just needs handing
        over without being decoded as text.
      </p>
      <CodeTabs
        tabs={[
          {
            label: "Python",
            lang: "python",
            code: `import struct

def vec(values):
    return struct.pack(f"<{len(values)}f", *values)

# Note: decode_responses=True would mangle a binary reply, so use a
# separate raw client when you read vectors back with WITHVEC.
raw = redis.Redis(host="localhost", port=7171)
raw.execute_command("MEM.ADD", "user:123", "TEXT", text, "VEC", vec(embedding))`,
          },
          {
            label: "Node.js",
            lang: "js",
            code: `const vec = (values) => Buffer.from(new Float32Array(values).buffer);

await r.call("MEM.ADD", "user:123", "TEXT", text, "VEC", vec(embedding));

// ioredis decodes replies as strings by default; use the buffer
// variants when reading a vector back.
const raw = await r.callBuffer("MEM.GET", "user:123", "1", "WITHVEC");`,
          },
          {
            label: "Go",
            lang: "go",
            code: `func vec(values []float32) []byte {
    out := make([]byte, 4*len(values))
    for i, v := range values {
        binary.LittleEndian.PutUint32(out[i*4:], math.Float32bits(v))
    }
    return out
}

r.Do(ctx, "MEM.ADD", "user:123", "TEXT", text, "VEC", vec(embedding))`,
          },
        ]}
        className="my-6"
      />

      <Callout variant="tip" title="FVEC for humans">
        <p>
          <code>FVEC n f1..fn</code> spells the same vector as decimal words. It
          is slower to parse and larger on the wire, so keep it for{" "}
          <code>redis-cli</code>, tests, and documentation.
        </p>
      </Callout>

      <h2 id="command-line">Command line</h2>
      <p><code>redis-cli</code> works if you have it:</p>
      <CodeBlock
        lang="bash"
        filename="terminal"
        code={`redis-cli -p 7171 set greeting hello
redis-cli -p 7171 get greeting
redis-cli -p 7171 MEM.INFO user:123`}
      />
      <p>
        Without it, <code>nc</code> still works, because Klyro accepts Redis&rsquo;s
        inline command form. Replies come back in RESP, so they carry type
        markers:
      </p>
      <CodeBlock
        lang="resp"
        filename="nc localhost 7171"
        code={`PING
+PONG
SET greeting "hello there"
+OK
GET greeting
$11
hello there
LPUSH mylist a
:1`}
      />

      <h2 id="protocol">Protocol details</h2>
      <p>
        Klyro negotiates RESP3 when a client sends <code>HELLO 3</code>, and
        answers RESP2 otherwise. The type-shape differences are implemented, so a
        client gets the same shapes it would from Redis: maps for{" "}
        <code>HGETALL</code> and <code>CONFIG GET</code>, sets for{" "}
        <code>SMEMBERS</code> and <code>SINTER</code>, doubles for{" "}
        <code>ZSCORE</code>, and nested pairs for <code>WITHSCORES</code>.
      </p>

      <Callout variant="warning" title="What clients cannot do yet">
        <p>
          Every client exposes far more of the Redis API than Klyro implements.
          Calling something unimplemented returns{" "}
          <code>ERR unknown command</code>, which surfaces as an exception. The
          notable absences are transactions, pub/sub, scripting, the blocking
          commands, and the Stream, Bitmap, HyperLogLog, and Geo types. Klyro
          also has no authentication, so leave the <code>password</code> option
          unset.
        </p>
      </Callout>
    </>
  );
}
