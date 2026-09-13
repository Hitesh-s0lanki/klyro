from __future__ import annotations

import socket
import subprocess
import time
from collections.abc import Iterator
from pathlib import Path

import pytest

from klyro_db import Klyro


@pytest.fixture
def klyro(tmp_path: Path) -> Iterator[Klyro]:
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    binary = Path(__file__).parents[2] / "target" / "release" / "klyro"
    process = subprocess.Popen([binary, str(port), "test.dump"], cwd=tmp_path)
    client = Klyro(port=port, socket_connect_timeout=.2)
    for _ in range(50):
        try:
            client.ping()
            break
        except Exception:
            time.sleep(.05)
    else:
        process.terminate()
        raise RuntimeError("Klyro did not start")
    try:
        yield client
    finally:
        client.close()
        process.terminate()
        process.wait(timeout=5)
