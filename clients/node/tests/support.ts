// Test-only helper: spawns a real `klyro` server subprocess on its own
// port + temp dump file, and tears it down afterward. Mirrors the
// approach in ../../../tests/common/mod.rs (the Rust integration
// suite), so this client's tests exercise the actual wire protocol
// rather than a mock.

import { type ChildProcess, execSync, spawn } from "node:child_process";
import { existsSync, rmSync } from "node:fs";
import { connect as netConnect } from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

// `npm test`/`npm run build` run with cwd = clients/node, so the repo
// root is two levels up. Overridable via KLYRO_BIN for other setups.
const REPO_ROOT = resolve(process.cwd(), "..", "..");
const DEFAULT_BIN = join(REPO_ROOT, "target", "release", "klyro");
const SERVER_BIN = process.env.KLYRO_BIN ?? DEFAULT_BIN;

let ensured = false;
function ensureServerBuilt(): void {
  if (ensured) return;
  if (!existsSync(SERVER_BIN)) {
    execSync("cargo build --release", { cwd: REPO_ROOT, stdio: "inherit" });
  }
  if (!existsSync(SERVER_BIN)) {
    throw new Error(
      `klyro server binary not found at ${SERVER_BIN} even after 'cargo build --release'. ` +
        "Set KLYRO_BIN to override the path.",
    );
  }
  ensured = true;
}

// A monotonic counter rather than an OS-assigned ephemeral port: it's
// simpler and avoids any bind/free race, at the cost of needing a base
// unlikely to collide with anything else on the machine.
let nextPort = 27301;
function freePort(): number {
  return nextPort++;
}

export interface TestServer {
  readonly port: number;
  readonly dumpPath: string;
  /** Resolves once the child process has actually exited. */
  waitForExit(timeoutMs?: number): Promise<void>;
  /** Kills the server (if still running) and removes the dump file. */
  stop(): Promise<void>;
}

function cleanupDumpFiles(dumpPath: string): void {
  rmSync(dumpPath, { force: true });
  rmSync(`${dumpPath}.tmp`, { force: true });
}

export async function startServer(): Promise<TestServer> {
  ensureServerBuilt();
  const port = freePort();
  const dumpPath = join(tmpdir(), `klyro_node_client_test_${port}_${process.pid}.dump`);
  cleanupDumpFiles(dumpPath);

  const child: ChildProcess = spawn(SERVER_BIN, [String(port), dumpPath], {
    stdio: ["ignore", "pipe", "pipe"],
  });

  let exited = false;
  child.once("exit", () => {
    exited = true;
  });

  await waitForReady(port, () => exited);

  const waitForExit = (timeoutMs = 3000): Promise<void> => {
    if (exited) return Promise.resolve();
    return new Promise((resolvePromise, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`klyro on port ${port} did not exit within ${timeoutMs}ms`));
      }, timeoutMs);
      child.once("exit", () => {
        clearTimeout(timer);
        resolvePromise();
      });
    });
  };

  let stopped = false;
  const stop = async (): Promise<void> => {
    if (stopped) return;
    stopped = true;
    if (!exited) {
      child.kill();
      await waitForExit(2000).catch(() => {
        child.kill("SIGKILL");
      });
    }
    cleanupDumpFiles(dumpPath);
  };

  return { port, dumpPath, waitForExit, stop };
}

function waitForReady(port: number, hasExited: () => boolean): Promise<void> {
  const deadline = Date.now() + 3000;
  return new Promise((resolvePromise, reject) => {
    const attempt = (): void => {
      if (hasExited()) {
        reject(new Error(`klyro exited before becoming ready on port ${port}`));
        return;
      }
      const socket = netConnect({ host: "127.0.0.1", port });
      socket.once("connect", () => {
        socket.destroy();
        resolvePromise();
      });
      socket.once("error", () => {
        socket.destroy();
        if (Date.now() > deadline) {
          reject(new Error(`klyro on port ${port} never became ready`));
        } else {
          setTimeout(attempt, 30);
        }
      });
    };
    attempt();
  });
}
