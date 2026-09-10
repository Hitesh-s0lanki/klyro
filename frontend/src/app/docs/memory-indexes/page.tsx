import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";

export const metadata: Metadata = {
  title: "Memory indexes",
  description: "How a Klyro memory index is structured: modes, records, ids, metadata, importance, and per-record TTL.",
};

export default function MemoryIndexesPage() {
  return (
    <>
      <DocHeader
        eyebrow="Core concepts"
        title="Memory indexes"
        lead="A memory index is a key like any other. One key holds one index, one index holds many records, and the mode fixed at creation decides which retrieval structures exist inside it."
      />

      <h2 id="a-key-like-any-other">A key like any other</h2>
      <p>
        Memory is a native type, not a service bolted on the side. That means the
        generic keyspace commands work on it the day you create one:{" "}
        <code>TYPE</code> answers <code>memory</code>, and <code>DEL</code>,{" "}
        <code>EXPIRE</code>, <code>TTL</code>, <code>RENAME</code>,{" "}
        <code>COPY</code>, <code>KEYS</code>, <code>SCAN</code>, and{" "}
        <code>DBSIZE</code> all behave exactly as they do for a hash.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.CREATE user:123 MODE HYBRID DIM 384
+OK
TYPE user:123
+memory
EXPIRE user:123 3600
:1
COPY user:123 user:123:backup
:1`}
      />
      <p>
        The namespace you would otherwise configure is just the key.{" "}
        <code>user:123</code>, <code>session:abc</code>, and{" "}
        <code>agent:support:tickets</code> are three indexes, isolated from one
        another, discoverable with <code>SCAN</code>.
      </p>

      <h2 id="modes">Modes</h2>
      <p>
        One implementation exposes three logical structures. The mode is set at{" "}
        <code>MEM.CREATE</code> and cannot change afterwards, because it decides
        what gets built as records arrive.
      </p>
      <RefTable
        head={["Mode", "Text indexed", "Vector stored", "MEM.SEARCH", "MEM.VSEARCH", "MEM.QUERY"]}
        rows={[
          ["SEARCH", "Yes", "No", "Yes", "Error", "Keyword only"],
          ["VECTOR", "Stored, not indexed", "Yes", "Error", "Yes", "Vector only"],
          ["HYBRID", "Yes", "Yes", "Yes", "Yes", "Fused"],
        ]}
      />
      <p>
        <code>HYBRID</code> is the default and the one to reach for. Pick{" "}
        <code>SEARCH</code> when you have no embedding pipeline, and{" "}
        <code>VECTOR</code> when the text is not worth an inverted index, for
        instance when records are already summarised into vectors elsewhere.
      </p>

      <Callout variant="note" title="Errors beat quiet degradation">
        <p>
          A <code>VECTOR</code> index rejects <code>MEM.SEARCH</code> instead of
          falling back to something weaker. If a mode cannot serve a query, you
          hear about it at the call site rather than in your evaluation numbers a
          week later.
        </p>
      </Callout>

      <h2 id="records">Records</h2>
      <p>Every record carries the same fields:</p>
      <RefTable
        head={["Field", "Type", "Notes"]}
        rows={[
          ["id", "bytes", "Supplied with ID, or assigned by the server as a counter"],
          ["text", "bytes", "Binary safe: spaces, newlines, and NUL bytes are all fine"],
          ["vector", "float32[dim]", "Optional; normalised at insert when the metric is cosine"],
          ["meta", "flat string pairs", "Ordered, small, scanned linearly; not JSON"],
          ["importance", "float 0.0–1.0", "Defaults to 0.5"],
          ["created_at / updated_at", "timestamp", "Set by the server, readable in filters"],
          ["expire_at", "timestamp", "Optional per-record TTL, independent of the key's"],
        ]}
      />

      <h3 id="ids">Ids</h3>
      <p>
        Pass <code>ID</code> to control the identity of a record, which makes
        writes idempotent and lets you update in place. Omit it and Klyro assigns
        a monotonically increasing id and returns it.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.ADD user:123 ID pref:db TEXT "Prefers PostgreSQL." FVEC 4 0.1 0.9 0.2 0.4
$6
pref:db

# NX writes only if absent, XX only if present
MEM.ADD user:123 ID pref:db TEXT "Prefers SQLite now." FVEC 4 0.2 0.7 0.1 0.5 XX
$6
pref:db`}
      />

      <h3 id="metadata">Metadata</h3>
      <p>
        Metadata is flat string pairs, deliberately not JSON. Filtering wants
        comparable scalars, and values that parse as a number compare
        numerically while everything else compares as bytes. Set and remove
        fields after the fact with <code>MEM.SETMETA</code> and{" "}
        <code>MEM.DELMETA</code>.
      </p>

      <h3 id="importance">Importance</h3>
      <p>
        Importance is a number between 0 and 1 that you assign when writing. It
        is the lever for the difference between &ldquo;the user stated a
        preference&rdquo; and &ldquo;the user said hello&rdquo;. It contributes
        to the fused score with its own weight, 0.05 by default.
      </p>

      <h3 id="record-ttl">Per-record TTL</h3>
      <p>
        Records expire on their own clock, separate from the key that holds
        them, so a session memory can lapse without the index going with it.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.ADD session:abc TEXT "Currently comparing the Pro and Team plans." \\
  FVEC 4 0.3 0.4 0.5 0.6 TTL 1800
$1
1

MEM.EXPIRE session:abc 1 3600
:1

# 0 seconds clears the deadline
MEM.EXPIRE session:abc 1 0
:1`}
      />

      <h2 id="inspecting-an-index">Inspecting an index</h2>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.INFO user:123
 1) "mode"          2) "hybrid"
 3) "dim"           4) (integer) 384
 5) "metric"        6) "cosine"
 7) "weights"       8) "0.35 0.50 0.10 0.05"
 9) "halflife"     10) (integer) 604800
11) "records"      12) (integer) 128
13) "vectors"      14) (integer) 128
15) "terms"        16) (integer) 1904
17) "avg_doc_len"  18) "11.4"
19) "bytes"        20) (integer) 421376

MEM.CARD user:123
:128

MEM.SCAN user:123 0 COUNT 100 FILTER type EQ preference
1) "0"
2) 1) "1"
   2) "pref:db"`}
      />
      <p>
        <code>MEM.SCAN</code> pages through ids with a resumable cursor and
        accepts the same filters as a query, which makes it the right tool for
        audits, exports, and bulk deletes.
      </p>
    </>
  );
}
