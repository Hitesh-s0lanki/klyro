import type { CodeTab } from "@/components/ui/CodeTabs";

/**
 * Copy for the marketing page. Command shapes and numbers match the
 * server as built; registry coordinates are marked where packages are published.
 */

export const heroTabs: CodeTab[] = [
  {
    label: "Python",
    lang: "python",
    code: `from klyro_db import Klyro

db = Klyro()
db.set("session:42", "active", ex=900)
db.hset("user:42", mapping={"name": "Ari", "plan": "pro"})
db.lpush("jobs", "generate-report")

status = db.get("session:42")
profile = db.hgetall("user:42")
job = db.brpop("jobs", timeout=5)`,
  },
  {
    label: "TypeScript",
    lang: "ts",
    code: `import { createClient } from "klyro-db";

const db = createClient();
await db.set("session:42", "active", "EX", 900);
await db.hset("user:42", { name: "Ari", plan: "pro" });
await db.lpush("jobs", "generate-report");

const status = await db.get("session:42");
const profile = await db.hgetall("user:42");
const job = await db.brpop("jobs", 5);`,
  },
  {
    label: "Go",
    lang: "go",
    code: `import klyro "github.com/Hitesh-s0lanki/klyro/go"

r := klyro.NewClient(nil)

r.Set(ctx, "session:42", "active", 15*time.Minute)
r.HSet(ctx, "user:42", "name", "Ari", "plan", "pro")
r.LPush(ctx, "jobs", "generate-report")

status, err := r.Get(ctx, "session:42").Result()
profile, err := r.HGetAll(ctx, "user:42").Result()
job, err := r.BRPop(ctx, 5*time.Second, "jobs").Result()`,
  },
  {
    label: "redis-cli",
    lang: "resp",
    code: `SET session:42 active EX 900
+OK
HSET user:42 name Ari plan pro
(integer) 2
LPUSH jobs generate-report
(integer) 1
BRPOP jobs 5
1) "jobs"
2) "generate-report"`,
  },
];

export const features = [
  {
    icon: "Database",
    title: "Six native data types",
    body:
      "Store simple values, queues, objects, unique members, rankings, and searchable records in one keyspace. Choose the type that matches the data.",
  },
  {
    icon: "Zap",
    title: "Transactions and optimistic locking",
    body:
      "Group commands with MULTI and EXEC. Use WATCH when an update depends on the current value, or DISCARD to abandon queued work.",
  },
  {
    icon: "Plug",
    title: "Pub/sub and blocking queues",
    body:
      "Publish live events by channel or pattern. Let workers block on lists or sorted sets until new work arrives.",
  },
  {
    icon: "Clock",
    title: "Expiry on keys and records",
    body:
      "Expire keys in seconds or milliseconds. Searchable records can have their own TTL without removing the index that contains them.",
  },
  {
    icon: "ShieldCheck",
    title: "Snapshots and graceful shutdown",
    body:
      "Load a snapshot at startup and save on demand, on a schedule, or during graceful shutdown. Each save replaces the previous file atomically.",
  },
  {
    icon: "Layers",
    title: "Memory limits and eviction",
    body:
      "Set a memory ceiling, then choose no eviction or an LRU, LFU, random, or TTL-based policy for selecting keys to remove.",
  },
  {
    icon: "Sparkles",
    title: "Text and vector indexes when needed",
    body:
      "Create SEARCH, VECTOR, or HYBRID indexes alongside ordinary keys. Combine BM25, vector similarity, recency, importance, and metadata filters.",
  },
  {
    icon: "Filter",
    title: "RESP clients and typed packages",
    body:
      "Connect with redis-py, ioredis, go-redis, redis-rs, or redis-cli. Klyro also provides typed memory helpers for TypeScript, Python, and Go.",
  },
] as const;

export const steps = [
  {
    number: "01",
    title: "Write application state",
    body:
      "Keys need no schema. Store a temporary value as a string, a profile as a hash, or unique members in a set.",
    code: `SET session:42 active EX 900
HSET user:42 name Ari plan pro
SADD online-users 42`,
  },
  {
    number: "02",
    title: "Coordinate workers and events",
    body:
      "Use blocking list operations for work queues, pub/sub for live events, and MULTI/EXEC when a group of commands must run together.",
    code: `LPUSH jobs generate-report
BRPOP jobs 5
PUBLISH deployments complete`,
  },
  {
    number: "03",
    title: "Add ranked retrieval where it fits",
    body:
      "A memory index is optional. Use it for records that need BM25 keyword search, vector similarity, filters, or a weighted hybrid ranking.",
    code: `MEM.QUERY user:123 TEXT "how do they ship?" FVEC 3 0.10 0.79 0.46 \\
  TOPK 5 FILTER type EQ shipping FUSION LINEAR WITHSCORES`,
  },
] as const;

