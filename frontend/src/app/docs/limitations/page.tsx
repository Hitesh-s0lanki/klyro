import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";

export const metadata: Metadata = {
  title: "Limitations",
  description: "What Klyro 0.1.0 does not do yet, and what that means for how you deploy it.",
};

export default function LimitationsPage() {
  return (
    <>
      <DocHeader
        eyebrow="About"
        title="Limitations"
        lead="Klyro is version 0.1.0. These are the things worth knowing before you depend on it, stated plainly rather than buried."
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
        Only the 122 documented commands exist. A client library will happily
        call anything else and get back <code>ERR unknown command</code>. The
        notable absences:
      </p>
      <RefTable
        head={["Area", "Status"]}
        rows={[
          ["Transactions (MULTI/EXEC)", "Not implemented"],
          ["Pub/sub (SUBSCRIBE/PUBLISH)", "Not implemented"],
          ["Scripting (EVAL)", "Not implemented"],
          ["Blocking commands (BLPOP)", "Not implemented"],
          ["Streams, Bitmaps, HyperLogLog, Geo", "Not implemented"],
          ["ZUNIONSTORE/ZINTERSTORE, lexicographic ranges, ZADD flags", "Not implemented"],
        ]}
      />
      <p>
        Practically, that means no queues and no server-side atomic
        read-modify-write beyond what a single command already does.
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
          <strong>Metadata is flat strings.</strong> Not JSON, and not nested.
          Values that parse as numbers compare numerically; everything else
          compares as bytes.
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
          position in a sorted snapshot of the keyspace, rather than the O(1) a
          real <code>SCAN</code> gives.
        </li>
        <li>
          <strong>RESP3 push messages are unimplemented</strong>, because there
          is no pub/sub or client-side caching to push.
        </li>
      </ul>

      <h2 id="what-it-is-good-at">What it is good at</h2>
      <p>
        Per-user and per-session agent memory: thousands to low tens of
        thousands of records per index, read constantly, written continuously,
        needed in single-digit milliseconds, on a trusted network, where losing
        the last minute of writes to a hard crash is survivable. Inside that
        envelope the tradeoffs above are the reason it is fast and simple to
        operate. Outside it, reach for something else, and see the{" "}
        <a href="/docs/roadmap">roadmap</a> for what is coming.
      </p>
    </>
  );
}
