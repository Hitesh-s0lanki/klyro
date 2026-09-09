import { Socket, connect as netConnect } from "node:net";
import { errorFromLine } from "./errors.js";

/**
 * How to know a reply is complete:
 *  - "line": exactly one reply line.
 *  - "multiEnd": zero or more data lines, terminated by a literal
 *    "END" line (KEYS, LRANGE, HGETALL, SMEMBERS, ZRANGE).
 *  - "multiScan": zero or more data lines, terminated by a
 *    "CURSOR <n>" line (SCAN).
 *
 * For both multi-line modes, the server instead replies with a single
 * "ERR ..." line (no terminator) on failure - see `deliverLine` below.
 */
export type ReplyMode = "line" | "multiEnd" | "multiScan";

interface PendingRequest {
  mode: ReplyMode;
  lines: string[];
  resolve: (lines: string[]) => void;
  reject: (err: Error) => void;
  timer: NodeJS.Timeout;
}

/**
 * Low-level line-oriented transport for one Klyro connection.
 *
 * Responsibilities kept deliberately narrow: buffer partial TCP reads
 * and split them into "\r\n"-terminated lines (a single `data` event is
 * *not* guaranteed to line up with a reply, or even be a whole line),
 * detect where one reply ends per `ReplyMode`, and serialize commands
 * one at a time since the protocol carries no request id to demux
 * out-of-order replies. Command construction and reply parsing belong
 * to `KlyroClient`, not here.
 */
export class KlyroConnection {
  private socket: Socket | null = null;
  private buffer = "";
  private current: PendingRequest | null = null;
  private sendChain: Promise<unknown> = Promise.resolve();
  private closed = false;
  private closeError: Error | null = null;

  constructor(
    private readonly host: string,
    private readonly port: number,
    private readonly timeoutMs: number,
  ) {}

  connect(): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const socket = netConnect({ host: this.host, port: this.port });

      const onConnectError = (err: Error): void => {
        socket.destroy();
        reject(err);
      };
      socket.once("error", onConnectError);
      socket.once("connect", () => {
        socket.removeListener("error", onConnectError);
        socket.on("data", (chunk: Buffer) => this.onData(chunk));
        socket.on("error", (err) => this.fail(err));
        socket.on("close", () => this.fail(this.closeError ?? new Error("connection closed")));
        this.socket = socket;
        resolve();
      });
    });
  }

  /** Sends one command line and resolves with the parsed reply lines. */
  send(command: string, mode: ReplyMode): Promise<string[]> {
    const run = (): Promise<string[]> => this.sendNow(command, mode);
    const result = this.sendChain.then(run, run);
    // Keep the chain alive regardless of outcome so later commands
    // still run after an earlier one rejected.
    this.sendChain = result.then(
      () => undefined,
      () => undefined,
    );
    return result;
  }

  private sendNow(command: string, mode: ReplyMode): Promise<string[]> {
    if (this.closed) {
      return Promise.reject(this.closeError ?? new Error("connection is closed"));
    }
    const socket = this.socket;
    if (!socket) {
      return Promise.reject(new Error("not connected: call connect() first"));
    }
    return new Promise<string[]>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.fail(new Error(`Klyro command timed out after ${this.timeoutMs}ms: ${command.split(" ")[0]}`));
      }, this.timeoutMs);
      this.current = { mode, lines: [], resolve, reject, timer };
      socket.write(command + "\r\n", "utf8", (err) => {
        if (err) this.fail(err);
      });
    });
  }

  private onData(chunk: Buffer): void {
    this.buffer += chunk.toString("utf8");
    let idx: number;
    while ((idx = this.buffer.indexOf("\r\n")) !== -1) {
      const line = this.buffer.slice(0, idx);
      this.buffer = this.buffer.slice(idx + 2);
      this.deliverLine(line);
    }
  }

  private deliverLine(line: string): void {
    const req = this.current;
    if (!req) return; // nothing waiting (e.g. stray data after a timeout); drop it

    // The one-off error shape: on failure, KEYS/LRANGE/HGETALL/SMEMBERS/
    // ZRANGE/SCAN (and every single-line command) reply with a single
    // "ERR ..." line and no terminator. Only the *first* line of a
    // reply can be this - a later data line that happens to start with
    // "ERR" (e.g. a key literally named "ERRors") must not be
    // misparsed, hence `req.lines.length === 0` and requiring the
    // space after "ERR" that every real error line has.
    if (req.lines.length === 0 && line.startsWith("ERR ")) {
      this.finish(req, undefined, line);
      return;
    }

    if (req.mode === "line") {
      this.finish(req, [line]);
      return;
    }

    if (req.mode === "multiEnd") {
      if (line === "END") {
        this.finish(req, req.lines);
      } else {
        req.lines.push(line);
      }
      return;
    }

    // multiScan
    if (line.startsWith("CURSOR ")) {
      req.lines.push(line);
      this.finish(req, req.lines);
    } else {
      req.lines.push(line);
    }
  }

  private finish(req: PendingRequest, lines: string[] | undefined, errLine?: string): void {
    if (this.current !== req) return;
    clearTimeout(req.timer);
    this.current = null;
    if (errLine !== undefined) {
      req.reject(errorFromLine(errLine));
    } else {
      req.resolve(lines ?? []);
    }
  }

  /** Tears the connection down and rejects any in-flight request. */
  private fail(err: Error): void {
    if (this.closed) return;
    this.closed = true;
    this.closeError = err;
    const req = this.current;
    this.current = null;
    if (req) {
      clearTimeout(req.timer);
      req.reject(err);
    }
    this.socket?.destroy();
  }

  /** Closes the connection. Idempotent. */
  async close(): Promise<void> {
    if (this.closed) return;
    const socket = this.socket;
    await new Promise<void>((resolve) => {
      if (socket) socket.once("close", () => resolve());
      // Rejects any in-flight request and calls socket.destroy(), which
      // is guaranteed to eventually emit "close".
      this.fail(new Error("connection closed"));
      if (!socket) resolve();
    });
  }
}
