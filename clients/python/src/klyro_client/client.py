"""Synchronous TCP client for Klyro.

One connection per :class:`KlyroClient` instance, request-then-await-reply
(no pipelining), fail-fast on connection loss (no auto-reconnect) - see
``docs/client-libraries.md`` in the main repo for the reasoning behind
those choices. Zero runtime dependencies: only the stdlib ``socket``
module is used.
"""

from __future__ import annotations

import socket
from typing import Iterable

from .exceptions import KlyroError, WrongTypeError
from .protocol import LineReader, format_score, validate_line_value, validate_token

DEFAULT_HOST = "127.0.0.1"
DEFAULT_PORT = 7171
DEFAULT_TIMEOUT = 5.0

#: Max score/member pairs ZADD accepts in one call (see docs/README and
#: commands.rs); enforced client-side too so callers get a clear,
#: local ValueError instead of a round trip just to learn the same
#: thing from an ERR reply.
_MAX_ZADD_PAIRS = 128


class KlyroClient:
    """A synchronous client for a Klyro server.

    Does not connect on construction - call :meth:`connect` (or use the
    instance as a context manager) before issuing commands::

        with KlyroClient() as c:
            c.set("foo", "bar")
            c.get("foo")  # -> "bar"
    """

    def __init__(
        self,
        host: str = DEFAULT_HOST,
        port: int = DEFAULT_PORT,
        timeout: float | None = DEFAULT_TIMEOUT,
    ) -> None:
        self.host = host
        self.port = port
        self.timeout = timeout
        self._sock: socket.socket | None = None
        self._reader: LineReader | None = None

    # -- connection lifecycle -------------------------------------------------

    def connect(self) -> "KlyroClient":
        """Opens the TCP connection. A no-op if already connected."""
        if self._sock is not None:
            return self
        sock = socket.create_connection((self.host, self.port), timeout=self.timeout)
        self._sock = sock
        self._reader = LineReader(sock)
        return self

    def close(self) -> None:
        """Closes the connection, if open. Safe to call more than once."""
        sock, self._sock = self._sock, None
        self._reader = None
        if sock is not None:
            try:
                sock.close()
            except OSError:
                pass

    @property
    def is_connected(self) -> bool:
        return self._sock is not None

    def __enter__(self) -> "KlyroClient":
        self.connect()
        return self

    def __exit__(self, *exc_info: object) -> None:
        self.close()

    def __repr__(self) -> str:
        state = "connected" if self.is_connected else "disconnected"
        return f"KlyroClient({self.host!r}, {self.port!r}, {state})"

    # -- wire helpers -----------------------------------------------------

    def _require_conn(self) -> tuple[socket.socket, LineReader]:
        if self._sock is None or self._reader is None:
            raise KlyroError(
                "not connected: call .connect() first (or use `with KlyroClient() as c:`)"
            )
        return self._sock, self._reader

    @staticmethod
    def _raise_for_error(line: str) -> None:
        # Every real error reply is "ERR <message>" (always a space after
        # ERR) - checking for that space avoids misreading a data line
        # that merely *starts with* "ERR" (e.g. a key named "ERRlog" as
        # the first line of a KEYS/SMEMBERS/... reply) as an error.
        if line.startswith("ERR "):
            if line.startswith("ERR WRONGTYPE"):
                raise WrongTypeError(line)
            raise KlyroError(line)

    def _send(self, line: str) -> None:
        sock, _ = self._require_conn()
        try:
            sock.sendall(line.encode("utf-8") + b"\r\n")
        except OSError as exc:
            raise ConnectionError(f"failed to send command to Klyro: {exc}") from exc

    def _command(self, line: str) -> str:
        """Sends `line` and returns the single reply line, raising on ERR."""
        self._send(line)
        _, reader = self._require_conn()
        reply = reader.read_line()
        self._raise_for_error(reply)
        return reply

    def _multi_command(
        self, line: str, *, terminator: str | None = None, terminator_prefix: str | None = None
    ) -> tuple[list[str], str]:
        """Sends `line` and collects a multi-line reply.

        Reads lines one at a time. If the *first* line starts with
        "ERR", that is the whole reply (raised as an error) - these
        commands do not send a trailing terminator on error. Otherwise
        keeps collecting lines until one equals `terminator` (KEYS,
        LRANGE, HGETALL, SMEMBERS, ZRANGE all use the literal "END") or
        starts with `terminator_prefix` (SCAN uses "CURSOR "). Returns
        (data_lines, terminator_line).
        """
        assert (terminator is None) != (terminator_prefix is None), (
            "exactly one of terminator/terminator_prefix must be given"
        )
        self._send(line)
        _, reader = self._require_conn()

        first = reader.read_line()
        self._raise_for_error(first)

        data_lines: list[str] = []
        current = first
        while True:
            if terminator is not None and current == terminator:
                return data_lines, current
            if terminator_prefix is not None and current.startswith(terminator_prefix):
                return data_lines, current
            data_lines.append(current)
            current = reader.read_line()

    @staticmethod
    def _parse_value(reply: str) -> str:
        if not reply.startswith("VALUE "):
            raise KlyroError(f"unexpected reply, expected VALUE ...: {reply!r}")
        return reply[len("VALUE ") :]

    @staticmethod
    def _parse_int(reply: str, prefix: str) -> int:
        if not reply.startswith(prefix):
            raise KlyroError(f"unexpected reply, expected {prefix!r}...: {reply!r}")
        rest = reply[len(prefix) :]
        try:
            return int(rest)
        except ValueError as exc:
            raise KlyroError(f"unexpected reply, non-integer after {prefix!r}: {reply!r}") from exc

    @staticmethod
    def _bool_from_ok_or_not_found(reply: str) -> bool:
        if reply == "OK":
            return True
        if reply == "NOT_FOUND":
            return False
        raise KlyroError(f"unexpected reply, expected OK/NOT_FOUND: {reply!r}")

    # -- generic (any type) -------------------------------------------------

    def ping(self) -> bool:
        return self._command("PING") == "PONG"

    def delete(self, key: str) -> bool:
        """`DEL key`. Returns True if the key existed and was removed."""
        validate_token("key", key)
        return self._bool_from_ok_or_not_found(self._command(f"DEL {key}"))

    def expire(self, key: str, seconds: int) -> bool:
        validate_token("key", key)
        return self._bool_from_ok_or_not_found(self._command(f"EXPIRE {key} {int(seconds)}"))

    def ttl(self, key: str) -> int:
        """`TTL key` -> seconds remaining, -1 = no expiry, -2 = missing key."""
        validate_token("key", key)
        return self._parse_int(self._command(f"TTL {key}"), "TTL ")

    def type_of(self, key: str) -> str | None:
        """`TYPE key`. Returns one of "STRING"/"LIST"/"HASH"/"SET"/"ZSET",
        or None if the key is missing (server replies "NONE")."""
        validate_token("key", key)
        reply = self._command(f"TYPE {key}")
        return None if reply == "NONE" else reply

    def keys(self, pattern: str | None = None) -> list[str]:
        """`KEYS [pattern]`. No pattern (or None) returns every key."""
        line = "KEYS" if pattern is None else f"KEYS {pattern}"
        data, _ = self._multi_command(line, terminator="END")
        return data

    def scan(
        self, cursor: int, match: str | None = None, count: int | None = None
    ) -> tuple[list[str], int]:
        """`SCAN cursor [MATCH pattern] [COUNT count]`.

        Returns (keys, next_cursor). Start with cursor=0 and keep
        passing back next_cursor until it comes back 0 again, meaning
        the whole keyspace has been covered.
        """
        parts = [f"SCAN {int(cursor)}"]
        if match is not None:
            parts.append(f"MATCH {match}")
        if count is not None:
            parts.append(f"COUNT {int(count)}")
        data, terminator = self._multi_command(" ".join(parts), terminator_prefix="CURSOR ")
        next_cursor = int(terminator[len("CURSOR ") :])
        return data, next_cursor

    def dbsize(self) -> int:
        return self._parse_int(self._command("DBSIZE"), "COUNT ")

    def save(self) -> None:
        reply = self._command("SAVE")
        if reply != "OK":
            raise KlyroError(f"unexpected reply to SAVE: {reply!r}")

    def quit(self) -> None:
        """`QUIT`. Server replies BYE and closes the connection; this
        closes our socket too."""
        try:
            reply = self._command("QUIT")
            if reply != "BYE":
                raise KlyroError(f"unexpected reply to QUIT: {reply!r}")
        finally:
            self.close()

    def shutdown(self) -> None:
        """`SHUTDOWN`. Server saves, replies SHUTTING_DOWN, and exits;
        treat the connection as gone afterward, so this closes our
        socket too."""
        try:
            reply = self._command("SHUTDOWN")
            if reply != "SHUTTING_DOWN":
                raise KlyroError(f"unexpected reply to SHUTDOWN: {reply!r}")
        finally:
            self.close()

    # -- string ---------------------------------------------------------

    def set(self, key: str, value: str) -> None:
        """`SET key value`. Always clears any existing TTL on `key`."""
        validate_token("key", key)
        validate_line_value("value", value)
        reply = self._command(f"SET {key} {value}")
        if reply != "OK":
            raise KlyroError(f"unexpected reply to SET: {reply!r}")

    def get(self, key: str) -> str | None:
        validate_token("key", key)
        reply = self._command(f"GET {key}")
        if reply == "NOT_FOUND":
            return None
        return self._parse_value(reply)

    def incr(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"INCR {key}"), "VALUE ")

    def decr(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"DECR {key}"), "VALUE ")

    def append(self, key: str, value: str) -> int:
        """`APPEND key value` -> new total length. Creates the key if
        missing; preserves any existing TTL."""
        validate_token("key", key)
        validate_line_value("value", value)
        return self._parse_int(self._command(f"APPEND {key} {value}"), "LEN ")

    def getrange(self, key: str, start: int, end: int) -> str:
        """Inclusive range; negative indices count from the end.
        Out-of-range returns "" rather than raising."""
        validate_token("key", key)
        reply = self._command(f"GETRANGE {key} {int(start)} {int(end)}")
        return self._parse_value(reply)

    def setrange(self, key: str, offset: int, value: str) -> int:
        """Pads any gap before `offset` with ASCII spaces; preserves TTL."""
        validate_token("key", key)
        validate_line_value("value", value)
        return self._parse_int(self._command(f"SETRANGE {key} {int(offset)} {value}"), "LEN ")

    # -- list -------------------------------------------------------------

    def lpush(self, key: str, *values: str) -> int:
        """`LPUSH key value [value ...]` -> new length.

        Pushes each value to the head in turn, so
        `lpush("k", "a", "b", "c")` leaves the list as [c, b, a].
        """
        return self._push("LPUSH", key, values)

    def rpush(self, key: str, *values: str) -> int:
        """`RPUSH key value [value ...]` -> new length.

        Pushes to the tail, so `rpush("k", "a", "b", "c")` leaves the
        list as [a, b, c].
        """
        return self._push("RPUSH", key, values)

    def _push(self, cmd: str, key: str, values: Iterable[str]) -> int:
        validate_token("key", key)
        values = list(values)
        if not values:
            raise ValueError(f"{cmd} requires at least one value")
        for v in values:
            validate_token("value", v)
        return self._parse_int(self._command(f"{cmd} {key} {' '.join(values)}"), "LEN ")

    def lpop(self, key: str) -> str | None:
        validate_token("key", key)
        reply = self._command(f"LPOP {key}")
        if reply == "NOT_FOUND":
            return None
        return self._parse_value(reply)

    def rpop(self, key: str) -> str | None:
        validate_token("key", key)
        reply = self._command(f"RPOP {key}")
        if reply == "NOT_FOUND":
            return None
        return self._parse_value(reply)

    def llen(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"LLEN {key}"), "LEN ")

    def lrange(self, key: str, start: int, stop: int) -> list[str]:
        """Inclusive range; negative indices count from the end."""
        validate_token("key", key)
        data, _ = self._multi_command(f"LRANGE {key} {int(start)} {int(stop)}", terminator="END")
        return data

    # -- hash ---------------------------------------------------------------

    def hset(self, key: str, field: str, value: str) -> None:
        validate_token("key", key)
        validate_token("field", field)
        validate_line_value("value", value)
        reply = self._command(f"HSET {key} {field} {value}")
        if reply != "OK":
            raise KlyroError(f"unexpected reply to HSET: {reply!r}")

    def hget(self, key: str, field: str) -> str | None:
        validate_token("key", key)
        validate_token("field", field)
        reply = self._command(f"HGET {key} {field}")
        if reply == "NOT_FOUND":
            return None
        return self._parse_value(reply)

    def hdel(self, key: str, field: str) -> bool:
        validate_token("key", key)
        validate_token("field", field)
        return self._bool_from_ok_or_not_found(self._command(f"HDEL {key} {field}"))

    def hlen(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"HLEN {key}"), "LEN ")

    def hgetall(self, key: str) -> dict[str, str]:
        validate_token("key", key)
        data, _ = self._multi_command(f"HGETALL {key}", terminator="END")
        if len(data) % 2 != 0:
            raise KlyroError(f"malformed HGETALL reply (odd number of lines): {data!r}")
        return dict(zip(data[0::2], data[1::2]))

    # -- set ------------------------------------------------------------

    def sadd(self, key: str, *members: str) -> int:
        """`SADD key member [member ...]` -> count of members newly
        added (duplicates don't count)."""
        validate_token("key", key)
        members = tuple(members)
        if not members:
            raise ValueError("SADD requires at least one member")
        for m in members:
            validate_token("member", m)
        return self._parse_int(self._command(f"SADD {key} {' '.join(members)}"), "ADDED ")

    def srem(self, key: str, member: str) -> bool:
        """`SREM key member`. Only one member per call, unlike SADD."""
        validate_token("key", key)
        validate_token("member", member)
        return self._bool_from_ok_or_not_found(self._command(f"SREM {key} {member}"))

    def sismember(self, key: str, member: str) -> bool:
        validate_token("key", key)
        validate_token("member", member)
        reply = self._command(f"SISMEMBER {key} {member}")
        if reply == "TRUE":
            return True
        if reply == "FALSE":
            return False
        raise KlyroError(f"unexpected reply to SISMEMBER: {reply!r}")

    def scard(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"SCARD {key}"), "LEN ")

    def smembers(self, key: str) -> set[str]:
        validate_token("key", key)
        data, _ = self._multi_command(f"SMEMBERS {key}", terminator="END")
        return set(data)

    # -- sorted set -------------------------------------------------------

    def zadd(self, key: str, *pairs: tuple[float, str]) -> int:
        """`ZADD key score member [score member ...]`.

        Takes one or more (score, member) tuples, e.g.::

            client.zadd("board", (100, "alice"), (95.5, "bob"))

        Returns the count of members newly added - repositioning an
        existing member's score doesn't count. Up to 128 pairs per
        call (enforced client-side to fail fast, matching the server).
        """
        validate_token("key", key)
        pairs = tuple(pairs)
        if not pairs:
            raise ValueError("ZADD requires at least one (score, member) pair")
        if len(pairs) > _MAX_ZADD_PAIRS:
            raise ValueError(
                f"ZADD accepts at most {_MAX_ZADD_PAIRS} score/member pairs per call, "
                f"got {len(pairs)}"
            )
        tokens: list[str] = []
        for score, member in pairs:
            validate_token("member", member)
            tokens.append(format_score(score))
            tokens.append(member)
        return self._parse_int(self._command(f"ZADD {key} {' '.join(tokens)}"), "ADDED ")

    def zscore(self, key: str, member: str) -> float | None:
        validate_token("key", key)
        validate_token("member", member)
        reply = self._command(f"ZSCORE {key} {member}")
        if reply == "NOT_FOUND":
            return None
        return float(self._parse_value(reply))

    def zrem(self, key: str, member: str) -> bool:
        """`ZREM key member`. Only one member per call."""
        validate_token("key", key)
        validate_token("member", member)
        return self._bool_from_ok_or_not_found(self._command(f"ZREM {key} {member}"))

    def zcard(self, key: str) -> int:
        validate_token("key", key)
        return self._parse_int(self._command(f"ZCARD {key}"), "LEN ")

    def zrange(self, key: str, start: int, stop: int) -> list[tuple[str, float]]:
        """Inclusive range, ascending by score; negative indices count
        from the end. Returns a list of (member, score) tuples."""
        validate_token("key", key)
        data, _ = self._multi_command(f"ZRANGE {key} {int(start)} {int(stop)}", terminator="END")
        result: list[tuple[str, float]] = []
        for line in data:
            member, _, score = line.rpartition(" ")
            if not member:
                raise KlyroError(f"malformed ZRANGE line: {line!r}")
            result.append((member, float(score)))
        return result
