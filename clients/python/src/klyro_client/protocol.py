"""Low-level wire-protocol helpers: line buffering and token validation.

Kept separate from :mod:`klyro_client.client` so the "how do I read a
CRLF-terminated line off a socket without assuming one recv() == one
reply" logic and the "what's a legal key/member token" logic are each
easy to find and unit-test in isolation from the command surface.
"""

from __future__ import annotations

import socket

#: Server-side single-reply cap (see the "Known limitations" section of
#: the top-level README); used only to size our recv buffer sensibly.
_RECV_CHUNK = 8192


class LineReader:
    """Buffers bytes off a socket and yields one ``\\r\\n``-terminated
    line (with the terminator stripped) at a time.

    Does not assume a single ``recv()`` call returns exactly one reply:
    a large multi-line reply (KEYS, LRANGE, ...) may arrive over several
    ``recv()`` calls, and conversely several small replies (or a
    multi-line reply's several lines) can arrive in one ``recv()`` and
    must be doled out one line per :meth:`read_line` call, buffering the
    remainder for the next one.
    """

    __slots__ = ("_sock", "_buf")

    def __init__(self, sock: socket.socket) -> None:
        self._sock = sock
        self._buf = b""

    def read_line(self) -> str:
        while b"\n" not in self._buf:
            chunk = self._sock.recv(_RECV_CHUNK)
            if not chunk:
                raise ConnectionError(
                    "connection closed by the Klyro server while reading a reply"
                )
            self._buf += chunk
        line, _, self._buf = self._buf.partition(b"\n")
        if line.endswith(b"\r"):
            line = line[:-1]
        return line.decode("utf-8", errors="replace")

    def reset(self) -> None:
        """Drops any buffered bytes. Only meaningful right after a fresh
        connect; not used mid-session."""
        self._buf = b""


def validate_token(name: str, value: str) -> str:
    """Validates a key/field/member argument: must be non-empty and
    contain no whitespace (the server has no escaping mechanism, so a
    stray space or newline would be silently parsed as extra/garbled
    command arguments rather than raising a clear client-side error).
    """
    if value == "":
        raise ValueError(f"{name} must not be empty")
    if any(ch.isspace() for ch in value):
        raise ValueError(f"{name} must not contain whitespace: {value!r}")
    return value


def validate_line_value(name: str, value: str) -> str:
    """Validates a SET/HSET-style value: may contain spaces (it's "rest
    of the line") but never a newline, since the protocol is
    line-oriented and an embedded newline would be parsed as the start
    of a second command.
    """
    if "\n" in value or "\r" in value:
        raise ValueError(f"{name} must not contain newlines: {value!r}")
    return value


def format_score(score: int | float) -> str:
    """Formats a ZADD score as a single whitespace-free token.

    Renders whole-number scores without a trailing ``.0`` (purely
    cosmetic - the server's f64 parser accepts either form) and
    otherwise falls back to ``repr()``, which round-trips any finite
    float without scientific notation for the range Klyro scores
    realistically take.
    """
    if isinstance(score, int):
        return str(score)
    if score.is_integer():
        return str(int(score))
    return repr(score)
