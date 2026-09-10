import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
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
        lead="The wire protocol needs no SDK. These optional clients wrap the MEM.* family in a typed surface, so vectors, metadata, and filters stop being positional strings."
      />

      <Callout variant="warning" title="Placeholder coordinates">
        <p>
          The package names, versions, and import paths on this page are
          reserved but <strong>not yet published</strong>. They are here so
          application code can be sketched against the intended shape. Replace
          them with the real coordinates once the SDKs ship; nothing else in the
          documentation depends on them.
        </p>
      </Callout>

      <h2 id="packages">Packages</h2>
      <RefTable
        head={["Package", "Registry", "Install", "Status"]}
        rows={[
          ["@klyro/client", "npm", "npm install @klyro/client", "Planned"],
          ["klyro", "PyPI", "pip install klyro", "Planned"],
          ["github.com/klyro/klyro-go", "Go modules", "go get github.com/klyro/klyro-go", "Planned"],
          ["klyro-client", "crates.io", "cargo add klyro-client", "Planned"],
        ]}
      />
      <p>
        Until they land, use any Redis client with its raw-command call. See{" "}
        <a href="/docs/clients">client libraries</a>, which is the supported
        path today.
      </p>

      <h2 id="typescript">TypeScript</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[0].install} />
      <CodeBlock lang="ts" filename="memory.ts" code={packages[0].code} />

      <h3 id="typescript-options">Client options</h3>
      <RefTable
        head={["Option", "Type", "Default", "Meaning"]}
        rows={[
          ["url", "string", "klyro://localhost:7171", "Connection string; host and port are read from it"],
          ["socketTimeout", "number", "5000", "Milliseconds before a command is abandoned"],
          ["maxRetries", "number", "3", "Reconnect attempts before an error is surfaced"],
          ["defaultTopK", "number", "10", "TOPK used when a query does not name one"],
          ["encoding", '"utf8" | "buffer"', '"utf8"', "How record text is decoded on the way back"],
        ]}
      />

      <h2 id="python">Python</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[1].install} />
      <CodeBlock lang="python" filename="memory.py" code={packages[1].code} />

      <h2 id="go">Go</h2>
      <CodeBlock lang="bash" filename="terminal" code={packages[2].install} />
      <CodeBlock lang="go" filename="memory.go" code={packages[2].code} />

      <h2 id="framework-adapters">Framework adapters</h2>
      <p>
        Thin adapters that expose a Klyro index as the memory or retriever
        interface an agent framework already expects. All placeholders, same as
        above.
      </p>
      <CodeTabs
        tabs={[
          {
            label: "LangChain",
            lang: "python",
            code: `# pip install klyro-langchain
from klyro_langchain import KlyroMemoryStore
from langchain_openai import OpenAIEmbeddings

store = KlyroMemoryStore(
    namespace="user:123",
    embeddings=OpenAIEmbeddings(),
    url="klyro://localhost:7171",
)

retriever = store.as_retriever(search_kwargs={"top_k": 5})`,
          },
          {
            label: "LlamaIndex",
            lang: "python",
            code: `# pip install klyro-llama-index
from klyro_llama_index import KlyroVectorStore
from llama_index.core import VectorStoreIndex, StorageContext

vector_store = KlyroVectorStore(namespace="user:123", dim=1536)
storage = StorageContext.from_defaults(vector_store=vector_store)
index = VectorStoreIndex.from_documents(documents, storage_context=storage)`,
          },
          {
            label: "Vercel AI SDK",
            lang: "ts",
            code: `// npm install @klyro/ai-sdk
import { klyroMemory } from "@klyro/ai-sdk";
import { streamText } from "ai";

const memory = klyroMemory({ namespace: "user:123", dim: 1536 });

const recalled = await memory.recall(prompt, { topK: 5 });

const result = streamText({
  model: myModel,
  system: \`Known about this user:\\n\${recalled.map((m) => m.text).join("\\n")}\`,
  prompt,
});`,
          },
        ]}
        className="my-6"
      />

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

      <Callout variant="note" title="Nothing is gated behind them">
        <p>
          The SDKs are ergonomics. Every capability is reachable over RESP with
          the client you already have, and will stay that way.
        </p>
      </Callout>
    </>
  );
}
