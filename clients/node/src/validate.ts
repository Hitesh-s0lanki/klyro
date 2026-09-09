/**
 * Client-side argument validation. Klyro's wire protocol has no
 * escaping mechanism: a key/field/member with embedded whitespace
 * would silently split into extra arguments, and a SET/HSET value with
 * an embedded newline would be parsed as a second command. Rather than
 * send a malformed command line and get a confusing server-side `ERR`
 * (or, worse, silently corrupt data), we validate client-side and
 * throw a clear `TypeError` up front.
 */

const WHITESPACE_RE = /\s/;

/**
 * Validates a single whitespace-delimited token: a key, hash field,
 * set/zset member, or KEYS/SCAN pattern. Must be non-empty and contain
 * no whitespace.
 */
export function validateToken(label: string, value: string): void {
  if (value.length === 0) {
    throw new TypeError(`${label} must not be empty`);
  }
  if (WHITESPACE_RE.test(value)) {
    throw new TypeError(
      `${label} must not contain whitespace (the protocol has no escaping): ${JSON.stringify(value)}`,
    );
  }
}

/**
 * Validates a "rest of line" value (SET/HSET/APPEND/SETRANGE): spaces
 * are fine, but a newline would be parsed by the server as the start
 * of a second command.
 */
export function validateLineValue(label: string, value: string): void {
  if (value.includes("\n") || value.includes("\r")) {
    throw new TypeError(`${label} must not contain newlines: ${JSON.stringify(value)}`);
  }
}
