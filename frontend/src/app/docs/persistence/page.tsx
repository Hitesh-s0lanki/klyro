import type { Metadata } from "next";
import { DocHeader } from "@/components/docs/DocHeader";
import { Callout } from "@/components/docs/Callout";
import { RefTable } from "@/components/docs/Table";
import { CodeBlock } from "@/components/ui/CodeBlock";

export const metadata: Metadata = {
  title: "Persistence",
  description: "How Klyro snapshots the keyspace, what a memory index writes to disk, and what a crash costs.",
};

export default function PersistencePage() {
  return (
    <>
      <DocHeader
        eyebrow="Core concepts"
        title="Persistence"
        lead="Klyro keeps everything in RAM and snapshots the whole keyspace to a dump file. Saves are atomic, the format is versioned, and older dumps still load."
      />

      <h2 id="when-a-save-happens">When a save happens</h2>
      <RefTable
        head={["Trigger", "Notes"]}
        rows={[
          ["Graceful shutdown", "SHUTDOWN, SIGINT, or SIGTERM — including docker stop"],
          ["SAVE", "Writes the dump immediately and replies OK"],
          ["Autosave", "Every save-interval seconds, but only if something changed"],
        ]}
      />
      <p>
        On startup the dump file is loaded if it exists. Killing the process
        outright, a crash, or power loss loses everything since the last save,
        which is at most <code>save-interval</code> seconds of writes.
      </p>

      <h2 id="the-format">The dump format</h2>
      <p>
        The file is length-prefixed, because a value may contain a newline: a{" "}
        <code>KLYRO-DUMP 3</code> header, then one record per key giving its type
        and absolute expiry, then each blob as its length followed by exactly
        that many bytes. Saves are written to <code>&lt;path&gt;.tmp</code> and
        renamed over the real path, so a crash mid-save cannot corrupt the
        existing dump.
      </p>

      <Callout variant="note" title="Indexes are rebuilt, not stored">
        <p>
          A memory index writes its configuration and its records, never its
          inverted index or its vector array. Both are derivable, so they are
          rebuilt on load. That keeps the dump small and leaves one format to
          maintain rather than two.
        </p>
      </Callout>

      <p>
        Version 1 and 2 dumps still load, so an existing file survives the
        upgrade. They are rewritten as version 3 on the next save.
      </p>

      <h2 id="operating-it">Operating it</h2>
      <CodeBlock
        lang="resp"
        filename="klyro"
        code={`SAVE
+OK

CONFIG GET dbfilename save-interval
1) "dbfilename"     2) "klyro.dump"
3) "save-interval"  4) "60"

# Redirect the next save; the current file is left alone
CONFIG SET dbfilename /data/klyro-2.dump
+OK

SHUTDOWN
+OK`}
      />

      <h2 id="in-docker">In Docker</h2>
      <p>
        The image keeps the dump in the <code>/data</code> volume, so mount one:
      </p>
      <CodeBlock
        lang="bash"
        filename="terminal"
        code={`docker run -d -p 7171:7171 -v klyro-data:/data ghcr.io/hitesh-s0lanki/klyro:latest`}
      />
      <p>
        <code>docker stop</code> sends <code>SIGTERM</code>, and Klyro saves
        before exiting. Compose allows 30 seconds for that; raise{" "}
        <code>stop_grace_period</code> if a large keyspace needs longer.
      </p>

      <Callout variant="warning" title="Snapshot only">
        <p>
          There is no append-only log and no replication in 0.1.0. If losing up
          to a minute of writes is unacceptable, lower{" "}
          <code>save-interval</code>, and treat the dump as what it is: a
          periodic snapshot, not a durable transaction log.
        </p>
      </Callout>
    </>
  );
}
