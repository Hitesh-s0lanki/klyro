/**
 * Base error for any `ERR ...` reply from the Klyro server. `message`
 * is the raw reply line as sent by the server (e.g. `"ERR unknown
 * command"`), so callers who need the exact server text can read it
 * off `error.message` directly.
 */
export class KlyroError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "KlyroError";
    // Restores `instanceof` across the ES2022 class-extends-Error edge
    // case some transpilation targets are known to break.
    Object.setPrototypeOf(this, KlyroError.prototype);
  }
}

/**
 * Thrown specifically for `ERR WRONGTYPE ...` replies: a command was
 * run against a key already holding a different data type (e.g.
 * `LPUSH` on a key created by `SET`). Callers that want to special-case
 * this (vs. a generic usage error) can `catch` and check
 * `instanceof WrongTypeError`.
 */
export class WrongTypeError extends KlyroError {
  constructor(message: string) {
    super(message);
    this.name = "WrongTypeError";
    Object.setPrototypeOf(this, WrongTypeError.prototype);
  }
}

/** Builds the right error subclass for a raw `"ERR ..."` reply line. */
export function errorFromLine(line: string): KlyroError {
  if (line.startsWith("ERR WRONGTYPE")) {
    return new WrongTypeError(line);
  }
  return new KlyroError(line);
}
