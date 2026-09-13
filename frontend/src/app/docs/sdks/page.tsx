import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { packages } from "@/content/home";

export const metadata: Metadata = {
  title: "SDKs & packages",
  description: "Package names, install commands, and import snippets for the typed Klyro clients.",
};

export default function SdksPage() {
  return (
    <>
      <DocHeader
        eyebrow="Integrations"
        title="SDKs & packages"
        lead="The TypeScript, Python, and Go clients wrap every MEM.* command in a typed surface, while raw RESP remains available in any language."
      />

      <h2 id="packages">Packages</h2>
      <RefTable
        head={["Package", "Registry", "Install", "Status"]}
        rows={[
          ["klyro-db 0.1.1", "npm", "npm install klyro-db", "Published"],
          ["klyro-db 0.1.1", "PyPI", "pip install klyro-db", "Published"],
          ["klyro/go", "Go source module", "go get github.com/Hitesh-s0lanki/klyro/go", "Typed client"],
          ["redis-rs", "crates.io", "cargo add redis", "Raw RESP client"],
        ]}
      />
      <p>
        npm provides the typed TypeScript client and native server launcher.
        PyPI provides the typed Python client. The repository&apos;s Go module wraps
        go-redis with typed memory methods. Other languages use raw commands; see{" "}
        <a href="/docs/clients">client libraries</a>.
      </p>

      <h2 id="typescript">TypeScript</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[0].install} />
      <CodeBlock lang="ts" filename="memory.ts" code={packages[0].code} />

      <h3 id="typescript-options">Client options</h3>
      <RefTable
        head={["Option", "Type", "Default", "Meaning"]}
        rows={[
          ["host", "string", "127.0.0.1", "Klyro server host"],
          ["port", "number", "7171", "Klyro server port"],
          ["lazyConnect", "boolean", "false", "Wait for client.connect() before opening the socket"],
          ["connectTimeout", "number", "10000", "ioredis connection timeout in milliseconds"],
          ["retryStrategy", "function", "ioredis default", "Controls reconnect timing"],
        ]}
      />
      <p>
        <code>createClient()</code> returns an ioredis client, so ordinary Redis
        methods retain their upstream types. Klyro&apos;s 15 memory commands live
        under <code>client.memory</code>. Use <code>client.memoryBuffer</code> when
        IDs, text, or metadata contain arbitrary bytes. Both memory surfaces
        encode number arrays and <code>Float32Array</code> values as little-endian
        float32 vectors; returned vectors are <code>Float32Array</code> values.
      </p>

      <h3 id="typescript-results">TypeScript result shapes</h3>
      <p>
        Records expose <code>id</code>, optional <code>text</code>,{" "}
        <code>importance</code>, millisecond <code>created_at</code> and{" "}
        <code>updated_at</code> timestamps, and <code>pttl</code> in milliseconds.
        Requested metadata is a <code>Map</code>. Search hits add{" "}
        <code>score</code>, while <code>withScores</code> adds the keyword, vector,
        and recency components.
      </p>

      <h2 id="python">Python</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[1].install} />
      <CodeBlock lang="python" filename="memory.py" code={packages[1].code} />
      <p>
        <code>Klyro</code> subclasses redis-py&apos;s <code>Redis</code>, so standard
        commands remain available on the same object. Its <code>memory</code>
        property provides typed dataclasses and decoded replies for every
        current <code>MEM.*</code> command. The distribution includes a{" "}
        <code>py.typed</code> marker for mypy, Pyright, and compatible editors.
        Vector sequences are encoded as little-endian float32 bytes and decoded
        to tuples. Timestamps and <code>pttl</code> are milliseconds; TTL and
        half-life inputs are seconds.
      </p>

      <h2 id="go">Go</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[2].install} />
      <CodeBlock lang="go" filename="memory.go" code={packages[2].code} />
      <p>
        <code>NewClient</code> embeds the go-redis universal client, so its standard
        commands remain available. Typed memory methods live under{" "}
        <code>client.Memory</code>. Use <code>klyro.Wrap</code> to add them to an
        existing go-redis client.
      </p>

      <h3 id="go-zero-values">Go option zero values</h3>
      <p>
        Go uses zero values to mean “not supplied”: an empty mode defaults to{" "}
        <code>HYBRID</code>, an empty metric defaults to <code>COSINE</code>, and
        zero values for <code>TopK</code>, <code>TTL</code>, and{" "}
        <code>HalfLife</code> are omitted. Use pointers for optional importance
        and weights. Calling <code>Expire(ctx, key, id, 0)</code> is the explicit exception: it is
        sent to the server and clears a record deadline.
      </p>

      <h2 id="client-functions">Client construction</h2>
      <RefTable
        head={["Library", "Function", "What it does"]}
        rows={[
          ["TypeScript / JavaScript", "createClient(options?)", "Creates an ioredis client for 127.0.0.1:7171 by default and attaches memory plus memoryBuffer. It does not launch Klyro."],
          ["Python", "Klyro(host='127.0.0.1', port=7171, **kwargs)", "Creates a redis.Redis subclass, forces binary replies so vectors remain intact, and attaches memory."],
          ["Go", "NewClient(options)", "Creates a go-redis client. Nil options, or options with an empty address, use 127.0.0.1:7171."],
          ["Go", "Wrap(client)", "Adds the typed Memory API to an existing redis.UniversalClient without creating another connection pool."],
        ]}
      />

      <h2 id="memory-methods">All typed memory functions</h2>
      <p>
        The names differ by language, but each row calls the same server command.
        TypeScript methods return promises; Python methods are synchronous; Go
        methods receive a context and return a value plus an error.
      </p>
      <RefTable
        head={["TypeScript", "Python", "Go", "Purpose"]}
        rows={[
          ["create", "create", "Create", "Create a SEARCH, VECTOR, or HYBRID index. VECTOR and HYBRID require a dimension; returns OK or an error."],
          ["info", "info", "Info", "Return mode, dimension, metric, weights, half-life, record/vector/term counts, average document length, and estimated bytes."],
          ["config", "config", "Config", "Change weights, half-life, or both. At least one change is required; returns OK or an error."],
          ["card", "card", "Card", "Return the number of live, non-expired records in the index."],
          ["add", "add", "Add", "Insert or replace a record and return its ID. Accepts text, optional ID/vector/metadata/importance/TTL, and NX or XX."],
          ["get", "get", "Get", "Return one decoded record, or null/None/nil when its ID is missing. Return flags can include metadata or the vector and omit text."],
          ["mget", "mget", "MGet", "Return records in the requested ID order, retaining a null/None/nil entry for every missing ID."],
          ["del", "delete", "Delete", "Delete one or more records and return the number removed. At least one ID is required."],
          ["setMeta", "set_metadata", "SetMetadata", "Set one or more metadata fields and return how many fields were newly added. The metadata collection cannot be empty."],
          ["delMeta", "delete_metadata", "DeleteMetadata", "Remove one or more metadata fields and return how many existed. At least one field is required."],
          ["expire", "expire", "Expire", "Set a record TTL in seconds and return whether the record exists. Zero clears its deadline."],
          ["scan", "scan", "Scan", "Page through record IDs with an optional count and filters. Returns a cursor and IDs; stop when the cursor is 0."],
          ["search", "search", "Search", "Run BM25 keyword retrieval with optional top-K, filters, return flags, and component scores."],
          ["vsearch", "vector_search", "VectorSearch", "Run exact vector retrieval with the index metric and the same retrieval options as search."],
          ["query", "query", "Query", "Run text, vector, or hybrid retrieval. Accepts query-level weights and LINEAR or RRF fusion; text or vector is required."],
        ]}
      />

      <h2 id="option-types">Options shared by the functions</h2>
      <RefTable
        head={["Concept", "TypeScript", "Python", "Go", "Behavior"]}
        rows={[
          ["Create", "MemoryCreateOptions", "MemoryCreate", "CreateOptions", "Mode, dimension, metric, optional weights, and recency half-life in seconds."],
          ["Configure", "MemoryConfigOptions", "MemoryConfig", "ConfigOptions", "New weights and/or half-life. Existing records are not rewritten."],
          ["Add", "MemoryAddOptions", "MemoryAdd", "AddOptions", "Text is required. ID, vector, metadata, importance, record TTL, and NX/XX are optional."],
          ["Return flags", "MemoryReturnOptions", "MemoryReturn", "ReturnOptions", "NOTEXT, WITHMETA, and WITHVEC control the fields returned for records."],
          ["Search", "MemorySearchOptions", "MemorySearch", "SearchOptions", "Adds top-K, repeated AND filters, and WITHSCORES to the return flags."],
          ["Query", "MemoryQueryOptions", "MemoryQuery", "QueryOptions", "Adds text/vector inputs, per-query weights, and fusion to search options."],
          ["Scan", "MemoryScanOptions", "MemoryScan", "ScanOptions", "Controls the requested page count and optional repeated AND filters."],
          ["Weights", "MemoryWeights", "Weights", "Weights", "Keyword, vector, recency, and importance values are sent in that order."],
          ["Filter", "MemoryFilter", "Filter", "Filter", "Field, operator, and value. Operators are EQ, NE, GT, GTE, LT, LTE, IN, and CONTAINS."],
        ]}
      />

      <h2 id="result-types">Decoded result types</h2>
      <RefTable
        head={["Result", "Fields and behavior"]}
        rows={[
          ["MemoryInfo", "Index configuration and live statistics. Half-life is in seconds; avg_doc_len is numeric; bytes is an estimate."],
          ["MemoryRecord", "ID, optional text, importance, created/updated timestamps, remaining pttl, and optionally metadata/vector."],
          ["MemoryHit", "A MemoryRecord plus the final score. WITHSCORES adds keyword_score, vector_score, and recency_score."],
          ["MemoryScanResult / ScanResult", "A string cursor and the current page of record IDs."],
        ]}
      />

      <Callout variant="note" title="Time units">
        <p>
          Record <code>ttl</code>, <code>expire</code>, and index half-life inputs
          use seconds. Returned <code>created_at</code>, <code>updated_at</code>,
          and <code>pttl</code> values use milliseconds. A <code>pttl</code> of{" "}
          <code>-1</code> means the record has no deadline.
        </p>
      </Callout>

      <Callout variant="note" title="The server runs separately">
        <p>
          Creating a client opens a connection; it does not start Klyro.
          Run the server with <code>npx klyro-db</code>, Docker, or the native
          binary first. The npm package also includes the server launcher; the
          PyPI wheel and Go module are client libraries.
        </p>
      </Callout>

      <h2 id="what-an-sdk-adds">What an SDK adds over raw commands</h2>
      <ul>
        <li>
          <strong>Typed records.</strong> Metadata as an object rather than
          repeated <code>META field value</code> triples.
        </li>
        <li>
          <strong>Vector marshalling.</strong> A number array becomes float32
          bytes without you reaching for <code>struct.pack</code> or{" "}
          <code>Float32Array</code>.
        </li>
        <li>
          <strong>Parsed results.</strong> <code>WITHSCORES</code> comes back as
          a score object rather than a flat array to index by position.
        </li>
        <li>
          <strong>Structured filters.</strong> Pass filter objects or dataclasses
          instead of assembling repeated protocol tokens. The server remains the
          authority for field, operator, and value validation.
        </li>
      </ul>

      <h2 id="important-behavior">Important behavior</h2>
      <ul>
        <li><code>NX</code> and <code>XX</code> failures are server errors, not null results.</li>
        <li><code>MEM.MGET</code> preserves input order and returns null entries for missing IDs.</li>
        <li><code>MEM.SCAN</code> returns a cursor; continue until it is <code>"0"</code>.</li>
        <li><code>NOTEXT</code> omits text. Metadata and vectors are only present when requested.</li>
        <li>Memory helpers issue individual commands. Use the underlying client for pipelines or transactions.</li>
      </ul>

      <Callout variant="note" title="Raw RESP remains available">
        <p>
          The SDKs are ergonomics. Every capability is reachable over RESP with
          the client you already have, and will stay that way.
        </p>
      </Callout>
    </>
  );
}
