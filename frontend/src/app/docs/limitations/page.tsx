import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";

export const metadata: Metadata = {
  title: "Limitations",
  description: "What Klyro 0.1.1 does not do yet, and what that means for how you deploy it.",
};

export default function LimitationsPage() {
  return (
    <>
      <DocHeader
        eyebrow="About"
        title="Limitations"
        lead="Klyro is version 0.1.1. Review these operational and compatibility limits before using it for important workloads."
      />

      <Callout variant="danger" title="No authentication, ACLs, or TLS">
        <p>
          Anyone who can reach the port has full access to the keyspace. Bind to
          a trusted network, keep the port off the public internet, and leave the{" "}
          <code>password</code> option unset in your client.
        </p>
      </Callout>

      <h2 id="missing-commands">Missing commands</h2>
      <p>
        Klyro implements the 145 commands in this documentation. Calls to other
        commands return <code>ERR unknown command</code>. Notable absences include:
      </p>
      <RefTable
        head={["Area", "Status"]}
        rows={[
          ["Scripting (EVAL)", "Not implemented"],
          ["Streams, Bitmaps, HyperLogLog, Geo", "Not implemented"],
          ["ZUNIONSTORE/ZINTERSTORE, lexicographic ranges, ZADD flags", "Not implemented"],
          ["Sharded pub/sub and keyspace notifications", "Not implemented"],
        ]}
      />
      <p>
        Transactions, standard pub/sub, and blocking queue commands are
        implemented. Client libraries still expose many Redis commands that
        Klyro does not support, so use the documented command reference as the
        compatibility boundary.
      </p>

      <h2 id="retrieval">Retrieval</h2>
      <ul>
        <li>
          <strong>No built-in embedder.</strong> Memory indexes do not embed
          text; the client supplies the vector.
        </li>
        <li>
          <strong>Exact vector search only.</strong> Scoring is a brute-force
          scan capped by <code>mem-max-scan</code>, because the server is
          single-threaded and an unbounded scan would stall every other client.
          An approximate index is planned.
        </li>
        <li>
          <strong>Metadata uses flat string fields.</strong> Values that parse as
          numbers compare numerically; other values compare as bytes. Nested
          objects and JSON operators are unavailable.
        </li>
      </ul>

      <h2 id="operations">Operations</h2>
      <RefTable
        head={["Limit", "Consequence"]}
        rows={[
          ["Eviction is approximate", "A victim is the best of maxmemory-samples random draws, not the true least-recently-used key"],
          ["maxmemory measures the process", "Client buffers and the runtime count toward it, so a limit below what the server needs at rest can never be met"],
          ["Snapshot-only persistence", "A SIGKILL or crash loses everything since the last save; at most ~60s on a normal exit"],
          ["No replication or clustering", "One process, one node, bounded by one machine's RAM and one CPU core"],
          ["Single-threaded event loop", "One slow command delays every other client"],
        ]}
      />

      <h2 id="performance-characteristics">Performance characteristics</h2>
      <ul>
        <li>
          <strong>Sorted set operations are O(n).</strong> A sorted array, not a
          skip list. Fine at moderate scale, not built for huge sets.
        </li>
        <li>
          <strong>SCAN costs O(n log n) per call.</strong> The cursor is a
          position in a sorted snapshot of the keyspace. Redis uses a different
          cursor implementation with O(1) work per call.
        </li>
        <li>
          <strong>Client-side caching is unimplemented.</strong> RESP3 push
          messages are used for pub/sub, but tracking and invalidation messages
          are not available.
        </li>
      </ul>

      <h2 id="what-it-is-good-at">What it is good at</h2>
      <p>
        Klyro fits single-node workloads that keep frequently accessed state,
        queues, counters, collections, and moderate search indexes close to the
        application. Run it on a trusted network and use it where snapshot-based
        durability meets the workload&apos;s recovery needs. Large vector collections,
        public endpoints, and workloads requiring replication need a different
        deployment today. See the{" "}
        <a href="/docs/roadmap">roadmap</a> for what is coming.
      </p>
    </>
  );
}
