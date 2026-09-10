import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { Steps, Step } from "@/components/docs/Steps";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { site } from "@/lib/site";

export const metadata: Metadata = {
  title: "Quickstart",
  description: "Run Klyro, create a hybrid memory index, add records, and return a ranked query.",
};

export default function QuickstartPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Quickstart"
        lead="Start the server, create a hybrid index, write two memories, and read back a ranked result. Nothing here needs a Klyro-specific client."
      />

      <Steps>
        <Step title="Run the server">
          <p>The published image needs no build step:</p>
          <CodeBlock
            lang="bash"
            filename="terminal"
            code={`docker run -d --name klyro -p 7171:7171 -v klyro-data:/data \\
  ${site.docker}`}
          />
          <p>
            Prefer a local binary? <code>cargo build --release</code> then{" "}
            <code>./target/release/klyro</code>. Both listen on port{" "}
            <code>7171</code> by default. See{" "}
            <a href="/docs/installation">installation</a> for every option.
          </p>
        </Step>

        <Step title="Check the connection">
          <p>
            Any Redis client works, <code>redis-cli</code> included. Without one,{" "}
            <code>nc</code> works too, because Klyro accepts the inline command
            form.
          </p>
          <CodeBlock
            lang="resp"
            filename="redis-cli -p 7171"
            code={`PING
+PONG
INFO memorydb
$...
# memorydb
memory_indexes:0
memory_records:0`}
          />
        </Step>

        <Step title="Create a memory index">
          <p>
            A memory index is a key. <code>MODE</code> picks the retrieval
            structure, <code>DIM</code> fixes the vector width, and{" "}
            <code>METRIC</code> chooses how similarity is measured.
          </p>
          <CodeBlock
            lang="resp"
            filename="klyro"
            code={`MEM.CREATE user:123 MODE HYBRID DIM 4 METRIC COSINE
+OK

TYPE user:123
+memory`}
          />
          <p>
            The dimension here is 4 so the examples stay readable. A real index
            uses whatever your embedding model produces, commonly 384, 768, or
            1536.
          </p>
        </Step>

        <Step title="Add memories">
          <p>
            <code>FVEC n f1..fn</code> spells a vector as decimal words, which is
            what makes it typeable. Applications send <code>VEC</code> instead:
            raw little-endian float32 bytes, exactly what a{" "}
            <code>Float32Array</code> or <code>struct.pack</code> already holds.
          </p>
          <CodeBlock
            lang="resp"
            filename="klyro"
            code={`MEM.ADD user:123 TEXT "User prefers PostgreSQL for backend projects." \\
  FVEC 4 0.10 0.90 0.20 0.40 META type preference IMPORTANCE 0.85
$1
1

MEM.ADD user:123 TEXT "User is deploying to Frankfurt this quarter." \\
  FVEC 4 0.80 0.10 0.30 0.20 META type logistics IMPORTANCE 0.4
$1
2`}
          />
        </Step>

        <Step title="Query it">
          <p>
            <code>MEM.QUERY</code> runs a keyword search when given only{" "}
            <code>TEXT</code>, a semantic search when given only a vector, and
            fuses the two rankings when given both.
          </p>
          <CodeBlock
            lang="resp"
            filename="klyro"
            code={`MEM.QUERY user:123 TEXT "which database do they like?" \\
  FVEC 4 0.12 0.88 0.18 0.42 TOPK 5 WITHSCORES
1) 1) "1"
   2) "User prefers PostgreSQL for backend projects."
   3) 1) "score"    2) "0.9142"
      2) "keyword"  3) "0.7310"
      3) "vector"   4) "0.9981"
      4) "recency"  5) "1.0000"
      5) "importance" 6) "0.8500"`}
          />
          <p>
            <code>WITHSCORES</code> breaks the fused number into the parts that
            produced it, which is the fastest way to work out why a memory
            surfaced.
          </p>
        </Step>

        <Step title="Do it from your language">
          <p>The same three calls, through the client you already use:</p>
          <CodeTabs
            tabs={[
              {
                label: "Python",
                lang: "python",
                code: `import redis, struct

r = redis.Redis(host="localhost", port=7171, decode_responses=True)

def vec(values):
    return struct.pack(f"<{len(values)}f", *values)

r.execute_command("MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", 4)
r.execute_command(
    "MEM.ADD", "user:123",
    "TEXT", "User prefers PostgreSQL for backend projects.",
    "VEC", vec([0.1, 0.9, 0.2, 0.4]),
    "META", "type", "preference",
    "IMPORTANCE", 0.85,
)
print(r.execute_command(
    "MEM.QUERY", "user:123",
    "TEXT", "which database do they like?",
    "VEC", vec([0.12, 0.88, 0.18, 0.42]),
    "TOPK", 5, "WITHSCORES",
))`,
              },
              {
                label: "Node.js",
                lang: "js",
                code: `import Redis from "ioredis";

const r = new Redis({ host: "localhost", port: 7171 });
const vec = (values) => Buffer.from(new Float32Array(values).buffer);

await r.call("MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", "4");
await r.call(
  "MEM.ADD", "user:123",
  "TEXT", "User prefers PostgreSQL for backend projects.",
  "VEC", vec([0.1, 0.9, 0.2, 0.4]),
  "META", "type", "preference",
  "IMPORTANCE", "0.85",
);

const hits = await r.call(
  "MEM.QUERY", "user:123",
  "TEXT", "which database do they like?",
  "VEC", vec([0.12, 0.88, 0.18, 0.42]),
  "TOPK", "5", "WITHSCORES",
);
console.log(hits);`,
              },
              {
                label: "Go",
                lang: "go",
                code: `package main

import (
    "context"
    "encoding/binary"
    "math"

    "github.com/redis/go-redis/v9"
)

func vec(values []float32) []byte {
    out := make([]byte, 4*len(values))
    for i, v := range values {
        binary.LittleEndian.PutUint32(out[i*4:], math.Float32bits(v))
    }
    return out
}

func main() {
    ctx := context.Background()
    r := redis.NewClient(&redis.Options{Addr: "localhost:7171"})

    r.Do(ctx, "MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", 4)
    r.Do(ctx, "MEM.ADD", "user:123",
        "TEXT", "User prefers PostgreSQL for backend projects.",
        "VEC", vec([]float32{0.1, 0.9, 0.2, 0.4}),
        "META", "type", "preference",
        "IMPORTANCE", 0.85)

    hits, _ := r.Do(ctx, "MEM.QUERY", "user:123",
        "TEXT", "which database do they like?",
        "VEC", vec([]float32{0.12, 0.88, 0.18, 0.42}),
        "TOPK", 5, "WITHSCORES").Result()
    _ = hits
}`,
              },
            ]}
            className="my-4"
          />
        </Step>
      </Steps>

      <Callout variant="warning" title="Before you expose it">
        <p>
          Klyro has no authentication, ACLs, or TLS in 0.1.0. Bind it to a
          trusted network, and leave the <code>password</code> option unset in
          your client.
        </p>
      </Callout>

      <h2 id="where-to-go-next">Where to go next</h2>
      <ul>
        <li>
          <a href="/docs/memory-indexes">Memory indexes</a> — what each mode
          stores and which queries it will serve.
        </li>
        <li>
          <a href="/docs/retrieval-and-ranking">Retrieval &amp; ranking</a> — the
          weights, the two fusion strategies, and reading a score breakdown.
        </li>
        <li>
          <a href="/docs/api-reference">MEM.* reference</a> — every argument of
          every memory command.
        </li>
      </ul>
    </>
  );
}
