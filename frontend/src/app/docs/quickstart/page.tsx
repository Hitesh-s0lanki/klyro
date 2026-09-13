import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { Steps, Step } from "@/components/docs/Steps";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { heroTabs } from "@/content/home";
import { site } from "@/lib/site";

export const metadata: Metadata = {
  title: "Quickstart",
  description:
    "Run Klyro, connect a Redis client, and work with strings, hashes, lists, and key expiry.",
};

export default function QuickstartPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Quickstart"
        lead="Start the server and use familiar Redis commands to store application state, build a queue, and expire data."
      />

      <Steps>
        <Step title="Run the server">
          <p>Run the published container image without building from source:</p>
          <CodeBlock
            lang="bash"
            filename="terminal"
            code={`docker run -d --name klyro -p 7171:7171 -v klyro-data:/data \\
  ${site.docker}`}
          />
          <p>
            Prefer a local binary? <code>cargo build --release</code> then{" "}
            <code>./target/release/klyro</code>. Both listen on port{" "}
            <code>7171</code> by default. See{" "}
            <a href="/docs/installation">installation</a> for every option.
          </p>
        </Step>

        <Step title="Check the connection">
          <p>
            Any Redis client works, including <code>redis-cli</code>. Klyro also
            accepts inline commands, so <code>nc</code> is enough for a quick check.
          </p>
          <CodeBlock lang="resp" filename="redis-cli -p 7171" code={`PING
+PONG`} />
        </Step>

        <Step title="Store application state">
          <p>
            Strings hold simple values and counters. Hashes keep related fields
            together under one key.
          </p>
          <CodeBlock
            lang="resp"
            filename="redis-cli -p 7171"
            code={`SET app:status ready
+OK

INCR metrics:requests
:1

HSET user:42 name "Mira" plan "pro"
:2

HGETALL user:42
1) "name"
2) "Mira"
3) "plan"
4) "pro"`}
          />
        </Step>

        <Step title="Build a worker queue">
          <p>
            Lists preserve insertion order. Producers can push jobs while workers
            use a blocking pop to wait without polling.
          </p>
          <CodeBlock
            lang="resp"
            filename="producer"
            code={`LPUSH jobs:email '{"to":"mira@example.com","template":"welcome"}'
:1`}
          />
          <CodeBlock
            lang="resp"
            filename="worker"
            code={`BRPOP jobs:email 30
1) "jobs:email"
2) "{\"to\":\"mira@example.com\",\"template\":\"welcome\"}"`}
          />
        </Step>

        <Step title="Expire temporary data">
          <p>
            Give temporary state a time-to-live and Klyro removes the key after
            the configured number of seconds.
          </p>
          <CodeBlock
            lang="resp"
            filename="redis-cli -p 7171"
            code={`SET session:abc active EX 3600
+OK

TTL session:abc
:3600`}
          />
        </Step>

        <Step title="Use it from your language">
          <p>
            TypeScript, Python, and Go have typed Klyro clients. Existing RESP
            clients can use the same commands.
          </p>
          <CodeTabs tabs={[...heroTabs]} className="my-4" />
        </Step>
      </Steps>

      <Callout variant="note" title="Need ranked text or vector search?">
        <p>
          Memory indexes live in the same keyspace and add keyword, vector, and
          hybrid retrieval. Start with the <a href="/docs/memory-indexes">memory
          index guide</a> when your application needs that capability.
        </p>
      </Callout>

      <Callout variant="warning" title="Before you expose it">
        <p>
          Klyro has no authentication, ACLs, or TLS in 0.1.1. Bind it to a
          trusted network, and leave the <code>password</code> option unset in
          your client.
        </p>
      </Callout>

      <h2 id="where-to-go-next">Where to go next</h2>
      <ul>
        <li><a href="/docs/data-types">Data types</a> — commands for strings, lists, hashes, sets, and sorted sets.</li>
        <li><a href="/docs/persistence">Persistence</a> — automatic snapshots, manual saves, and restart behavior.</li>
        <li><a href="/docs/clients">Client libraries</a> — connect from Python, Node.js, Go, or the command line.</li>
      </ul>
    </>
  );
}
