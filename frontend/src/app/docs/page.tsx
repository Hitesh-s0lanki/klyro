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
    "Klyro is an in-memory database for keys, collections, queues, pub/sub, transactions, expiry, persistence, and optional ranked retrieval.",
};

export default function IntroductionPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Introduction"
        lead="Klyro is an in-memory database with a familiar Redis interface. Store keys and collections, coordinate workers, publish events, run transactions, and add ranked text or vector retrieval when an application needs it."
      />

      <p>
        Klyro keeps application data in one schema-free keyspace. Strings,
        lists, hashes, sets, and sorted sets cover common state, cache, queue,
        counter, and ranking workloads. Transactions, pub/sub, blocking list
        operations, and key expiry provide the coordination primitives around
        those data structures.
      </p>
      <p>
        The server speaks RESP, so existing Redis clients can connect directly.
        It can write snapshots to disk, enforce a memory limit with configurable
        eviction, and store optional <strong>Memory</strong> indexes beside the
        core data types for keyword, vector, or hybrid retrieval.
      </p>

      <CardGrid>
        <LinkCard href="/docs/quickstart" title="Quickstart">
          Run the server, connect a client, and work with keys and collections.
        </LinkCard>
        <LinkCard href="/docs/data-types" title="Data type commands">
          Strings, lists, hashes, sets, and sorted sets with Redis-shaped replies.
        </LinkCard>
        <LinkCard href="/docs/clients" title="Client libraries">
          Connect with redis-py, ioredis, go-redis, redis-cli, or a typed package.
        </LinkCard>
        <LinkCard href="/docs/memory-indexes" title="Ranked retrieval">
          Add text, vector, or hybrid indexes when the workload calls for them.
        </LinkCard>
      </CardGrid>

      <h2 id="what-you-get">What you get</h2>
      <ul>
        <li>
          <strong>Useful data structures.</strong> Model values, counters,
          collections, queues, unique membership, and ranked sets without a schema.
        </li>
        <li>
          <strong>Atomic operations.</strong> Group commands in transactions and
          use optimistic locking when an update depends on the current value.
        </li>
        <li>
          <strong>Application coordination.</strong> Publish events, subscribe to
          channels, and block workers until queue items arrive.
        </li>
        <li>
          <strong>Controlled memory use.</strong> Expire keys and records, set a
          memory ceiling, and choose how Klyro evicts data when it reaches the limit.
        </li>
        <li>
          <strong>Restorable snapshots.</strong> Save the keyspace to disk and
          load it when the server starts again.
        </li>
      </ul>

      <h2 id="the-shape-of-it">The shape of it</h2>
      <p>
        Use a published package or any RESP client. The same keyspace and command
        behavior are available from each language.
      </p>

      <CodeTabs tabs={[...heroTabs]} className="my-6" />

      <h2 id="three-modes">Optional ranked retrieval</h2>
      <p>
        Memory indexes add three retrieval modes to the same database. The mode
        is fixed when an index is created, and a mode that cannot serve a query
        returns an error.
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
          Klyro stores and searches vectors but does not generate them. Send the
          float32 output from your embedding model; Klyro handles indexing,
          filtering, scoring, and fusion. Because the model runs outside the
          server, you can choose it independently for each index.
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

      <h2 id="where-it-fits">Where ranked retrieval fits</h2>
      <p>
        Memory indexes suit thousands to low tens of thousands of records per
        index. Vector search uses an exact scan bounded by{" "}
        <code>mem-max-scan</code>; larger vector collections need an approximate
        index, which is planned. See{" "}
        <a href="/docs/limitations">limitations</a> for the full picture before
        you depend on it.
      </p>

      <h2 id="next-steps">Next steps</h2>
      <CardGrid columns={3}>
        <LinkCard href="/docs/quickstart" title="Quickstart">
          Start the server and work with your first keys.
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
