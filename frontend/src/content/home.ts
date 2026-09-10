import type { CodeTab } from "@/components/ui/CodeTabs";

/**
 * Copy for the marketing page. Command shapes and numbers match the
 * server as built; the SDK package names are placeholders until the
 * packages are published.
 */

export const heroTabs: CodeTab[] = [
  {
    label: "Python",
    lang: "python",
    code: `import redis

r = redis.Redis(host="localhost", port=7171, decode_responses=True)

# One key holds one memory index.
r.execute_command("MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", "384")

# Store a memory: text, its embedding, metadata, importance.
r.execute_command(
    "MEM.ADD", "user:123",
    "TEXT", "User prefers PostgreSQL for backend projects.",
    "FVEC", 384, *embedding,
    "META", "type", "preference",
    "IMPORTANCE", 0.85,
)

# Retrieve: keyword + semantic + recency + importance, fused.
hits = r.execute_command(
    "MEM.QUERY", "user:123",
    "TEXT", "what database does the user prefer?",
    "FVEC", 384, *query_embedding,
    "TOPK", 5,
    "FILTER", "type", "EQ", "preference",
    "WITHSCORES",
)`,
  },
  {
    label: "Node.js",
    lang: "js",
    code: `import Redis from "ioredis";

const r = new Redis({ host: "localhost", port: 7171 });

await r.call("MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", "384");

await r.call(
  "MEM.ADD", "user:123",
  "TEXT", "User prefers PostgreSQL for backend projects.",
  "VEC", Buffer.from(new Float32Array(embedding).buffer),
  "META", "type", "preference",
  "IMPORTANCE", "0.85",
);

const hits = await r.call(
  "MEM.QUERY", "user:123",
  "TEXT", "what database does the user prefer?",
  "VEC", Buffer.from(new Float32Array(queryEmbedding).buffer),
  "TOPK", "5",
  "WITHSCORES",
);`,
  },
  {
    label: "Go",
    lang: "go",
    code: `import "github.com/redis/go-redis/v9"

r := redis.NewClient(&redis.Options{Addr: "localhost:7171"})

r.Do(ctx, "MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", 384)

r.Do(ctx, "MEM.ADD", "user:123",
    "TEXT", "User prefers PostgreSQL for backend projects.",
    "VEC", float32Bytes(embedding),
    "META", "type", "preference",
    "IMPORTANCE", 0.85)

hits, err := r.Do(ctx, "MEM.QUERY", "user:123",
    "TEXT", "what database does the user prefer?",
    "VEC", float32Bytes(queryEmbedding),
    "TOPK", 5, "WITHSCORES").Result()`,
  },
  {
    label: "redis-cli",
    lang: "resp",
    code: `MEM.CREATE user:123 MODE HYBRID DIM 4
+OK
MEM.ADD user:123 TEXT "User prefers PostgreSQL." FVEC 4 0.1 0.9 0.2 0.4 META type preference IMPORTANCE 0.85
$1
1
MEM.QUERY user:123 TEXT "preferred database" FVEC 4 0.1 0.8 0.2 0.5 TOPK 3 WITHSCORES
1) "1"
2) "User prefers PostgreSQL."
3) 0.91423`,
  },
];

