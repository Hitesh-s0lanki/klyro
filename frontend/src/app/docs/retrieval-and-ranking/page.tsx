import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";

export const metadata: Metadata = {
  title: "Retrieval & ranking",
  description: "BM25, vector similarity, recency decay, importance, and how Klyro fuses them into one ranking.",
};

export default function RetrievalPage() {
  return (
    <>
      <DocHeader
        eyebrow="Core concepts"
        title="Retrieval & ranking"
        lead="Four signals decide what an agent recalls. This page covers what each one measures, how they are combined, and how to change the balance when the results are not what you want."
      />

      <h2 id="the-four-signals">The four signals</h2>
      <RefTable
        head={["Signal", "Measures", "Source"]}
        rows={[
          ["Keyword", "Term overlap, length-normalised", "BM25 over an inverted index"],
          ["Vector", "Semantic closeness", "The index metric: cosine, L2, or inner product"],
          ["Recency", "How fresh the record is", "Exponential decay on a configurable half-life"],
          ["Importance", "How much the record matters", "The value you set per record, 0.0 to 1.0"],
        ]}
      />

      <h2 id="fusion">Fusion</h2>
      <p>
        Keyword and vector scores live on different scales: BM25 is unbounded,
        cosine is not. Klyro rescales both onto a common range before combining
        them, then adds the recency and importance terms.
      </p>
      <CodeBlock
        lang="text"
        filename="LINEAR fusion"
        copyable={false}
        code={`score = w_keyword·keyword
      + w_vector·vector
      + w_recency·recency
      + w_importance·importance

defaults  0.35        0.50      0.10        0.05`}
      />
      <p>
        <code>FUSION RRF</code> switches to reciprocal rank fusion, which ranks
        by position rather than by score. It is steadier when one of the two
        rankings scored nearly everything the same, which happens with short
        queries or a narrow corpus.
      </p>

      <CodeTabs
        className="my-6"
        tabs={[
          { label: "TypeScript", lang: "ts", code: `const hits = await db.memory.query("user:123", {
  text: "shipping preferences",
  vector: [0.10, 0.79, 0.46],
  topK: 5,
  weights: { keyword: 0.2, vector: 0.7, recency: 0.05, importance: 0.05 },
  fusion: "RRF",
  withScores: true,
});` },
          { label: "Python", lang: "python", code: `from klyro_db import MemoryQuery, Weights

hits = db.memory.query("user:123", MemoryQuery(
    text="shipping preferences",
    vector=[0.10, 0.79, 0.46],
    top_k=5,
    weights=Weights(keyword=0.2, vector=0.7, recency=0.05, importance=0.05),
    fusion="RRF",
    with_scores=True,
))` },
          { label: "Go", lang: "go", code: `weights := &klyro.Weights{Keyword: 0.2, Vector: 0.7, Recency: 0.05, Importance: 0.05}
hits, err := db.Memory.Query(ctx, "user:123", klyro.QueryOptions{
  Text: "shipping preferences",
  Vector: []float32{0.10, 0.79, 0.46},
  Weights: weights,
  Fusion: klyro.RRF,
  SearchOptions: klyro.SearchOptions{TopK: 5, WithScores: true},
})` },
          { label: "redis-cli", lang: "resp", code: `MEM.QUERY user:123 TEXT "shipping preferences" FVEC 3 0.10 0.79 0.46 \\
  WEIGHTS 0.2 0.7 0.05 0.05 FUSION RRF TOPK 5 WITHSCORES` },
        ]}
      />

      <h2 id="recency">Recency decay</h2>
      <p>
        Recency halves every <code>HALFLIFE</code> seconds, defaulting to seven
        days. A record written moments ago scores 1.0 on this signal; one written
        a half-life ago scores 0.5; one from four half-lives back scores about
        0.06.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`# A day-long half-life for a fast-moving session index
MEM.CREATE session:abc MODE HYBRID DIM 384 HALFLIFE 86400
+OK

# Change it later without touching the records
MEM.CONFIG session:abc HALFLIFE 43200 WEIGHTS 0.3 0.45 0.2 0.05
+OK`}
      />

      <Callout variant="tip" title="Tune the weights, not the prompt">
        <p>
          When an agent keeps recalling something stale, raise the recency weight
          before rewriting the prompt. When it keeps missing exact names or error
          codes, raise the keyword weight. Both are one command, and{" "}
          <code>WITHSCORES</code> tells you which one to reach for.
        </p>
      </Callout>

      <h2 id="reading-a-score">Reading a score breakdown</h2>
      <p>
        <code>WITHSCORES</code> returns the fused score together with the
        components that produced it, so a surprising result is a data question
        rather than a guess.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.QUERY user:123 TEXT "which database?" FVEC 4 0.1 0.9 0.2 0.4 TOPK 2 WITHSCORES
1) 1) "1"
   2) "User prefers PostgreSQL for backend projects."
   3) 1) "score"      2) "0.9142"
      3) "keyword"    4) "0.7310"
      5) "vector"     6) "0.9981"
      7) "recency"    8) "1.0000"
      9) "importance" 10) "0.8500"`}
      />

      <h2 id="the-three-query-commands">The three query commands</h2>
      <RefTable
        head={["Command", "Uses", "When to use it"]}
        rows={[
          ["MEM.SEARCH", "Keyword only", "Exact terms matter and you have no query vector"],
          ["MEM.VSEARCH", "Vector only", "Pure similarity, e.g. deduplicating near-identical memories"],
          ["MEM.QUERY", "Either or both, fused", "The default: pass what you have and let the index decide"],
        ]}
      />
      <p>
        <code>MEM.QUERY</code> given only <code>TEXT</code> runs a keyword
        search, given only a vector runs a semantic one, and given both fuses
        them. That is why application code can call one command whether or not an
        embedding was available for a given turn.
      </p>

      <h2 id="return-flags">Return flags</h2>
      <RefTable
        head={["Flag", "Effect"]}
        rows={[
          ["NOTEXT", "Omit the record text, when you only need ids and scores"],
          ["WITHMETA", "Include the metadata pairs with each hit"],
          ["WITHVEC", "Include the stored vector"],
          ["WITHSCORES", "Include the fused score and its components"],
        ]}
      />

      <h2 id="metrics">Metrics</h2>
      <p>
        The metric is fixed at creation. Cosine is the default and the right
        choice for most embedding models; vectors are normalised at insert so the
        comparison is a dot product at query time.
      </p>
      <RefTable
        head={["METRIC", "Comparison", "Notes"]}
        rows={[
          ["COSINE", "Angle between vectors", "Default; magnitudes are normalised away"],
          ["L2", "Euclidean distance", "Smaller is closer; converted so higher scores rank first"],
          ["IP", "Inner product", "For models trained with an unnormalised objective"],
        ]}
      />

      <Callout variant="warning" title="Vector search is an exact scan">
        <p>
          Scoring visits every candidate that survives the filters, bounded by{" "}
          <code>mem-max-scan</code>. That is exact rather than approximate, and it
          suits indexes of thousands to low tens of thousands of records. Filter
          hard on large indexes, and see{" "}
          <a href="/docs/limitations">limitations</a> for the ceiling.
        </p>
      </Callout>
    </>
  );
}
