import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";

export const metadata: Metadata = {
  title: "MEM.* commands",
  description: "Reference for the fifteen Klyro memory commands: creation, writes, reads, scanning, and the three query forms.",
};

export default function ApiReferencePage() {
  return (
    <>
      <DocHeader
        eyebrow="Reference"
        title="MEM.* commands"
        lead="Fifteen commands with no Redis equivalent. The dotted prefix follows the convention Redis modules use, so any client reaches these through the send-a-raw-command call it already has."
      />

      <Callout variant="note" title="Reply types match Redis">
        <p>
          Every reply is a standard RESP type, which is what lets stock client
          libraries decode them. The tables below name the type rather than the
          literal bytes.
        </p>
      </Callout>

      <h2 id="index-management">Index management</h2>
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          [
            "MEM.CREATE key [MODE SEARCH|VECTOR|HYBRID] [DIM n] [METRIC COSINE|L2|IP] [WEIGHTS kw vec rec imp] [HALFLIFE seconds]",
            "OK, or an error if the key exists",
          ],
          [
            "MEM.INFO key",
            "Map: mode, dim, metric, weights, halflife, records, vectors, terms, avg_doc_len, bytes",
          ],
          ["MEM.CONFIG key [WEIGHTS kw vec rec imp] [HALFLIFE seconds]", "OK"],
          ["MEM.CARD key", "The live record count"],
        ]}
      />
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.CREATE user:123 MODE HYBRID DIM 384 METRIC COSINE \\
  WEIGHTS 0.35 0.50 0.10 0.05 HALFLIFE 604800
+OK

MEM.CONFIG user:123 WEIGHTS 0.25 0.60 0.10 0.05
+OK`}
      />
      <p>
        <code>MODE</code> defaults to <code>HYBRID</code>, <code>METRIC</code> to{" "}
        <code>COSINE</code>, and <code>HALFLIFE</code> to the server's{" "}
        <code>mem-recency-halflife</code>. <code>DIM</code> is required for any
        mode that stores vectors and must be at or below{" "}
        <code>mem-max-dim</code>.
      </p>

      <h2 id="writing-records">Writing records</h2>
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          [
            "MEM.ADD key [ID id] TEXT text [VEC blob | FVEC n f1..fn] [META field value]... [IMPORTANCE x] [TTL seconds] [NX|XX]",
            "The record id",
          ],
          ["MEM.DEL key id [id ...]", "Number removed"],
          ["MEM.SETMETA key id field value [field value ...]", "Number of fields newly set"],
          ["MEM.DELMETA key id field [field ...]", "Number removed"],
          ["MEM.EXPIRE key id seconds", "1 if set; 0 seconds clears the deadline"],
        ]}
      />

      <h3 id="vectors">Vectors</h3>
      <p>
        <code>VEC</code> takes raw little-endian float32 bytes, four per
        dimension, which is exactly what <code>struct.pack</code> or a{" "}
        <code>Float32Array</code> already holds. <code>FVEC</code> spells the same
        vector as decimal words so a query can be typed into{" "}
        <code>redis-cli</code>.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`# typeable form
MEM.ADD user:123 TEXT "Prefers PostgreSQL." FVEC 4 0.1 0.9 0.2 0.4 \\
  META type preference IMPORTANCE 0.85 TTL 86400
$1
1