export const features = [
  {
    icon: "Layers",
    title: "Hybrid retrieval in one query",
    body:
      "Keyword relevance and semantic similarity are scored against the same records and fused into one ranking. No second store to keep in sync, no application-side merge step.",
  },
  {
    icon: "Sparkles",
    title: "BM25 keyword scoring",
    body:
      "A real inverted index with BM25 ranking, not a substring match. Exact terms — names, error codes, SKUs — stay findable when embeddings blur them together.",
  },
  {
    icon: "Database",
    title: "Bring your own embeddings",
    body:
      "Send float32 vectors from whichever model you use, with cosine, L2, or inner-product scoring. The model changes every few months; your index does not have to.",
  },
  {
    icon: "Clock",
    title: "Recency and importance built in",
    body:
      "Every record carries a timestamp and an importance weight. Recency decays on a configurable half-life, so an agent's newest memories outrank its stalest ones by default.",
  },
  {
    icon: "Filter",
    title: "Filters that run before scoring",
    body:
      "Repeated FILTER triples over metadata and record fields, ANDed, with EQ, NE, GT, GTE, LT, LTE, IN, and CONTAINS. Filtering first keeps a query off the whole namespace.",
  },
  {
    icon: "Zap",
    title: "Three modes, one type",
    body:
      "SEARCH for keyword-only, VECTOR for semantic-only, HYBRID for both. A mode that cannot serve a query says so rather than silently returning worse results.",
  },
  {
    icon: "Plug",
    title: "Any Redis client works",
    body:
      "Klyro speaks RESP2 and RESP3. redis-py, ioredis, and go-redis are verified against it, redis-cli included. There is no Klyro-specific driver to install.",
  },
  {
    icon: "ShieldCheck",
    title: "Per-record TTL and durability",
    body:
      "Records expire independently of the key that holds them, so a session memory can lapse without dropping the index. Snapshots are atomic and reload on start.",
  },
  {
    icon: "Box",
    title: "One 15 MB container",
    body:
      "A static musl binary on bare Alpine, running unprivileged, with a healthcheck that PINGs over the real protocol. One dependency in the whole crate: libc.",
  },
] as const;

export const steps = [
  {
    number: "01",
    title: "Create an index",
    body:
      "A memory index is a key like any other. Pick a mode, a dimension, a metric, and the weights that decide how the four signals combine.",
    code: `MEM.CREATE user:123 MODE HYBRID DIM 384 METRIC COSINE \\
  WEIGHTS 0.35 0.50 0.10 0.05 HALFLIFE 604800`,
  },
  {
    number: "02",
    title: "Write memories",
    body:
      "Each record is text plus an optional vector, flat metadata, an importance score, and its own TTL. Klyro assigns the id unless you supply one.",
    code: `MEM.ADD user:123 TEXT "Ships to Berlin, prefers DHL." \\
  FVEC 384 0.02 0.41 ... META type shipping IMPORTANCE 0.7 TTL 86400`,
  },
  {
    number: "03",
    title: "Query and rank",
    body:
      "MEM.QUERY runs a keyword search given text, a semantic search given a vector, and fuses both when given the pair. WITHSCORES returns the parts.",
    code: `MEM.QUERY user:123 TEXT "how do they ship?" FVEC 384 ... \\
  TOPK 5 FILTER type EQ shipping FUSION LINEAR WITHSCORES`,
  },
] as const;

export const benefits = [
  {
    title: "Cut the memory stack from three services to one",
    body:
      "The usual agent memory setup is a relational store for records, a vector database for embeddings, and a cache in front of both. Klyro is one process that holds all three roles, so there is no dual-write path and nothing to reconcile when a write lands in one store and fails in another.",
    metric: "3 services → 1",
  },
  {
    title: "Ship it with the client you already have",
    body:
      "No SDK lock-in, no new transport, no HTTP layer to secure. If your language has a Redis client — and every language does — it can already talk to Klyro. Onboarding is a connection string, not a migration.",
    metric: "0 new drivers",
  },
  {
    title: "Sub-millisecond reads, in process memory",
    body:
      "Everything lives in RAM behind a single-threaded event loop, so commands run to completion without lock contention or a network hop to a second tier. Retrieval latency stops being the reason your agent feels slow.",
    metric: "In-RAM reads",
  },
  {
    title: "Recall you can actually explain",
    body:
      "WITHSCORES breaks a fused result into its keyword, vector, recency, and importance parts. When an agent surfaces the wrong memory, you can see which signal caused it and change one weight instead of guessing at a prompt.",
    metric: "4 signals, itemised",
  },
] as const;

