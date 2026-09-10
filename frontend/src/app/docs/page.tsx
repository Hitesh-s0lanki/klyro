import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { CardGrid, LinkCard } from "@/components/docs/CardGrid";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { heroTabs } from "@/content/home";

export const metadata: Metadata = {
  title: "Introduction",
  description:
    "Klyro is an in-memory data server with a native memory type for AI agents: text and embeddings in one index, ranked by relevance, similarity, recency, and importance.",
};

export default function IntroductionPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Introduction"
        lead="Klyro is an in-memory data server that gives AI agents a memory they can rank. It stores text and embeddings in the same index, scores both against a query, and returns one fused ranking — over the Redis wire protocol, so the client you already have works."
      />

      <p>
        Most agent memory is assembled out of parts: a relational table for the
        records, a vector store for the embeddings, and a cache to keep reads
        quick. Every write has to reach two systems, every read has to merge two
        result sets, and the ranking logic ends up scattered through application
        code.
      </p>
      <p>
        Klyro collapses that into one process and one data type.{" "}
        <strong>Memory</strong> sits alongside String, List, Hash, Set, and
        Sorted Set as a sixth native type. A key holds an index, an index holds
        records, and a record holds text, an optional vector, flat metadata, an
        importance score, and its own expiry.
      </p>

      <CardGrid>
        <LinkCard href="/docs/quickstart" title="Quickstart">
          Run the server and go from an empty keyspace to a ranked hybrid query
          in about a minute.
        </LinkCard>
        <LinkCard href="/docs/memory-indexes" title="Memory indexes">
          The three retrieval modes, what each one stores, and which queries
          each can serve.
        </LinkCard>
        <LinkCard href="/docs/api-reference" title="MEM.* reference">
          All fifteen memory commands, their arguments, and their reply shapes.
        </LinkCard>
        <LinkCard href="/docs/sdks" title="SDKs & packages">
          Package names, install commands, and import snippets for the typed
          clients.
        </LinkCard>
      </CardGrid>

      <h2 id="what-you-get">What you get</h2>
      <ul>
        <li>
          <strong>Hybrid retrieval in one call.</strong> BM25 keyword scoring and
          vector similarity run against the same records, then fuse into a single
          ranking.
        </li>
        <li>
          <strong>Four ranking signals.</strong> Keyword relevance, semantic
          similarity, recency, and importance, each with a weight you set per
          index or per query.
        </li>
        <li>
          <strong>Filters before scoring.</strong> Metadata and record fields
          narrow the candidate set first, which keeps a query off the whole
          namespace.
        </li>
        <li>
          <strong>No new client.</strong> Klyro speaks RESP2 and RESP3, so
          redis-py, ioredis, go-redis, and <code>redis-cli</code> all connect
          with no adapter.
        </li>
        <li>
          <strong>The rest of the keyspace.</strong> 107 Redis-shaped commands
          across five classic types, in the same process, with matching reply
          types.
        </li>
      </ul>

      <h2 id="the-shape-of-it">The shape of it</h2>
      <p>
        Three commands cover the whole lifecycle. Create an index, add records,
        query them.
      </p>

      <CodeTabs tabs={[...heroTabs]} className="my-6" />

      <h2 id="three-modes">Three retrieval modes</h2>
      <p>
        A single implementation exposes three logical structures. The mode is
        fixed when the index is created, and a mode that cannot serve a query
        returns an error rather than quietly returning a worse answer.
      </p>

      <RefTable
        head={["Mode", "Keyword", "Semantic", "Needs vectors"]}
        rows={[
          ["SEARCH", "Yes, BM25", "No", "No"],
          ["VECTOR", "No", "Yes", "Yes"],
          ["HYBRID (default)", "Yes, BM25", "Yes", "Yes"],
        ]}
      />

      <Callout variant="note" title="Bring your own embeddings">
        <p>
          Klyro does not embed text for you. Your application sends the float32
          vector it got from whichever model it uses, and Klyro owns storage,
          indexing, filtering, scoring, and fusion. An optional built-in encoder
          is on the roadmap; until then the model can change without the index
          changing with it.
        </p>
      </Callout>

      <h2 id="how-scoring-works">How a fused score is built</h2>
      <p>
        Keyword relevance is BM25 over an inverted index. Semantic similarity is
        the index metric, one of cosine, L2, or inner product. Both are rescaled
        onto a common range before they are combined, because BM25 is unbounded
        and cosine is not.
      </p>

      <CodeBlock
        lang="text"
        filename="fused score"
        copyable={false}
        code={`score = w_keyword·keyword
      + w_vector·vector
      + w_recency·recency
      + w_importance·importance

defaults: 0.35  0.50  0.10  0.05     half-life: 7 days`}
      />

      <p>
        Recency halves every half-life, so a memory written this morning outranks
        an equally relevant one from last month. Importance is a value you set
        per record, which is how a stated preference stays ahead of small talk.
        Both weights are adjustable per index with <code>MEM.CONFIG</code> and
        per query with <code>WEIGHTS</code>.
      </p>

      <h2 id="where-it-fits">Where it fits</h2>
      <p>
        Klyro suits per-user and per-session memory: thousands to low tens of
        thousands of records per index, read constantly, written continuously,
        and needed in single-digit milliseconds. Vector search is an exact
        brute-force scan bounded by <code>mem-max-scan</code>, so it is
        deliberately not a billion-vector store. See{" "}
        <a href="/docs/limitations">limitations</a> for the full picture before
        you depend on it.
      </p>

      <h2 id="next-steps">Next steps</h2>
      <CardGrid columns={3}>
        <LinkCard href="/docs/quickstart" title="Quickstart">
          Server up, index created, first query returned.
        </LinkCard>
        <LinkCard href="/docs/retrieval-and-ranking" title="Retrieval & ranking">
          Weights, fusion strategies, and reading a score breakdown.
        </LinkCard>
        <LinkCard href="/docs/clients" title="Client libraries">
          Verified clients and how to send raw commands from each.
        </LinkCard>
      </CardGrid>
    </>
  );
}