# NX only writes when the id is absent, XX only when present
MEM.ADD user:123 ID pref:db TEXT "Prefers SQLite." FVEC 4 0.2 0.7 0.1 0.5 NX
$6
pref:db`}
      />
      <p>
        Importance defaults to 0.5 and is clamped to the 0.0–1.0 range. Text is
        capped by <code>mem-max-text-bytes</code>, and the tokenizer stops after{" "}
        <code>mem-max-terms-per-doc</code> terms.
      </p>

      <h2 id="reading-records">Reading records</h2>
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["MEM.GET key id [NOTEXT] [WITHMETA] [WITHVEC]", "The record as a map, or nil"],
          ["MEM.MGET key id [id ...]", "Array of records, nil per missing id"],
          ["MEM.SCAN key cursor [COUNT n] [FILTER ...]", "[next cursor, ids]"],
        ]}
      />
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.GET user:123 1 WITHMETA
1) "id"          2) "1"
3) "text"        4) "Prefers PostgreSQL."
5) "importance"  6) "0.85"
7) "created_at"  8) (integer) 1757462400
9) "meta"       10) 1) "type" 2) "preference"

MEM.SCAN user:123 0 COUNT 100 FILTER type EQ preference
1) "0"
2) 1) "1"`}
      />

      <h2 id="querying">Querying</h2>
      <RefTable
        head={["Command", "Reply"]}
        rows={[
          ["MEM.SEARCH key query [TOPK k] [FILTER ...] [flags]", "Ranked hits, keyword only"],
          ["MEM.VSEARCH key (VEC blob | FVEC n f1..fn) [TOPK k] [FILTER ...] [flags]", "Ranked hits, semantic only"],
          [
            "MEM.QUERY key [TEXT query] [VEC blob | FVEC n f1..fn] [TOPK k] [WEIGHTS ...] [FUSION LINEAR|RRF] [FILTER ...] [flags]",
            "Ranked hits, fused",
          ],
        ]}
      />
      <p>
        <code>MEM.QUERY</code> is the one to reach for. Given only{" "}
        <code>TEXT</code> it runs a keyword search, given only a vector a semantic
        one, and given both it fuses the two rankings.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.QUERY user:123 TEXT "what database do they prefer?" \\
  FVEC 384 0.02 0.41 ... TOPK 5 \\
  WEIGHTS 0.3 0.55 0.1 0.05 FUSION LINEAR \\
  FILTER type EQ preference FILTER @importance GTE 0.5 \\
  WITHSCORES WITHMETA`}
      />

      <h3 id="flags">Return flags</h3>
      <RefTable
        head={["Flag", "Effect"]}
        rows={[
          ["NOTEXT", "Omit the record text"],
          ["WITHMETA", "Include the metadata pairs"],
          ["WITHVEC", "Include the stored vector"],
          ["WITHSCORES", "Include the fused score and its four components"],
        ]}
      />

      <h3 id="filters">Filters</h3>
      <p>
        Repeated <code>FILTER field op value</code> triples, ANDed, with{" "}
        <code>EQ</code>, <code>NE</code>, <code>GT</code>, <code>GTE</code>,{" "}
        <code>LT</code>, <code>LTE</code>, <code>IN</code>, and{" "}
        <code>CONTAINS</code>. A field beginning with <code>@</code> reads the
        record rather than its metadata: <code>@id</code>, <code>@text</code>,{" "}
        <code>@importance</code>, <code>@created_at</code>,{" "}
        <code>@updated_at</code>. See{" "}
        <a href="/docs/filters">filters &amp; metadata</a>.
      </p>

      <h2 id="limits">Limits and errors</h2>
      <RefTable
        head={["Situation", "Result"]}
        rows={[
          ["Key holds another type", "WRONGTYPE error"],
          ["MEM.SEARCH on a VECTOR index", "Error: the mode cannot serve keyword search"],
          ["MEM.VSEARCH on a SEARCH index", "Error: the index stores no vectors"],
          ["Vector length ≠ DIM", "Error naming the expected dimension"],
          ["TOPK above mem-max-topk", "Error rather than a silent clamp"],
          ["Scan would exceed mem-max-scan", "The query is refused rather than partly answered"],
        ]}
      />

      <Callout variant="tip" title="Generic commands work too">
        <p>
          A memory key answers <code>TYPE</code>, <code>DEL</code>,{" "}
          <code>EXPIRE</code>, <code>TTL</code>, <code>PERSIST</code>,{" "}
          <code>RENAME</code>, <code>COPY</code>, <code>KEYS</code>,{" "}
          <code>SCAN</code>, and <code>DBSIZE</code>. See{" "}
          <a href="/docs/data-types">data type commands</a>.
        </p>
      </Callout>
    </>
  );
}
