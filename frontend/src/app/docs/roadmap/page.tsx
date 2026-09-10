import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { RefTable } from "@/components/docs/Table";
import { Callout } from "@/components/docs/Callout";

export const metadata: Metadata = {
  title: "Roadmap",
  description: "What is built, what is next, and what is deliberately out of scope for Klyro.",
};

export default function RoadmapPage() {
  return (
    <>
      <DocHeader
        eyebrow="About"
        title="Roadmap"
        lead="Where Klyro is today and what comes next. Items are grouped by whether they are done, planned, or listed only for completeness."
      />

      <h2 id="shipped">Shipped</h2>
      <RefTable
        head={["Milestone", "Detail"]}
        rows={[
          ["RESP2 and RESP3", "Binary-safe values, matching reply shapes, verified against redis-py, go-redis, and ioredis"],
          ["Five classic types", "107 Redis-shaped commands across strings, lists, hashes, sets, and sorted sets"],
          ["Memory as a native type", "Three modes, 15 MEM.* commands, filters, fusion, per-record TTL"],
          ["Dump format v3", "Memory indexes persist and reload; v1 and v2 dumps still load"],
          ["Configuration surface", "Config file, command line, CONFIG GET/SET, and an INFO memorydb section"],
          ["Container image", "Two-stage static musl build, ~15 MB, unprivileged, with a protocol-level healthcheck"],
        ]}
      />

      <h2 id="planned">Planned</h2>
      <RefTable
        head={["Item", "Why"]}
        rows={[
          ["Official SDKs", "Typed clients so vectors, metadata, and filters stop being positional strings"],
          ["REST gateway", "A thin separate binary speaking RESP to Klyro, for callers that want HTTP"],
          ["Approximate vector index", "Lifts the exact-scan ceiling so an index can hold far more than tens of thousands of records"],
          ["Built-in embedding provider", "So MEM.ADD can accept text alone, with the model still swappable"],
          ["Authentication", "AUTH and ACLs, the precondition for running anywhere untrusted"],
          ["Append-only log", "Durability beyond a periodic snapshot"],
        ]}
      />

      <h2 id="under-consideration">Under consideration</h2>
      <ul>
        <li>Transactions (<code>MULTI</code>/<code>EXEC</code>) and pub/sub.</li>
        <li>Streams, bitmaps, HyperLogLog, and geospatial types.</li>
        <li>
          <code>maxmemory</code> with an eviction policy, which matters more once
          indexes outlive a single session.
        </li>
        <li>A skip-list sorted set, replacing today&rsquo;s O(n) sorted array.</li>
      </ul>

      <h2 id="out-of-scope">Listed for completeness, not planned</h2>
      <p>
        Replication and clustering appear in the gap analysis because a
        Redis-shaped server is expected to have them, not because a single-node
        project needs them next. They would change the architecture
        substantially, and the exact-scan ceiling and authentication both matter
        more first.
      </p>

      <Callout variant="note" title="Versioning">
        <p>
          Klyro is 0.1.0. Command shapes may still change between minor versions
          where the current shape turns out to be wrong. The dump format is
          versioned and older dumps keep loading, so an upgrade never asks you to
          re-import.
        </p>
      </Callout>
    </>
  );
}
