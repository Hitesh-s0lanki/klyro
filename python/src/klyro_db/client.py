from __future__ import annotations

from typing import Any

from redis import Redis

from .memory import MemoryClient


class Klyro(Redis):
    """Redis-compatible client with typed helpers for Klyro memory commands."""

    memory: MemoryClient

    def __init__(self, host: str = "127.0.0.1", port: int = 7171, **kwargs: Any) -> None:
        # Binary replies preserve vectors; MemoryClient decodes textual fields.
        kwargs["decode_responses"] = False
        super().__init__(host=host, port=port, **kwargs)
        self.memory = MemoryClient(self)
