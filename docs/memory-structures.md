# Memory structures: Search, Vector, Hybrid

An implementation plan for turning Klyro from a Redis-style data server
into a memory database for AI agents, by adding a sixth native data
type alongside String, List, Hash, Set, and Sorted Set.

Status: plan only, nothing built. Written 2026-09-10 against commit
`b0be44f`. See [klyro.md](klyro.md) for the current architecture and
[roadmap.md](roadmap.md) for the Redis-compatibility backlog this runs
beside.

---

## 1. The two architectural calls

The product brief describes a REST API over PostgreSQL + pgvector with
a bundled local embedding model. This repo is a single-threaded,
zero-dependency (`libc` only) RESP server with its own storage engine.
Following the brief literally would mean building a second product next
to this one. Two calls make it fit instead.

### Call 1: memory is a data type, not a service

`SEARCH`, `VECTOR`, and `HYBRID` become **three modes of one native
Klyro type**, reached through a `MEM.*` command family over RESP.

Why this and not an HTTP layer:

- **Clients already work.** redis-py, ioredis, go-redis, and
  `redis-cli` all send arbitrary commands. `MEM.QUERY` needs no new
  client, no new port, no HTTP stack, no TLS decisions.
- **The keyspace already does namespaces.** The brief's `namespace` is
  a Klyro key. `user:123` is a key holding a memory index.
- **Every generic command comes free.** `TYPE` answers `memory`,
  `DEL`/`EXPIRE`/`TTL`/`RENAME`/`COPY`/`KEYS`/`SCAN`/`DBSIZE`/
  `FLUSHDB` and dump persistence all work on a memory key the day the
  type variant lands, because they operate on `Entry`, not on the value
  shape.
- **It keeps the property that defines this codebase.** No
  dependencies, no second storage engine, no Postgres.

The `MEM.` dotted prefix is the Redis module convention (`FT.`,
`JSON.`, `TS.`), so it reads as expected to anyone who has used
RediSearch.

A REST gateway matching the brief's HTTP shapes is still worth having.
It becomes a thin, separate binary that speaks RESP to Klyro. Phase 5,
not phase 1.

### Call 2: bring your own vector, at first

A local BGE-small encoder means an ONNX runtime and a WordPiece
tokenizer. That is a large dependency and a large amount of work that
has nothing to do with memory retrieval.

Phase 1 through 4 take the vector from the client: the caller embeds
however it likes and sends float32 bytes. Klyro owns storage, indexing,
filtering, scoring, and fusion. Phase 6 adds an optional embedding
provider, at which point `MEM.ADD` can accept text alone.

This is also the honest split of responsibility. The embedding model
changes every few months; the index should not have to.

---

## 2. Where it sits

New files, following the existing `types/` + `commands/` mirroring:

```text
src/types/memory/
  mod.rs        Memory: the value. Config, records, both indexes.
  record.rs     MemoryRecord and its metadata map.
  text.rs       Tokenizer, inverted index, BM25 scoring.
  vector.rs     Vector store, metrics, brute-force top-k.
  filter.rs     Metadata filter parsing and evaluation.
  fuse.rs       Score normalization, recency, importance, fusion.

src/commands/memory.rs   The MEM.* handlers.

tests/memory_search.rs
tests/memory_vector.rs
tests/memory_hybrid.rs
tests/memory_persistence.rs
```

Touched files:

| File | Change |
| --- | --- |
| `src/store.rs` | `StoreType::Memory`, `Value::Memory`, accessors, widen `type_breakdown`'s `[0usize; 5]` to 6 |
| `src/commands/mod.rs` | `MEM.*` routing arm, read-command list entries |
| `src/persist.rs` | Dump format v3 with a `MEMORY` record |
| `src/config.rs` | Nine new tunables (section 8) |
| `src/commands/server.rs` | An INFO `# memorydb` section |
| `src/types/mod.rs` | `pub mod memory;` |
| `README.md`, `docs/roadmap.md` | Command tables, status |

Nothing in the event loop, RESP codec, or expiry machinery changes.

---

## 3. Data model

One key holds one `Memory`. A `Memory` holds many `MemoryRecord`s.

