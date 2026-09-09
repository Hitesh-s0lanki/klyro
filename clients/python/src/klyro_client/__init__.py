"""klyro_client: the official Python client for Klyro.

    from klyro_client import KlyroClient

    with KlyroClient() as c:          # defaults to 127.0.0.1:7171
        c.set("foo", "bar")
        c.get("foo")                  # -> "bar"

See the package README for a fuller quick-start and error-handling notes.
"""

from __future__ import annotations

from .client import DEFAULT_HOST, DEFAULT_PORT, DEFAULT_TIMEOUT, KlyroClient
from .exceptions import KlyroError, WrongTypeError

__all__ = [
    "KlyroClient",
    "KlyroError",
    "WrongTypeError",
    "DEFAULT_HOST",
    "DEFAULT_PORT",
    "DEFAULT_TIMEOUT",
]

__version__ = "0.1.0"
