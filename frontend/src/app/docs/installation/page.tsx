import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CodeTabs } from "@/components/ui/CodeTabs";
import { site } from "@/lib/site";

export const metadata: Metadata = {
  title: "Installation",
  description: "Run Klyro from npm, from the published container image, with Compose, or as a locally built binary.",
};

export default function InstallationPage() {
  return (
    <>
      <DocHeader
        eyebrow="Get started"
        title="Installation"
        lead="Klyro is a single static binary with one dependency. Install it from npm, run the published image, bring it up with Compose, or build it from source."
      />

      <h2 id="npm">npm</h2>
      <p>
        The shortest path from nothing to a running server. npm downloads a
        prebuilt binary for your machine; no Rust toolchain and no container
        runtime are involved.
      </p>
      <CodeTabs
        tabs={[
          {
            label: "npx",
            lang: "bash",
            code: `npx klyro-db                # port 7171, dump file klyro.dump
npx klyro-db 7200           # a different port
npx klyro-db klyro.conf     # a config file`,
          },
          {
            label: "Install",
            lang: "bash",
            code: `npm install -g klyro-db

# leaves a klyro command on your PATH, taking the
# same arguments as the binary
klyro 7200 data.dump`,
          },
        ]}
        className="my-6"
      />
      <p>
        <code>klyro-db</code> includes the typed TypeScript client and the CLI
        launcher. The native binaries ship as separate packages, and npm
        installs only the one that matches your platform:
      </p>
      <RefTable
        head={["Platform", "Package"]}
        rows={[
          ["macOS, Apple silicon", "klyro-db-darwin-arm64"],
          ["macOS, Intel", "klyro-db-darwin-x64"],
          ["Linux arm64, glibc", "klyro-db-linux-arm64"],
          ["Linux x64, glibc", "klyro-db-linux-x64"],
        ]}
      />
      <Callout variant="warning" title="No Windows build">
        <p>
          Klyro&apos;s event loop is <code>poll(2)</code> and it writes its dump
          from a POSIX signal handler, so there is no Windows binary to publish.
          Use Docker or WSL. Alpine and other musl distributions are covered by
          the container image, which is already musl-static.
        </p>
      </Callout>

      <h2 id="client-packages">Typed client packages</h2>
      <p>
        These packages connect application code to a running Klyro server and
        cover every current <code>MEM.*</code> command with typed inputs and
        decoded replies.
      </p>
      <CodeTabs
        tabs={[
          { label: "TypeScript", lang: "bash", code: "npm install klyro-db@0.1.1" },
          { label: "Python", lang: "bash", code: "pip install klyro-db==0.1.1" },
          { label: "Go", lang: "bash", code: "go get github.com/Hitesh-s0lanki/klyro/go" },
        ]}
        className="my-6"
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
        (<code>0.1.1</code> and <code>0.1</code>), and{" "}
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

# 492 tests: each integration test spawns its own klyro
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
# server
klyro_version:0.1.1
tcp_port:7171

MEM.CREATE smoke:test MODE HYBRID DIM 4
+OK
DEL smoke:test
:1`}
      />
    </>
  );
}