```rust
pub struct Memory {
    config: MemoryConfig,
    records: HashMap<Bytes, MemoryRecord>,   // id -> record
    text: TextIndex,                          // built when mode indexes text
    vectors: VectorIndex,                     // built when mode stores vectors
    next_id: u64,                             // for server-generated ids
}

pub struct MemoryConfig {
    mode: Mode,            // Search | Vector | Hybrid
    dim: usize,            // 0 for Search
    metric: Metric,        // Cosine (default) | L2 | InnerProduct
    weights: Weights,      // keyword, vector, recency, importance
    half_life: Duration,   // recency decay, default 7 days
}

pub struct MemoryRecord {
    id: Bytes,
    text: Bytes,
    vector: Option<Vec<f32>>,     // normalized at insert when metric is Cosine
    meta: Vec<(Bytes, Bytes)>,    // small, ordered, linear scan beats hashing here
    importance: f32,              // 0.0..=1.0, default 0.5
    created_at: SystemTime,
    updated_at: SystemTime,
    expire_at: Option<SystemTime>,
}
```

The three brief structures map onto `Mode`:

| Mode | Text indexed | Vector stored | `MEM.SEARCH` | `MEM.VSEARCH` | `MEM.QUERY` |
| --- | --- | --- | --- | --- | --- |
| `SEARCH` | yes | no | yes | error | keyword only |
| `VECTOR` | stored, not indexed | yes | error | yes | vector only |
| `HYBRID` | yes | yes | yes | yes | fused |

One implementation, three logical structures, exactly as the brief
frames them. A mode that rejects a query it cannot serve is better than
one that silently returns worse results.

**Metadata is flat strings.** Not JSON. Filtering wants comparable
scalars, and a JSON parser is a dependency-shaped hole. Values that
parse as a number compare numerically; everything else compares as
bytes.

---

## 4. Command surface

Every command errors `WRONGTYPE` on a non-memory key, and
`ERR no such memory '<key>'` when the key is missing, except
`MEM.CREATE`.

### Index management

```text
MEM.CREATE key [MODE SEARCH|VECTOR|HYBRID] [DIM n] [METRIC COSINE|L2|IP]
               [WEIGHTS kw vec rec imp] [HALFLIFE seconds]
```
Defaults: `MODE HYBRID`, `METRIC COSINE`, weights `0.35 0.50 0.10 0.05`
from the brief, half-life 7 days. `DIM` is required and immutable for
`VECTOR`/`HYBRID`. Replies `OK`, or an error if the key exists.

```text
MEM.INFO key        -> map: mode, dim, metric, weights, halflife,
                       records, terms, avg_doc_len, bytes
MEM.CARD key        -> integer: live record count
MEM.CONFIG key WEIGHTS kw vec rec imp | HALFLIFE seconds
```
Weights are runtime-tunable per the brief. Mode, dim, and metric are
not, because changing them would invalidate the stored index.

### Record CRUD

```text
MEM.ADD key [ID id] TEXT text
            [VEC <float32-le-blob> | FVEC n f1 .. fn]
            [META field value ...] [IMPORTANCE x] [TTL seconds] [NX|XX]
    -> bulk: the record id
```
`VEC` takes raw little-endian float32 bytes, which is what every client
already has in hand and is 4 bytes per dimension on the wire. `FVEC`
takes them as decimal bulk strings, for `redis-cli` and for tests. A
`dim` mismatch is an error. Without `ID`, the server assigns
`m<counter>`. `NX`/`XX` gate on existence, matching `SET`.

```text
MEM.GET key id [WITHVEC]     -> map, or Nil
MEM.MGET key id [id ...]     -> array of maps/Nils
MEM.DEL key id [id ...]      -> integer removed
MEM.SETMETA key id field value [field value ...]  -> integer set
MEM.SETTEXT key id text [VEC ... | FVEC ...]      -> reindexes
MEM.EXPIRE key id seconds    -> per-record TTL, 0 clears
MEM.SCAN key cursor [COUNT n] [FILTER ...]        -> [cursor, ids]
```

Per-record TTL is what makes session and cache memory possible later
without a new structure. It is swept in `App::tick` alongside the
keyspace sweep.

### Retrieval

```text
MEM.SEARCH  key query [TOPK k] [FILTER ...] [<return-flags>]
MEM.VSEARCH key (VEC blob | FVEC n f1..fn) [TOPK k] [FILTER ...] [<return-flags>]
MEM.QUERY   key [TEXT query] [VEC blob | FVEC n f1..fn]
                [TOPK k] [WEIGHTS kw vec rec imp] [FUSION LINEAR|RRF]
                [FILTER ...] [<return-flags>]
```

