import { KlyroConnection } from "./connection.js";
import { validateLineValue, validateToken } from "./validate.js";

export interface KlyroClientOptions {
  /** @default "127.0.0.1" */
  host?: string;
  /** @default 7171 */
  port?: number;
  /** Per-command timeout, in milliseconds. @default 5000 */
  timeoutMs?: number;
}

export interface ScanOptions {
  /** Only return keys matching this glob pattern. */
  match?: string;
  /** Batch-size hint (server default is 10 if omitted). */
  count?: number;
}

export interface ScanResult {
  keys: string[];
  /** Pass this back as the next call's `cursor`; `0` means done. */
  cursor: number;
}

const VALID_TYPES = new Set(["STRING", "LIST", "HASH", "SET", "ZSET"]);

function unexpectedReply(command: string, line: string): Error {
  return new Error(`unexpected reply to ${command}: ${JSON.stringify(line)}`);
}

function okOrNotFound(command: string, line: string): boolean {
  if (line === "OK") return true;
  if (line === "NOT_FOUND") return false;
  throw unexpectedReply(command, line);
}

/** Parses a `"<PREFIX> <rest>"` line, returning `<rest>` (which may be empty). */
function parsePrefixed(command: string, prefix: string, line: string): string {
  if (!line.startsWith(prefix + " ")) throw unexpectedReply(command, line);
  return line.slice(prefix.length + 1);
}

function parsePrefixedInt(command: string, prefix: string, line: string): number {
  const raw = parsePrefixed(command, prefix, line);
  const n = Number(raw);
  if (!Number.isFinite(n)) throw unexpectedReply(command, line);
  return n;
}

function requireInteger(label: string, value: number): void {
  if (!Number.isInteger(value)) {
    throw new TypeError(`${label} must be an integer, got ${value}`);
  }
}

/**
 * Promise-based client for a Klyro server, backed by a single
 * `net.Socket`. One connection per instance, one command in flight at a
 * time (calls made concurrently are queued and sent in order - the
 * wire protocol carries no request id to demux out-of-order replies).
 *
 * ```ts
 * const client = new KlyroClient();
 * await client.connect();
 * await client.set("foo", "bar");
 * console.log(await client.get("foo")); // "bar"
 * await client.close();
 * ```
 */
export class KlyroClient {
  private readonly host: string;
  private readonly port: number;
  private readonly timeoutMs: number;
  private conn: KlyroConnection | null = null;

  constructor(options: KlyroClientOptions = {}) {
    this.host = options.host ?? "127.0.0.1";
    this.port = options.port ?? 7171;
    this.timeoutMs = options.timeoutMs ?? 5000;
  }

  /** Opens the TCP connection. Must be called (and awaited) before any command. */
  async connect(): Promise<void> {
    if (this.conn) return;
    const conn = new KlyroConnection(this.host, this.port, this.timeoutMs);
    await conn.connect();
    this.conn = conn;
  }

  /** Closes the connection. Safe to call more than once. */
  async close(): Promise<void> {
    const conn = this.conn;
    this.conn = null;
    if (conn) await conn.close();
  }

  private connection(): KlyroConnection {
    if (!this.conn) {
      throw new Error("KlyroClient is not connected - call connect() first");
    }
    return this.conn;
  }

  // ---------------------------------------------------------------------
  // Generic
  // ---------------------------------------------------------------------

  async ping(): Promise<void> {
    const [line] = await this.connection().send("PING", "line");
    if (line !== "PONG") throw unexpectedReply("PING", line!);
  }

  /** Deletes `key`. Resolves `true` if it existed, `false` otherwise. */
  async del(key: string): Promise<boolean> {
    validateToken("key", key);
    const [line] = await this.connection().send(`DEL ${key}`, "line");
    return okOrNotFound("DEL", line!);
  }

  /** Sets `key`'s TTL to `seconds` (signed). Resolves `false` if `key` is missing. */
  async expire(key: string, seconds: number): Promise<boolean> {
    validateToken("key", key);
    requireInteger("seconds", seconds);
    const [line] = await this.connection().send(`EXPIRE ${key} ${seconds}`, "line");
    return okOrNotFound("EXPIRE", line!);
  }