export const comparison = {
  columns: ["Klyro", "Postgres + pgvector", "Vector DB + cache"],
  rows: [
    { label: "Keyword (BM25) ranking", values: [true, "tsvector, separate index", false] },
    { label: "Semantic search", values: [true, true, true] },
    { label: "Hybrid fusion built in", values: [true, false, "app-side"] },
    { label: "Recency & importance weighting", values: [true, false, false] },
    { label: "Works with existing clients", values: [true, "SQL driver", "vendor SDK"] },
    { label: "Sub-millisecond in-memory reads", values: [true, false, "cache only"] },
    { label: "Services to operate", values: ["1", "1", "2+"] },
    { label: "Per-record TTL", values: [true, "cron job", "partial"] },
  ],
} as const;

export const packages = [
  {
    name: "@klyro/client",
    manager: "npm",
    install: "npm install @klyro/client",
    lang: "ts" as const,
    code: `import { Klyro } from "@klyro/client";

const klyro = new Klyro({ url: "klyro://localhost:7171" });

await klyro.memory.create("user:123", { mode: "hybrid", dim: 384 });
await klyro.memory.add("user:123", {
  text: "User prefers PostgreSQL for backend projects.",
  vector: embedding,
  meta: { type: "preference" },
  importance: 0.85,
});

const hits = await klyro.memory.query("user:123", {
  text: "preferred database?",
  vector: queryEmbedding,
  topK: 5,
});`,
  },
  {
    name: "klyro",
    manager: "pip",
    install: "pip install klyro",
    lang: "python" as const,
    code: `from klyro import Klyro

klyro = Klyro(host="localhost", port=7171)

klyro.memory.create("user:123", mode="hybrid", dim=384)
klyro.memory.add(
    "user:123",
    text="User prefers PostgreSQL for backend projects.",
    vector=embedding,
    meta={"type": "preference"},
    importance=0.85,
)

hits = klyro.memory.query(
    "user:123",
    text="preferred database?",
    vector=query_embedding,
    top_k=5,
)`,
  },
  {
    name: "klyro-go",
    manager: "go get",
    install: "go get github.com/klyro/klyro-go",
    lang: "go" as const,
    code: `import "github.com/klyro/klyro-go"

client := klyro.New("localhost:7171")

client.Memory.Create(ctx, "user:123", klyro.Hybrid(384))
client.Memory.Add(ctx, "user:123", klyro.Record{
    Text:       "User prefers PostgreSQL for backend projects.",
    Vector:     embedding,
    Meta:       map[string]string{"type": "preference"},
    Importance: 0.85,
})

hits, err := client.Memory.Query(ctx, "user:123", klyro.Query{
    Text:   "preferred database?",
    Vector: queryEmbedding,
    TopK:   5,
})`,
  },
] as const;

export const faq = [
  {
    q: "Does Klyro generate embeddings for me?",
    a: "Not yet. You send float32 vectors from whichever model you use, and Klyro owns storage, indexing, filtering, scoring, and fusion. An optional built-in embedder is planned, at which point MEM.ADD will accept text alone.",
  },
  {
    q: "Is it a fork of Redis?",
    a: "No. It is an independent server written in Rust with its own storage engine, which happens to speak RESP so that existing clients work. The MEM.* family has no Redis equivalent.",
  },
  {
    q: "How large a dataset can one index hold?",
    a: "Vector search is an exact brute-force scan bounded by the mem-max-scan setting, which suits per-user and per-session indexes of thousands to low tens of thousands of records. An approximate index for larger corpora is on the roadmap.",
  },
  {
    q: "What happens on restart?",
    a: "The keyspace is snapshotted to a dump file on graceful shutdown, on SAVE, and automatically every 60 seconds when something changed. Memory indexes persist their configuration and records; the inverted index and vector array are rebuilt on load.",
  },
  {
    q: "Can I use my existing Redis commands too?",
    a: "Yes. Strings, lists, hashes, sets, and sorted sets are all there, 107 Redis-shaped commands in total, with matching reply types. Memory indexes sit in the same keyspace as everything else.",
  },
  {
    q: "Is it production ready?",
    a: "It is version 0.1.0. There is no authentication, no TLS, no replication, and no clustering yet, so run it on a trusted network and read the known limitations before depending on it.",
  },
] as const;