`MEM.QUERY` is the brief's hybrid endpoint and the one an agent should
reach for. Given only `TEXT` it runs keyword; given only `VEC` it runs
vector; given both it fuses. Per-query `WEIGHTS` override the index
defaults without changing them.

Return flags: `NOTEXT`, `WITHMETA`, `WITHVEC`, `WITHSCORES`.
`WITHSCORES` breaks the fused score into its parts, matching the
brief's response shape.

Default reply, one map per hit, ranked:

```text
1) 1# "id"    -> "mem_001"
   2# "score" -> 0.86
   3# "text"  -> "User prefers PostgreSQL for backend projects."
```

With `WITHSCORES` the map also carries `keyword_score`,
`vector_score`, `recency_score`, and `importance`. Map replies mean
RESP3 clients get a dict and RESP2 clients get the flat array they
expect, both handled by the existing `Reply::Map`.

### Phase 6

```text
MEM.AUTO key query [TOPK k] [FILTER ...]
```
The brief's query analyzer: exact-looking queries route to keyword,
natural-language questions to vector, the rest to hybrid.

---

## 5. Keyword retrieval

**Tokenizer.** Lowercase, split on non-alphanumeric, drop tokens of one
character, drop a ~40 word English stopword list, cap at
`mem-max-terms-per-doc`. No stemming in v1: it costs recall on the
exact identifiers ("PostgreSQL", "mem_001", error strings) that are the
whole point of the Search structure. Revisit with real queries.

**Index.**

```rust
struct TextIndex {
    postings: HashMap<Bytes, Vec<(Bytes, u32)>>, // term -> [(record id, tf)]
    doc_len: HashMap<Bytes, u32>,
    total_len: u64,
    doc_count: u32,
}
```

**Scoring: BM25**, `k1 = 1.2`, `b = 0.75`.

```text
score(q, d) = Σ_t  IDF(t) · ( tf · (k1+1) ) / ( tf + k1·(1 - b + b·|d|/avgdl) )
IDF(t)      = ln( 1 + (N - n_t + 0.5) / (n_t + 0.5) )
```

BM25 rather than raw TF-IDF because length normalization matters when
memories range from a five-word preference to a paragraph of context.

Candidate generation walks the postings for the query's terms, unions
them, and keeps the top `mem-max-candidates` by BM25. Filters apply
before scoring where a filtered field is indexed, and after otherwise.

---

## 6. Vector retrieval

```rust
struct VectorIndex {
    dim: usize,
    metric: Metric,
    ids: Vec<Bytes>,      // parallel arrays: one contiguous scan, no pointer chasing
    data: Vec<f32>,       // ids[i] occupies data[i*dim .. (i+1)*dim]
    free: Vec<usize>,     // slots freed by MEM.DEL, reused by the next add
}
```

**Brute force in v1.** Cosine over a normalized store is a dot product.
At 384 dimensions, 100k records is 150 MB and roughly 38M
multiply-adds, which is single-digit milliseconds. That is the right
tradeoff before there is a workload to tune against, and it is exact,
which makes it the reference implementation for testing an approximate
index later.

**HNSW in phase 6**, behind the same `VectorIndex` interface, switched
on by a record-count threshold so small namespaces stay exact.

Cosine normalizes at insert, so query time never divides. L2 and inner
product store raw. `MEM.GET WITHVEC` returns the stored (possibly
normalized) vector, and says so in the docs.

**The blocking risk is real.** Klyro is single-threaded: a scan over a
million vectors stalls every other client on the box. Mitigations, in
order: `mem-max-scan` caps comparisons per query and returns an error
past it rather than silently truncating; `MEM.INFO` exposes record
counts so operators see it coming; phase 6's HNSW removes the linear
term. Do not skip the cap.

---

## 7. Filters and fusion

### Filters

Repeated triples, ANDed:

```text
FILTER field op value [FILTER field op value ...]
```

Operators: `EQ NE GT GTE LT LTE IN CONTAINS`. `IN` takes a
comma-separated list, `CONTAINS` does substring on the value.
Reserved field names reach the record itself rather than its metadata:
`@text`, `@importance`, `@created_at`, `@updated_at`, `@id`.

```text
MEM.QUERY user:123 TEXT "database" FILTER type EQ preference
                                   FILTER @created_at GTE 1767225600
```

Values that parse as numbers compare numerically. This covers every
example in the brief, including `created_after`, without a query
language.

### Fusion

