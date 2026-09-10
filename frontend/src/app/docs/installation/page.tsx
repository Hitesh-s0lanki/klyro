import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { site } from "@/lib/site";

export const metadata: Metadata = {
  title: "Installation",
  description: "Run Klyro from the published container image, with Compose, or as a locally built binary.",
};

export default function InstallationPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Installation"
        lead="Klyro is a single static binary with one dependency. Run the published image, bring it up with Compose, or build it from source."
      />

      <h2 id="docker">Docker</h2>
      <p>
        Every merge to <code>main</code> publishes an image, so there is usually
        nothing to build.
      </p>
      <CodeBlock
        lang="bash"
        filename="terminal"
        code={`docker run -d --name klyro -p 7171:7171 -v klyro-data:/data \\
  ${site.docker}`}
      />
      <p>
        Tags are <code>latest</code>, the version from <code>Cargo.toml</code>{" "}
        (<code>0.1.0</code> and <code>0.1</code>), and{" "}
        <code>sha-&lt;commit&gt;</code> for a specific build. Pin the version tag
        for anything you care about.
      </p>

      <h3 id="container-environment">Container environment</h3>
      <RefTable
        head={["Variable", "Default", "Meaning"]}
        rows={[
          ["KLYRO_PORT", "7171", "Port to listen on, inside the container"],
          ["KLYRO_DUMP", "/data/klyro.dump", "Path of the snapshot file"],
          ["KLYRO_CONFIG", "unset", "Config file to load, e.g. a mounted klyro.conf"],
        ]}
      />
      <CodeBlock
        lang="bash"
        filename="a config file from the host"
        code={`docker run -d -p 6380:6380 \\
  -e KLYRO_PORT=6380 \\
  -e KLYRO_CONFIG=/etc/klyro/klyro.conf \\
  -v $PWD/klyro.conf:/etc/klyro/klyro.conf:ro \\
  -v klyro-data:/data \\
  klyro`}
      />
      <p>
        <code>KLYRO_PORT</code> is passed on the command line, so it overrides a{" "}
        <code>port</code> line inside <code>KLYRO_CONFIG</code>. Arguments given
        to <code>docker run</code> after the image name bypass all three and go
        straight to the binary.
      </p>

      <Callout variant="tip" title="Shutdown saves the dump">
        <p>
          <code>docker stop</code> sends <code>SIGTERM</code>, which Klyro
          handles by writing the snapshot before exiting, so data survives a
          restart. Compose allows 30 seconds for that; raise{" "}
          <code>stop_grace_period</code> if a large keyspace needs longer.
        </p>
      </Callout>

      <h2 id="compose">Docker Compose</h2>
      <p>Compose sets up the port and the volume for you:</p>
      <CodeBlock lang="bash" filename="terminal" code={`docker compose up -d`} />
      <p>
        The image is a two-stage build, a static musl binary from{" "}
        <code>rust:1-alpine</code> copied onto bare Alpine, so it lands around 15
        MB. It runs as an unprivileged user (uid 10001), keeps the dump in the{" "}
        <code>/data</code> volume, and carries a healthcheck that{" "}
        <code>PING</code>s over the real protocol.
      </p>

      <h2 id="from-source">From source</h2>
      <CodeTabs
        tabs={[
          {
            label: "Build",
            lang: "bash",
            code: `git clone ${site.repo}
cd klyro
cargo build --release
./target/release/klyro`,
          },
          {
            label: "Run",
            lang: "bash",
            code: `# port and dump file are positional
cargo run --release -- 7171 klyro.dump

# or a config file
./target/release/klyro klyro.conf
./target/release/klyro --config klyro.conf 7200`,
          },
          {
            label: "Test",
            lang: "bash",
            code: `# unit tests in src/ plus the integration suite in tests/
cargo test

# 382 tests: each integration test spawns its own klyro
# subprocess and speaks RESP to it over a real socket.`,
          },
        ]}
        className="my-6"
      />

      <p>
        Settings come from a config file, the command line, or both. Command line
        arguments are applied last, so they win. Copy{" "}
        <code>klyro.conf.sample</code> to get started; it documents every
        parameter at its default value. See{" "}
        <a href="/docs/configuration">configuration</a>.
      </p>

      <h2 id="verify">Verify the install</h2>
      <CodeBlock
        lang="resp"
        filename="redis-cli -p 7171"
        code={`PING
+PONG

INFO server
$...
# server
klyro_version:0.1.0
tcp_port:7171

MEM.CREATE smoke:test MODE HYBRID DIM 4
+OK
DEL smoke:test
:1`}
      />
    </>
  );
}