export const benefits = [
  {
    title: "Keep common state behind one endpoint",
    body:
      "Sessions, counters, profiles, queues, sets, leaderboards, and searchable records share one keyspace, one persistence path, and one port.",
    metric: "1 keyspace",
  },
  {
    title: "Use familiar Redis commands",
    body:
      "Standard commands work through existing Redis clients. TypeScript, Python, and Go also have typed helpers for Klyro's MEM.* command family.",
    metric: "RESP2 / RESP3",
  },
  {
    title: "Control how RAM is used",
    body:
      "Configure a memory ceiling and choose the eviction policy that matches a cache, session store, or durable in-memory workload.",
    metric: "8 eviction policies",
  },
  {
    title: "Persist without adding another service",
    body:
      "Klyro reloads snapshots at startup and writes them on SAVE, graceful shutdown, or the configured automatic interval.",
    metric: "Atomic snapshots",
  },
] as const;

export const comparison = {
  columns: ["Core database", "Memory extension"],
  rows: [
    { label: "Data model", values: ["Strings and collections", "Text, vectors, metadata"] },
    { label: "Primary commands", values: ["GET, HSET, LPUSH, ZADD", "MEM.ADD, MEM.QUERY"] },
    { label: "Expiry", values: ["Per key", "Per index and per record"] },
    { label: "Coordination", values: ["Transactions, queues, pub/sub", "Ranked result retrieval"] },
    { label: "Search", values: ["Key scans and collection ranges", "BM25, vector, hybrid"] },
    { label: "Client access", values: ["Any RESP client", "Typed TypeScript, Python, Go, or raw RESP"] },
    { label: "Persistence", values: ["Shared snapshot", "Shared snapshot"] },
  ],
} as const;

export const packages = [
  {
    name: "klyro-db",
    manager: "npm",
    href: "https://www.npmjs.com/package/klyro-db",
    install: "npm install klyro-db@0.1.1",
    lang: "ts" as const,
    code: `import { createClient } from "klyro-db";

const klyro = createClient();

await klyro.memory.create("user:123", { mode: "HYBRID", dim: 3 });
await klyro.memory.add("user:123", {
  text: "User prefers PostgreSQL for backend projects.",
  vector: [0.12, 0.81, 0.43],
  meta: { type: "preference" },
  importance: 0.85,
});

const hits = await klyro.memory.query("user:123", {
  text: "preferred database?",
  vector: [0.10, 0.79, 0.46],
  topK: 5,
});`,
  },
  {
    name: "klyro-db",
    manager: "pip",
    href: "https://pypi.org/project/klyro-db/0.1.1/",
    install: "pip install klyro-db==0.1.1",
    lang: "python" as const,
    code: `from klyro_db import Klyro, MemoryAdd, MemoryCreate, MemoryQuery

klyro = Klyro(host="localhost", port=7171)

klyro.memory.create("user:123", MemoryCreate(mode="HYBRID", dim=3))
klyro.memory.add("user:123", MemoryAdd(
    text="User prefers PostgreSQL for backend projects.",
    vector=[0.12, 0.81, 0.43],
    metadata={"type": "preference"},
    importance=0.85,
))

hits = klyro.memory.query("user:123", MemoryQuery(
    text="preferred database?",
    vector=[0.10, 0.79, 0.46],
    top_k=5,
))`,
  },
  {
    name: "klyro/go",
    manager: "Go",
    href: "https://github.com/Hitesh-s0lanki/klyro/tree/main/go",
    install: "go get github.com/Hitesh-s0lanki/klyro/go",
    lang: "go" as const,
    code: `import klyro "github.com/Hitesh-s0lanki/klyro/go"

db := klyro.NewClient(nil)

db.Memory.Create(ctx, "user:123", klyro.CreateOptions{
  Mode: klyro.Hybrid,
  Dim: 3,
})
db.Memory.Add(ctx, "user:123", klyro.AddOptions{
  Text: "User prefers PostgreSQL for backend projects.",
  Vector: []float32{0.12, 0.81, 0.43},
})

hits, err := db.Memory.Query(ctx, "user:123", klyro.QueryOptions{
  Text: "preferred database?",
  Vector: []float32{0.10, 0.79, 0.46},
  SearchOptions: klyro.SearchOptions{TopK: 5},
})`,
  },
] as const;

export const faq = [
  {
    q: "Can Klyro replace Redis without application changes?",
    a: "Klyro speaks RESP2 and RESP3, so standard Redis clients can connect directly. It implements a documented subset of 130 Redis-shaped commands. Check the command reference before migrating an existing workload.",
  },
  {
    q: "Which data structures are included?",
    a: "Strings, lists, hashes, sets, sorted sets, and searchable memory indexes share one keyspace. Klyro also supports transactions, pub/sub, blocking queue operations, key expiry, memory limits, and eviction policies.",
  },
  {
    q: "How does Klyro persist data?",
    a: "Klyro loads a snapshot at startup and saves on demand, during graceful shutdown, or after the configured interval when data has changed. A crash can lose writes made since the last completed snapshot.",
  },
  {
    q: "Which client languages have typed support?",
    a: "TypeScript and Python packages are published as klyro-db. A Go module in the repository wraps go-redis. Other languages can send the same commands through any RESP client.",
  },
  {
    q: "Does Klyro generate embeddings?",
    a: "No. Your application sends float32 vectors from its embedding model. Klyro stores them and handles indexing, filters, similarity scoring, recency, importance, and hybrid ranking.",
  },
  {
    q: "Is it production ready?",
    a: "It is version 0.1.1. There is no authentication, no TLS, no replication, and no clustering yet, so run it on a trusted network and read the known limitations before depending on it.",
  },
] as const;