Component scores land on a common scale before weighting. BM25 is
unbounded and cosine is `[-1, 1]`, so raw addition would let whichever
index happens to score higher dominate.

**`LINEAR` (default), matching the brief.** Min-max normalize each
component across the candidate set, then:

```text
score = w_kw·kw_norm + w_vec·vec_norm + w_rec·recency + w_imp·importance
```

**`RRF`, available per query.** `Σ 1/(60 + rank)` over each list.
Rank-based, so it needs no normalization and is unbothered by a
candidate set where one index scored everything nearly the same. Worth
offering because linear normalization is fragile exactly when a query
matches few documents.

**Recency** is exponential decay on `updated_at`:
`recency = 0.5 ^ (age / half_life)`, half-life configurable per index.

**Importance** is the stored field, clamped to `[0, 1]`, default 0.5.

A record present in only one candidate list scores 0 for the missing
component rather than being dropped. That is what lets the brief's
hybrid example surface a memory that only one index found.

---

## 8. Persistence

Dump format **version 3** adds a `MEMORY` record. Versions 1 and 2 keep
loading, as v1 does today.

```text
MEMORY <expire-at-ms> <record-count>
<blob key>
<blob config>                 mode dim metric w1 w2 w3 w4 halflife
  per record:
<blob id>
<blob text>
<line: created_ms updated_ms importance expire_at_ms meta_count vec_len>
<blob meta-field> <blob meta-value>   * meta_count
<blob vector>                 raw float32-le, omitted when vec_len is 0
```

**Indexes are rebuilt on load, not serialized.** The postings and the
vector array are both derivable from the records. Serializing them
would roughly double dump size and add a second format to keep
consistent with the first. Rebuild cost is one tokenizer pass per
record, which is far below the file read.

`Persist::save_to` gains a `MEMORY` arm; `load_v3` gains the reader;
`LoadedValue` gains a variant. Nothing else in `persist.rs` changes.

---

## 9. Configuration and observability

New `Config` fields, all runtime-settable via `CONFIG SET` except where
noted:

| Parameter | Default | Purpose |
| --- | --- | --- |
| `mem-max-topk` | 100 | Ceiling on a query's `TOPK` |
| `mem-max-candidates` | 500 | Per-index candidates before fusion |
| `mem-max-scan` | 1_000_000 | Vector comparisons per query, then error |
| `mem-max-dim` | 4096 | Rejected at `MEM.CREATE` |
| `mem-max-text-bytes` | 65536 | Per record |
| `mem-max-records` | 0 | Per namespace, 0 = unlimited |
| `mem-max-terms-per-doc` | 1024 | Tokenizer cap |
| `mem-default-weights` | `0.35 0.50 0.10 0.05` | `MEM.CREATE` default |
| `mem-recency-halflife` | 604800 | Seconds, `MEM.CREATE` default |

A new INFO section:

```text
# memorydb
memory_namespaces:12
memory_records:48310
memory_terms:19204
memory_vectors:48310
memory_vector_bytes:74204160
memory_queries_total:8821
memory_query_usec_total:41028311
```

`TYPE` answers `memory`, and INFO's existing keyspace breakdown counts
memory keys once `type_breakdown` widens.

---

## 10. How clients use it

### Any Redis client, today

```python
import redis, struct
r = redis.Redis(port=7171)

r.execute_command("MEM.CREATE", "user:123", "MODE", "HYBRID", "DIM", 384)

vec = struct.pack(f"<{len(emb)}f", *emb)      # emb from any embedding model
r.execute_command("MEM.ADD", "user:123",
                  "TEXT", "User prefers PostgreSQL for backend projects.",
                  "VEC", vec,
                  "META", "type", "preference",
                  "IMPORTANCE", 0.85)

hits = r.execute_command("MEM.QUERY", "user:123",
                         "TEXT", "What database does the user prefer?",
                         "VEC", struct.pack("<384f", *query_emb),
                         "TOPK", 5,
                         "FILTER", "type", "EQ", "preference",
                         "WITHSCORES")
```

```javascript
await redis.sendCommand(["MEM.SEARCH", "user:123", "PostgreSQL", "TOPK", "5"]);
```

```go
res, err := rdb.Do(ctx, "MEM.QUERY", "user:123", "TEXT", q, "TOPK", 5).Result()
```

Nothing to install. That is the payoff of call 1.

### SDK sugar, phase 5

