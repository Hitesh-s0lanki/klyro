import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";

export const metadata: Metadata = {
  title: "Filters & metadata",
  description: "Narrow a query before it is scored, using metadata fields and the record's own fields.",
};

export default function FiltersPage() {
  return (
    <>
      <DocHeader
        eyebrow="Core concepts"
        title="Filters & metadata"
        lead="Filters are repeated FILTER triples, ANDed together, evaluated before anything is scored. That ordering is what keeps a query off the whole namespace."
      />

      <h2 id="shape">The shape of a filter</h2>
      <p>
        Each filter is three arguments: a field, an operator, and a value. Repeat
        the triple to add conditions; they are combined with AND.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`MEM.QUERY user:123 TEXT "delivery" FVEC 384 ... TOPK 5 \\
  FILTER type EQ shipping \\
  FILTER @importance GTE 0.6 \\
  FILTER region IN "eu,uk"`}
      />

      <h2 id="operators">Operators</h2>
      <RefTable
        head={["Operator", "Meaning", "Example"]}
        rows={[
          ["EQ / NE", "Equal, not equal", "FILTER type EQ preference"],
          ["GT / GTE", "Greater than, greater or equal", "FILTER @importance GTE 0.8"],
          ["LT / LTE", "Less than, less or equal", "FILTER @created_at LT 1757462400"],
          ["IN", "Member of a comma-separated list", 'FILTER region IN "eu,uk,us"'],
          ["CONTAINS", "Substring match on the value", "FILTER source CONTAINS zendesk"],
        ]}
      />
      <p>
        Values that parse as numbers compare numerically, and everything else
        compares as bytes. No schema is declared anywhere, so{" "}
        <code>FILTER @importance GTE 0.8</code> and{" "}
        <code>FILTER type EQ preference</code> both work on the same index.
      </p>

      <h2 id="record-fields">Record fields</h2>
      <p>
        A field name beginning with <code>@</code> reads the record itself rather
        than its metadata.
      </p>
      <RefTable
        head={["Field", "Type", "Typical use"]}
        rows={[
          ["@id", "bytes", "Fetch or exclude a known record"],
          ["@text", "bytes", "Substring conditions with CONTAINS"],
          ["@importance", "number", "Only recall what was marked as mattering"],
          ["@created_at", "unix seconds", "Restrict to a window of time"],
          ["@updated_at", "unix seconds", "Find records revised since a checkpoint"],
        ]}
      />

      <Callout variant="tip" title="Filters make large indexes viable">
        <p>
          Because filtering runs before scoring, a well-chosen filter is also the
          cheapest way to keep an exact vector scan fast. Tag records with the
          dimension you query by most often, and the scan only ever sees the
          slice that matters.
        </p>
      </Callout>

      <h2 id="managing-metadata">Managing metadata</h2>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`# Write metadata with the record
MEM.ADD user:123 TEXT "Escalated ticket 4821." FVEC 4 0.2 0.3 0.4 0.5 \\
  META type ticket META source zendesk META region eu
$1
3

# Add or overwrite fields afterwards
MEM.SETMETA user:123 3 status resolved priority high
:2

# Remove fields
MEM.DELMETA user:123 3 priority
:1`}
      />

      <h2 id="scanning-with-filters">Scanning with filters</h2>
      <p>
        <code>MEM.SCAN</code> takes the same filters, which turns them into a
        maintenance tool: page through everything matching a condition and act on
        it, without scoring anything.
      </p>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`# Every low-importance record from an old source
MEM.SCAN user:123 0 COUNT 200 \\
  FILTER source EQ zendesk FILTER @importance LT 0.3
1) "512"
2) 1) "18"
   2) "27"
   3) "44"

MEM.DEL user:123 18 27 44
:3`}
      />

      <h2 id="patterns">Useful patterns</h2>
      <ul>
        <li>
          <strong>Kind of memory.</strong> <code>META type preference</code>,{" "}
          <code>fact</code>, <code>event</code>, <code>summary</code>. Query one
          kind at a time when the agent needs a specific sort of recall.
        </li>
        <li>
          <strong>Provenance.</strong> <code>META source</code> with the system
          the memory came from, so a bad importer can be undone with one{" "}
          <code>MEM.SCAN</code> plus <code>MEM.DEL</code>.
        </li>
        <li>
          <strong>Tenancy inside an index.</strong> When one index serves several
          logical scopes, a <code>META scope</code> field plus{" "}
          <code>FILTER scope EQ</code> keeps them apart without a key per scope.
        </li>
        <li>
          <strong>Confidence gates.</strong> <code>FILTER @importance GTE</code>{" "}
          for prompts where only firm facts belong in context.
        </li>
      </ul>
    </>
  );
}
