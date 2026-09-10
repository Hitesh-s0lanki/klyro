import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";

export const metadata: Metadata = {
  title: "Configuration",
  description: "Every Klyro tunable, where to set it, and which ones can change while the server is running.",
};

export default function ConfigurationPage() {
  return (
    <>
      <DocHeader
        eyebrow="Reference"
        title="Configuration"
        lead="Settings come from a config file, the command line, or both. Command line arguments are applied last, so they win. Every parameter is readable with CONFIG GET, and all but bind and port can be changed at runtime."
      />

      <h2 id="sources">Where settings come from</h2>
      <CodeBlock
        lang="bash"
        filename="terminal"
        code={`# positional: port, then dump file
klyro 7171 klyro.dump

# a config file
klyro klyro.conf

# a config file plus an override that wins
klyro --config klyro.conf 7200`}
      />
      <p>
        The file format is one <code>name value</code> pair per line. Everything
        after a <code>#</code> is a comment, blank lines are ignored, and every
        value in <code>klyro.conf.sample</code> is the default, so an empty file
        behaves exactly like no file at all.
      </p>

      <h2 id="network">Network</h2>
      <RefTable
        head={["Parameter", "Default", "Meaning"]}
        rows={[
          ["bind", "0.0.0.0", "Address to listen on. Restart required."],
          ["port", "7171", "Port to listen on. Restart required."],
          ["maxclients", "10000", "Concurrent connections before new ones are refused"],
        ]}
      />

      <h2 id="persistence">Persistence</h2>
      <RefTable
        head={["Parameter", "Default", "Meaning"]}
        rows={[
          ["dbfilename", "klyro.dump", "Snapshot path, loaded at startup if present"],
          ["save-interval", "60", "Seconds between autosave checks; a save only happens if something changed"],
        ]}
      />

      <h2 id="keyspace-and-protocol">Keyspace and protocol</h2>
      <RefTable
        head={["Parameter", "Default", "Meaning"]}
        rows={[
          ["sweep-interval", "1000", "Milliseconds between active expiry sweeps"],
          ["max-string-bytes", "536870912", "Largest value a string may grow to"],
          ["proto-max-bulk-len", "536870912", "Largest bulk string one argument may carry"],
          ["client-output-buffer-limit", "268435456", "Unsent reply allowed to pile up per client"],
          ["scan-default-count", "10", "COUNT used by SCAN when the caller gives none"],
          ["zadd-max-pairs", "128", "Most score/member pairs one ZADD may carry"],
        ]}
      />

      <h2 id="memory-indexes">Memory indexes</h2>
      <RefTable
        head={["Parameter", "Default", "Meaning"]}
        rows={[
          ["mem-max-topk", "100", "Ceiling on a memory query's TOPK"],
          ["mem-max-candidates", "500", "Candidates each index contributes before fusion"],
          ["mem-max-scan", "1000000", "Ceiling on vector comparisons in one query"],
          ["mem-max-dim", "4096", "Widest embedding an index may be created with"],
          ["mem-max-text-bytes", "65536", "Ceiling on one record's text"],
          ["mem-max-records", "0", "Records per index; 0 means no limit"],
          ["mem-max-terms-per-doc", "1024", "Tokenizer cap, so one document cannot dominate the index"],
          ["mem-recency-halflife", "604800", "Default half-life for a new index, in seconds"],
        ]}
      />

      <Callout variant="note" title="Why candidates exceed TOPK">
        <p>
          <code>mem-max-candidates</code> sits above <code>mem-max-topk</code> on
          purpose. Fusion can only reorder what it is given, so a candidate set
          no larger than the answer would make the weights meaningless.
        </p>
      </Callout>

      <h2 id="at-runtime">Changing settings at runtime</h2>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`CONFIG GET mem-*
 1) "mem-max-topk"          2) "100"
 3) "mem-max-candidates"    4) "500"
 5) "mem-max-scan"          6) "1000000"
 7) "mem-max-dim"           8) "4096"
 9) "mem-max-text-bytes"   10) "65536"
11) "mem-max-records"      12) "0"
13) "mem-max-terms-per-doc" 14) "1024"
15) "mem-recency-halflife" 16) "604800"

CONFIG SET mem-max-topk 250
+OK

CONFIG SET port 7200
-ERR parameter cannot be changed at runtime

CONFIG RESETSTAT
+OK`}
      />
      <p>
        <code>INFO</code> reports what the server is doing, one text blob of{" "}
        <code># Section</code> headers over <code>key:value</code> lines. Ask for
        one section by name, including the memory-specific one:{" "}
        <code>INFO memorydb</code>.
      </p>
    </>
  );
}