Thin wrappers, one per language, that build the argument vectors and
parse the maps into objects, giving the brief's shape:

```javascript
await klyro.memory.hybrid.search({
  namespace: "user_123",
  query: "What database does the user prefer?",
  topK: 5,
});
```

Roughly 300 lines each for Python and TypeScript, on top of the
existing Redis client rather than a new connection layer.

### REST gateway, phase 5

A separate binary exposing the brief's HTTP surface
(`POST /v1/memory/hybrid/query` and siblings), translating JSON to
`MEM.*` and back. Separate so the core server keeps no HTTP dependency,
and so the gateway can scale out independently of the single-threaded
core.

---

## 11. Phasing

Each phase is independently shippable and leaves `cargo test` green.

| Phase | Scope | Rough size |
| --- | --- | --- |
| **1. Type foundation** | `StoreType::Memory`, `Memory`/`MemoryRecord`, `MEM.CREATE`/`ADD`/`GET`/`MGET`/`DEL`/`CARD`/`INFO`/`SETMETA`/`SETTEXT`/`SCAN`, per-record TTL and its sweep, dump v3, config, INFO section. No retrieval yet. | ~900 lines |
| **2. Search** | Tokenizer, postings, BM25, `MEM.SEARCH`, filter grammar and evaluation, return flags. | ~600 lines |
| **3. Vector** | `VectorIndex`, three metrics, brute-force top-k, `MEM.VSEARCH`, `mem-max-scan`. | ~450 lines |
| **4. Hybrid** | Normalization, recency, importance, `LINEAR` and `RRF`, `MEM.QUERY`, `MEM.CONFIG`. | ~400 lines |
| **5. Clients** | Command reference doc, README tables, Python and TypeScript SDKs, REST gateway. | ~800 lines, mostly outside `src/` |
| **6. Scale** | HNSW behind the `VectorIndex` interface, embedding provider so `MEM.ADD` accepts text alone, `MEM.AUTO` routing. | Large, separable |

Phases 1 through 4 are the brief's Phase 1 MVP, minus local embeddings,
plus everything the existing keyspace already provides.

---

## 12. Test plan

Unit tests beside each module, matching how `store.rs` and `zset.rs`
already test themselves:

- Tokenizer: casing, punctuation, stopwords, unicode bytes, the cap.
- BM25: a known-answer fixture where a hand-computed ranking is
  asserted, plus the length-normalization property (a short exact match
  outranks a long partial one).
- Vector: cosine against hand-computed values, normalization
  idempotence, dim mismatch rejection, slot reuse after `MEM.DEL`.
- Filter: each operator, numeric versus byte comparison, reserved
  `@`-fields.
- Fusion: a record in one list only, all-equal candidate sets (the case
  min-max normalization divides by zero on), weight overrides, decay at
  exactly one half-life.

Integration tests spawning a real server, following `tests/common`:

- CRUD round trip, id generation, `NX`/`XX`, per-record TTL expiry.
- Mode enforcement: `MEM.VSEARCH` on a `SEARCH` index errors.
- `WRONGTYPE` from every `MEM.*` command against a string key.
- Generic commands over a memory key: `TYPE`, `DEL`, `EXPIRE`,
  `RENAME`, `COPY`, `KEYS`, `SCAN`, `DBSIZE`.
- Binary-safe text and vectors through `VEC`, including embedded NULs.
- RESP2 flat-array versus RESP3 map reply shapes for hits.
- Persistence: build an index, `SAVE`, kill, restart, verify records,
  rebuilt postings, and query results all survive.
- The brief's own worked example: the five PostgreSQL/MySQL memories,
  asserting that hybrid ranks them as section 5.2 says it should.

---

## 13. Decisions to confirm

1. **RESP-native commands as the primary surface**, with REST as a
   phase 5 gateway. The alternative is HTTP first, which means an HTTP
   stack in a codebase whose defining property is having no
   dependencies.
2. **Bring-your-own vectors through phase 4.** The alternative is
   pulling in an ONNX runtime and a tokenizer before any retrieval code
   exists.
3. **No stemming in v1.** Cheap to add later; costs exact-match recall
   now.
4. **Indexes rebuilt on load rather than serialized.** Trades a little
   startup time for a much smaller dump and one format instead of two.
5. **Metadata as flat strings, not JSON.** Enough for every filter in
   the brief; no parser dependency.

Anything settled differently here mainly changes phase ordering, not
the module boundaries.