  /** `-2` if `key` is missing, `-1` if it has no expiry, else seconds remaining. */
  async ttl(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`TTL ${key}`, "line");
    return parsePrefixedInt("TTL", "TTL", line!);
  }

  /** `null` for a missing key (server's `NONE`), else `"STRING"|"LIST"|"HASH"|"SET"|"ZSET"`. */
  async typeOf(key: string): Promise<string | null> {
    validateToken("key", key);
    const [line] = await this.connection().send(`TYPE ${key}`, "line");
    if (line === "NONE") return null;
    if (VALID_TYPES.has(line!)) return line!;
    throw unexpectedReply("TYPE", line!);
  }

  /** Lists every key, or every key matching `pattern` (a glob: `* ? [abc] [a-z] [^abc] \\`). */
  async keys(pattern?: string): Promise<string[]> {
    if (pattern !== undefined) validateToken("pattern", pattern);
    const cmd = pattern !== undefined ? `KEYS ${pattern}` : "KEYS";
    return this.connection().send(cmd, "multiEnd");
  }

  /**
   * Iterates the keyspace. Start with `cursor = 0`; keep calling with
   * the returned `cursor` until it comes back `0` again, which means
   * the whole keyspace has been covered.
   */
  async scan(cursor: number, opts: ScanOptions = {}): Promise<ScanResult> {
    requireInteger("cursor", cursor);
    if (cursor < 0) throw new TypeError(`cursor must be >= 0, got ${cursor}`);

    let cmd = `SCAN ${cursor}`;
    if (opts.match !== undefined) {
      validateToken("match pattern", opts.match);
      cmd += ` MATCH ${opts.match}`;
    }
    if (opts.count !== undefined) {
      requireInteger("count", opts.count);
      if (opts.count <= 0) throw new TypeError(`count must be > 0, got ${opts.count}`);
      cmd += ` COUNT ${opts.count}`;
    }

    const lines = await this.connection().send(cmd, "multiScan");
    const last = lines[lines.length - 1];
    const match = last !== undefined ? /^CURSOR (\d+)$/.exec(last) : null;
    if (!match) throw unexpectedReply("SCAN", last ?? "");
    return { keys: lines.slice(0, -1), cursor: Number(match[1]) };
  }

  async dbsize(): Promise<number> {
    const [line] = await this.connection().send("DBSIZE", "line");
    return parsePrefixedInt("DBSIZE", "COUNT", line!);
  }

  /** Writes the dump file immediately. */
  async save(): Promise<void> {
    const [line] = await this.connection().send("SAVE", "line");
    if (line !== "OK") throw unexpectedReply("SAVE", line!);
  }

  /** Sends `QUIT`, then closes the connection. */
  async quit(): Promise<void> {
    const [line] = await this.connection().send("QUIT", "line");
    if (line !== "BYE") throw unexpectedReply("QUIT", line!);
    await this.close();
  }

  /** Tells the server to save and exit, then closes the connection. */
  async shutdown(): Promise<void> {
    const [line] = await this.connection().send("SHUTDOWN", "line");
    if (line !== "SHUTTING_DOWN") throw unexpectedReply("SHUTDOWN", line!);
    await this.close();
  }

  // ---------------------------------------------------------------------
  // String
  // ---------------------------------------------------------------------

  /** Sets `key` to `value` (may contain spaces, not newlines). Always clears any TTL. */
  async set(key: string, value: string): Promise<void> {
    validateToken("key", key);
    validateLineValue("value", value);
    const [line] = await this.connection().send(`SET ${key} ${value}`, "line");
    if (line !== "OK") throw unexpectedReply("SET", line!);
  }

  async get(key: string): Promise<string | null> {
    validateToken("key", key);
    const [line] = await this.connection().send(`GET ${key}`, "line");
    if (line === "NOT_FOUND") return null;
    return parsePrefixed("GET", "VALUE", line!);
  }

  /** Increments `key` (missing key starts at 0). Preserves any existing TTL. */
  async incr(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`INCR ${key}`, "line");
    return parsePrefixedInt("INCR", "VALUE", line!);
  }

  /** Decrements `key` (missing key starts at 0). Preserves any existing TTL. */
  async decr(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`DECR ${key}`, "line");
    return parsePrefixedInt("DECR", "VALUE", line!);
  }

  /** Appends `value` to `key` (creating it if missing). Resolves the new total length. */
  async append(key: string, value: string): Promise<number> {
    validateToken("key", key);
    validateLineValue("value", value);
    const [line] = await this.connection().send(`APPEND ${key} ${value}`, "line");
    return parsePrefixedInt("APPEND", "LEN", line!);
  }

  /** Inclusive substring; negative indices count from the end. Out-of-range yields `""`. */
  async getrange(key: string, start: number, end: number): Promise<string> {
    validateToken("key", key);
    requireInteger("start", start);
    requireInteger("end", end);
    const [line] = await this.connection().send(`GETRANGE ${key} ${start} ${end}`, "line");
    return parsePrefixed("GETRANGE", "VALUE", line!);
  }

  /** Writes `value` at `offset`, padding any gap with ASCII spaces. Resolves the new total length. */
  async setrange(key: string, offset: number, value: string): Promise<number> {
    validateToken("key", key);
    requireInteger("offset", offset);
    if (offset < 0) throw new TypeError(`offset must be >= 0, got ${offset}`);
    validateLineValue("value", value);
    const [line] = await this.connection().send(`SETRANGE ${key} ${offset} ${value}`, "line");
    return parsePrefixedInt("SETRANGE", "LEN", line!);
  }

  // ---------------------------------------------------------------------
  // List
  // ---------------------------------------------------------------------

  /** Pushes each value to the head in turn (`lpush(k, "a","b","c")` ends up `[c, b, a]`). */
  async lpush(key: string, ...values: string[]): Promise<number> {
    return this.pushCommand("LPUSH", key, values);
  }

  /** Pushes each value to the tail in turn (`rpush(k, "a","b","c")` ends up `[a, b, c]`). */
  async rpush(key: string, ...values: string[]): Promise<number> {
    return this.pushCommand("RPUSH", key, values);
  }

  private async pushCommand(cmd: "LPUSH" | "RPUSH", key: string, values: string[]): Promise<number> {
    validateToken("key", key);
    if (values.length === 0) throw new TypeError(`${cmd} requires at least one value`);
    values.forEach((v, i) => validateToken(`values[${i}]`, v));
    const [line] = await this.connection().send(`${cmd} ${key} ${values.join(" ")}`, "line");
    return parsePrefixedInt(cmd, "LEN", line!);
  }

  async lpop(key: string): Promise<string | null> {
    validateToken("key", key);
    const [line] = await this.connection().send(`LPOP ${key}`, "line");
    if (line === "NOT_FOUND") return null;
    return parsePrefixed("LPOP", "VALUE", line!);
  }

  async rpop(key: string): Promise<string | null> {
    validateToken("key", key);
    const [line] = await this.connection().send(`RPOP ${key}`, "line");
    if (line === "NOT_FOUND") return null;
    return parsePrefixed("RPOP", "VALUE", line!);
  }

  async llen(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`LLEN ${key}`, "line");
    return parsePrefixedInt("LLEN", "LEN", line!);
  }

  /** Inclusive range; negative indices count from the end. */
  async lrange(key: string, start: number, stop: number): Promise<string[]> {
    validateToken("key", key);
    requireInteger("start", start);
    requireInteger("stop", stop);
    return this.connection().send(`LRANGE ${key} ${start} ${stop}`, "multiEnd");
  }

  // ---------------------------------------------------------------------
  // Hash
  // ---------------------------------------------------------------------

  async hset(key: string, field: string, value: string): Promise<void> {
    validateToken("key", key);
    validateToken("field", field);
    validateLineValue("value", value);
    const [line] = await this.connection().send(`HSET ${key} ${field} ${value}`, "line");
    if (line !== "OK") throw unexpectedReply("HSET", line!);
  }

  async hget(key: string, field: string): Promise<string | null> {
    validateToken("key", key);
    validateToken("field", field);
    const [line] = await this.connection().send(`HGET ${key} ${field}`, "line");
    if (line === "NOT_FOUND") return null;
    return parsePrefixed("HGET", "VALUE", line!);
  }

  /** Deletes a field, resolving `false` if the key or field didn't exist. */
  async hdel(key: string, field: string): Promise<boolean> {
    validateToken("key", key);
    validateToken("field", field);
    const [line] = await this.connection().send(`HDEL ${key} ${field}`, "line");
    return okOrNotFound("HDEL", line!);
  }

  async hlen(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`HLEN ${key}`, "line");
    return parsePrefixedInt("HLEN", "LEN", line!);
  }

  async hgetall(key: string): Promise<Record<string, string>> {
    validateToken("key", key);
    const lines = await this.connection().send(`HGETALL ${key}`, "multiEnd");
    const result: Record<string, string> = {};
    for (let i = 0; i < lines.length; i += 2) {
      result[lines[i]!] = lines[i + 1]!;
    }
    return result;
  }

  // ---------------------------------------------------------------------
  // Set
  // ---------------------------------------------------------------------

  /** Resolves the count of members newly added (duplicates don't count). */
  async sadd(key: string, ...members: string[]): Promise<number> {
    validateToken("key", key);
    if (members.length === 0) throw new TypeError("SADD requires at least one member");
    members.forEach((m, i) => validateToken(`members[${i}]`, m));
    const [line] = await this.connection().send(`SADD ${key} ${members.join(" ")}`, "line");
    return parsePrefixedInt("SADD", "ADDED", line!);
  }

  /** Removes one member. Only one per call, unlike `sadd`. */
  async srem(key: string, member: string): Promise<boolean> {
    validateToken("key", key);
    validateToken("member", member);
    const [line] = await this.connection().send(`SREM ${key} ${member}`, "line");
    return okOrNotFound("SREM", line!);
  }

  async sismember(key: string, member: string): Promise<boolean> {
    validateToken("key", key);
    validateToken("member", member);
    const [line] = await this.connection().send(`SISMEMBER ${key} ${member}`, "line");
    if (line === "TRUE") return true;
    if (line === "FALSE") return false;
    throw unexpectedReply("SISMEMBER", line!);
  }

  async scard(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`SCARD ${key}`, "line");
    return parsePrefixedInt("SCARD", "LEN", line!);
  }

  async smembers(key: string): Promise<Set<string>> {
    validateToken("key", key);
    const lines = await this.connection().send(`SMEMBERS ${key}`, "multiEnd");
    return new Set(lines);
  }

  // ---------------------------------------------------------------------
  // Sorted set
  // ---------------------------------------------------------------------

  /**
   * Adds one or more (score, member) pairs, given as `[score, member]`
   * tuples (matching the wire order `score member`), e.g.:
   *
   * ```ts
   * await client.zadd("board", [100, "alice"], [50, "bob"]);
   * ```
   *
   * Up to 128 pairs per call. Resolves the count of members newly
   * added; repositioning an existing member's score doesn't count.
   */
  async zadd(key: string, ...pairs: Array<[number, string]>): Promise<number> {
    validateToken("key", key);
    if (pairs.length === 0) throw new TypeError("ZADD requires at least one [score, member] pair");
    const parts: string[] = [];
    pairs.forEach(([score, member], i) => {
      if (typeof score !== "number" || !Number.isFinite(score)) {
        throw new TypeError(`pairs[${i}][0] (score) must be a finite number, got ${score}`);
      }
      validateToken(`pairs[${i}][1] (member)`, member);
      parts.push(String(score), member);
    });
    const [line] = await this.connection().send(`ZADD ${key} ${parts.join(" ")}`, "line");
    return parsePrefixedInt("ZADD", "ADDED", line!);
  }

  async zscore(key: string, member: string): Promise<number | null> {
    validateToken("key", key);
    validateToken("member", member);
    const [line] = await this.connection().send(`ZSCORE ${key} ${member}`, "line");
    if (line === "NOT_FOUND") return null;
    const raw = parsePrefixed("ZSCORE", "VALUE", line!);
    const n = Number(raw);
    if (!Number.isFinite(n)) throw unexpectedReply("ZSCORE", line!);
    return n;
  }

  /** Removes one member. Only one per call, unlike `zadd`. */
  async zrem(key: string, member: string): Promise<boolean> {
    validateToken("key", key);
    validateToken("member", member);
    const [line] = await this.connection().send(`ZREM ${key} ${member}`, "line");
    return okOrNotFound("ZREM", line!);
  }

  async zcard(key: string): Promise<number> {
    validateToken("key", key);
    const [line] = await this.connection().send(`ZCARD ${key}`, "line");
    return parsePrefixedInt("ZCARD", "LEN", line!);
  }

  /** Inclusive range, ascending by score; negative indices count from the end. */
  async zrange(key: string, start: number, stop: number): Promise<Array<[string, number]>> {
    validateToken("key", key);
    requireInteger("start", start);
    requireInteger("stop", stop);
    const lines = await this.connection().send(`ZRANGE ${key} ${start} ${stop}`, "multiEnd");
    return lines.map((line) => {
      const idx = line.indexOf(" ");
      if (idx === -1) throw unexpectedReply("ZRANGE", line);
      const member = line.slice(0, idx);
      const score = Number(line.slice(idx + 1));
      if (!Number.isFinite(score)) throw unexpectedReply("ZRANGE", line);
      return [member, score] as [string, number];
    });
  }
}
