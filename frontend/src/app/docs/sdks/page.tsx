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
        IDs, text, or metadata contain arbitrary bytes.
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

      <h2 id="memory-methods">Typed memory methods</h2>
      <RefTable
        head={["TypeScript", "Python", "Go", "Command"]}
        rows={[
          ["memory.create", "memory.create", "Memory.Create", "MEM.CREATE"],
          ["memory.info", "memory.info", "Memory.Info", "MEM.INFO"],
          ["memory.config", "memory.config", "Memory.Config", "MEM.CONFIG"],
          ["memory.card", "memory.card", "Memory.Card", "MEM.CARD"],
          ["memory.add", "memory.add", "Memory.Add", "MEM.ADD"],
          ["memory.get / mget", "memory.get / mget", "Memory.Get / MGet", "MEM.GET / MEM.MGET"],
          ["memory.del", "memory.delete", "Memory.Delete", "MEM.DEL"],
          ["memory.setMeta", "memory.set_metadata", "Memory.SetMetadata", "MEM.SETMETA"],
          ["memory.delMeta", "memory.delete_metadata", "Memory.DeleteMetadata", "MEM.DELMETA"],
          ["memory.expire", "memory.expire", "Memory.Expire", "MEM.EXPIRE"],
          ["memory.scan", "memory.scan", "Memory.Scan", "MEM.SCAN"],
          ["memory.search", "memory.search", "Memory.Search", "MEM.SEARCH"],
          ["memory.vsearch", "memory.vector_search", "Memory.VectorSearch", "MEM.VSEARCH"],
          ["memory.query", "memory.query", "Memory.Query", "MEM.QUERY"],
        ]}
      />

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
          <strong>Filter builders.</strong> Conditions compose in the host
          language and are validated before they reach the wire.
        </li>
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
