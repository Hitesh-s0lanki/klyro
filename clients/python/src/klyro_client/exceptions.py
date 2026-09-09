"""Exception types raised by :mod:`klyro_client`."""

from __future__ import annotations


class KlyroError(Exception):
    """Raised whenever the server replies with an ``ERR ...`` line.

    The raw, unparsed text of the server's error reply (e.g.
    ``"ERR unknown command"``) is available both as ``args[0]`` (so
    ``str(exc)`` shows it) and as the ``raw`` attribute.
    """

    def __init__(self, raw: str) -> None:
        super().__init__(raw)
        self.raw = raw


class WrongTypeError(KlyroError):
    """Raised for the specific ``ERR WRONGTYPE ...`` reply.

    A subclass of :class:`KlyroError` so callers that only care about
    generic errors can keep catching ``KlyroError``, while callers that
    want to special-case "wrong type" can catch this instead.
    """
