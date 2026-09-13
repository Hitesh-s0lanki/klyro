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
        lead="Use the typed Klyro clients for TypeScript, Python, and Go, or connect through any Redis client. Both paths use the same RESP wire protocol."
      />

      <Callout variant="tip" title="Typed clients are available">
        <p>
          Install a TypeScript, Python, or Go client for typed methods covering
          all 15 <code>MEM.*</code> commands. Each typed client also exposes its
          underlying Redis client, so standard commands and raw calls remain
          useful for other languages and direct protocol access. See{" "}
          <a href="/docs/sdks">SDKs and packages</a>.
        </p>
      </Callout>

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

      <h2 id="typed-basics">Typed client basics</h2>
      <p>
        Each Klyro client keeps its ecosystem&apos;s standard Redis API and adds a
        typed <code>memory</code> surface. These examples connect to the default
        address, execute ordinary commands, and close the connection cleanly.
      </p>
      <CodeTabs
        tabs={[
          {
            label: "Python",
            lang: "python",
            code: `from klyro_db import Klyro

db = Klyro()
db.set("greeting", "hello")
print(db.get("greeting").decode())

db.close()`,
          },
          {
            label: "TypeScript",
            lang: "ts",
            code: `import { createClient } from "klyro-db";

const db = createClient();
await db.set("greeting", "hello");
console.log(await db.get("greeting"));

await db.quit();`,
          },
          {
            label: "Go",
            lang: "go",
            code: `import (
    "context"
    "log"

    klyro "github.com/Hitesh-s0lanki/klyro/go"
)

ctx := context.Background()
db := klyro.NewClient(nil)
defer db.Close()

if err := db.Set(ctx, "greeting", "hello", 0).Err(); err != nil {
    log.Fatal(err)
}
value, err := db.Get(ctx, "greeting").Result()
if err != nil {
    log.Fatal(err)
}
log.Println(value)`,
          },
        ]}
        className="my-6"
      />

      <h2 id="typed-client-shape">What the typed clients provide</h2>
      <RefTable
        head={["Client", "Standard commands", "Memory commands", "Connection lifecycle"]}
        rows={[
          ["TypeScript / JavaScript", "The returned ioredis instance", "memory and memoryBuffer", "connect (with lazyConnect), quit, disconnect"],
          ["Python", "Klyro subclasses redis.Redis", "memory", "close, connection_pool.disconnect"],
          ["Go", "Client embeds redis.UniversalClient", "Memory", "Close"],
        ]}
      />
      <p>
        The wrappers do not start the server. By default they connect to{" "}
        <code>127.0.0.1:7171</code>. Connection, retry, timeout, TLS, and pool
        settings are passed to ioredis, redis-py, or go-redis respectively;
        only use settings that the Klyro server supports.
      </p>

      <h2 id="raw-commands">Using generic Redis clients</h2>
      <p>
        Generic Redis clients do not know Klyro&apos;s memory family. Use the
        raw-command call each client provides for <code>MEM.*</code>. Standard
        commands such as <code>GET</code>, <code>HSET</code>, and <code>LPUSH</code>
        use the client&apos;s normal methods.
      </p>
      <RefTable
        head={["Client", "Memory command call"]}
        rows={[
          ["redis-py", "r.execute_command(\"MEM.SEARCH\", \"notes\", \"database\")"],
          ["ioredis", "r.call(\"MEM.SEARCH\", \"notes\", \"database\")"],
          ["go-redis", "r.Do(ctx, \"MEM.SEARCH\", \"notes\", \"database\")"],
          ["redis-rs", "redis::cmd(\"MEM.SEARCH\").arg(\"notes\").arg(\"database\").query(&mut con)"],
        ]}
      />

      <p>
        The typed equivalents are <code>db.memory.search</code> in TypeScript,
        <code>db.memory.search</code> in Python, and{" "}
        <code>db.Memory.Search</code> in Go. The complete method mapping is in{" "}
        <a href="/docs/sdks#memory-methods">SDKs and packages</a>.
      </p>

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
            label: "TypeScript",
            lang: "ts",
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
          notable absences are scripting and the Stream, Bitmap, HyperLogLog,
          and Geo types. Klyro
          also has no authentication or TLS termination, so leave credentials
          unset and keep the server on a trusted network (or terminate TLS in a
          private proxy).
        </p>
      </Callout>
    </>
  );
}
