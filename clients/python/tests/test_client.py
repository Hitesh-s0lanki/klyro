"""Integration tests for klyro_client.

These spawn the real, compiled `klyro` server binary as a subprocess and
talk to it over actual TCP - no mocking, since the whole point of this
suite is protocol fidelity against the real server (mirroring the
approach in the main repo's own tests/common/mod.rs).

Run with either:
    python3 -m unittest discover -s tests
    python3 -m pytest tests/
"""

from __future__ import annotations

import itertools
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path

# Make the package importable without requiring an editable install first
# (works either way: if klyro_client is already installed, this is a
# no-op fallback path).
_SRC_DIR = Path(__file__).resolve().parents[1] / "src"
try:
    import klyro_client  # noqa: F401
except ImportError:
    sys.path.insert(0, str(_SRC_DIR))

from klyro_client import KlyroClient, KlyroError, WrongTypeError  # noqa: E402

REPO_ROOT = Path(__file__).resolve().parents[3]
BINARY = REPO_ROOT / "target" / "release" / "klyro"

_port_counter = itertools.count(27700)
_port_lock = threading.Lock()


def _next_port() -> int:
    with _port_lock:
        return next(_port_counter)


def setUpModule() -> None:
    if not BINARY.exists():
        subprocess.run(["cargo", "build", "--release"], cwd=str(REPO_ROOT), check=True)
    if not BINARY.exists():
        raise RuntimeError(f"klyro binary still missing at {BINARY} after cargo build --release")


class KlyroServerProcess:
    """Spawns a real klyro server subprocess on its own port and a
    private temp dump file. A context manager so the subprocess is
    always killed and the dump file always cleaned up, even if the
    test using it fails - no manual try/finally scattered per test.
    """

    def __init__(self) -> None:
        self.port = _next_port()
        fd, path = tempfile.mkstemp(prefix=f"klyro_test_{self.port}_", suffix=".dump")
        os.close(fd)
        os.remove(path)  # klyro creates/writes it itself; start from a clean slate
        self.dump_path = Path(path)
        self.process: subprocess.Popen | None = None

    def __enter__(self) -> "KlyroServerProcess":
        self.process = subprocess.Popen(
            [str(BINARY), str(self.port), str(self.dump_path)],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        self._wait_until_ready()
        return self

    def _wait_until_ready(self, timeout: float = 5.0) -> None:
        deadline = time.monotonic() + timeout
        assert self.process is not None
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                out, err = self.process.communicate()
                raise RuntimeError(
                    f"klyro exited early (code {self.process.returncode}): "
                    f"{out.decode(errors='replace')} {err.decode(errors='replace')}"
                )
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.2):
                    return
            except OSError:
                time.sleep(0.05)
        raise RuntimeError(f"klyro on port {self.port} never became ready")

    def __exit__(self, *exc_info: object) -> None:
        if self.process is not None:
            if self.process.poll() is None:
                self.process.kill()
                self.process.wait(timeout=5)
            self.process = None
        for p in (self.dump_path, self.dump_path.with_suffix(self.dump_path.suffix + ".tmp")):
            try:
                p.unlink()
            except FileNotFoundError:
                pass


class KlyroClientTestCase(unittest.TestCase):
    """Base test case: a fresh server + connected client per test."""

    def setUp(self) -> None:
        server_cm = KlyroServerProcess()
        self.server = server_cm.__enter__()
        self.addCleanup(server_cm.__exit__, None, None, None)

        self.client = KlyroClient(host="127.0.0.1", port=self.server.port, timeout=2.0)
        self.client.connect()
        self.addCleanup(self.client.close)


class TestConnectionLifecycle(KlyroClientTestCase):
    def test_ping(self) -> None:
        self.assertTrue(self.client.ping())

    def test_not_connected_before_connect(self) -> None:
        fresh = KlyroClient(host="127.0.0.1", port=self.server.port, timeout=2.0)
        self.assertFalse(fresh.is_connected)
        with self.assertRaises(KlyroError):
            fresh.ping()

    def test_context_manager(self) -> None:
        with KlyroClient(host="127.0.0.1", port=self.server.port, timeout=2.0) as c:
            self.assertTrue(c.is_connected)
            self.assertTrue(c.ping())
        self.assertFalse(c.is_connected)

    def test_close_then_command_raises(self) -> None:
        self.client.close()
        self.assertFalse(self.client.is_connected)
        with self.assertRaises(KlyroError):
            self.client.ping()

    def test_quit_closes_connection(self) -> None:
        self.client.quit()
        self.assertFalse(self.client.is_connected)


class TestGenericCommands(KlyroClientTestCase):
    def test_del_expire_ttl_type_missing_key(self) -> None:
        self.assertFalse(self.client.delete("missing"))
        self.assertFalse(self.client.expire("missing", 10))
        self.assertEqual(self.client.ttl("missing"), -2)
        self.assertIsNone(self.client.type_of("missing"))

    def test_del_expire_ttl_type_existing_key(self) -> None:
        self.client.set("k", "v")
        self.assertEqual(self.client.type_of("k"), "STRING")
        self.assertEqual(self.client.ttl("k"), -1)
        self.assertTrue(self.client.expire("k", 100))
        ttl = self.client.ttl("k")
        self.assertTrue(0 <= ttl <= 100)
        self.assertTrue(self.client.delete("k"))
        self.assertFalse(self.client.delete("k"))
        self.assertIsNone(self.client.type_of("k"))

    def test_dbsize(self) -> None:
        self.assertEqual(self.client.dbsize(), 0)
        self.client.set("a", "1")
        self.client.set("b", "2")
        self.assertEqual(self.client.dbsize(), 2)

    def test_keys_pattern(self) -> None:
        for name in ("foo1", "foo2", "bar1"):
            self.client.set(name, "x")
        self.assertEqual(set(self.client.keys()), {"foo1", "foo2", "bar1"})
        self.assertEqual(set(self.client.keys("foo*")), {"foo1", "foo2"})
        self.assertEqual(set(self.client.keys("baz*")), set())

    def test_key_that_looks_like_an_error_line_is_not_misread(self) -> None:
        # A key like "ERRlog" starts with "ERR" but has no space after
        # it - unlike a real "ERR <message>" reply line - so it must
        # come back as ordinary data, not be raised as a KlyroError.
        self.client.set("ERRlog", "x")
        self.assertEqual(self.client.keys("ERRlog"), ["ERRlog"])

    def test_scan_covers_whole_keyspace(self) -> None:
        expected = {f"key{i}" for i in range(25)}
        for k in expected:
            self.client.set(k, "x")

        seen: set[str] = set()
        cursor = 0
        iterations = 0
        while True:
            batch, cursor = self.client.scan(cursor, count=5)
            seen.update(batch)
            iterations += 1
            if cursor == 0:
                break
            self.assertLess(iterations, 1000, "SCAN never converged")
        self.assertEqual(seen, expected)

    def test_save_writes_dump_file(self) -> None:
        self.client.set("persisted", "yes")
        self.client.save()
        self.assertTrue(self.server.dump_path.exists())

    def test_unknown_command_raises_klyro_error(self) -> None:
        with self.assertRaises(KlyroError) as ctx:
            self.client._command("BOGUS")
        self.assertNotIsInstance(ctx.exception, WrongTypeError)
        self.assertIn("ERR", ctx.exception.raw)


class TestStringCommands(KlyroClientTestCase):
    def test_set_get_roundtrip(self) -> None:
        self.assertIsNone(self.client.get("missing"))
        self.client.set("greeting", "hello world")
        self.assertEqual(self.client.get("greeting"), "hello world")

    def test_incr_decr(self) -> None:
        self.assertEqual(self.client.incr("counter"), 1)
        self.assertEqual(self.client.incr("counter"), 2)
        self.assertEqual(self.client.decr("counter"), 1)

    def test_incr_non_integer_raises_klyro_error_not_wrongtype(self) -> None:
        self.client.set("s", "notanumber")
        with self.assertRaises(KlyroError) as ctx:
            self.client.incr("s")
        self.assertNotIsInstance(ctx.exception, WrongTypeError)

    def test_append_creates_key_and_extends(self) -> None:
        # Note: a leading space in the appended value is not preserved
        # by the server (it trims leading whitespace off the "rest of
        # line" value before storing), so this deliberately avoids one
        # to test the documented "new total length" contract cleanly.
        self.assertEqual(self.client.append("log", "hello"), 5)
        self.assertEqual(self.client.append("log", "-world"), 11)
        self.assertEqual(self.client.get("log"), "hello-world")

    def test_getrange_and_setrange(self) -> None:
        self.client.set("s", "Hello World")
        self.assertEqual(self.client.getrange("s", 0, 4), "Hello")
        self.assertEqual(self.client.getrange("s", -5, -1), "World")
        self.assertEqual(self.client.getrange("s", 100, 200), "")

        n = self.client.setrange("missing_key", 5, "abc")
        self.assertEqual(n, 8)
        self.assertEqual(self.client.get("missing_key"), "     abc")

    def test_set_clears_ttl(self) -> None:
        self.client.set("k", "v")
        self.client.expire("k", 100)
        self.assertGreaterEqual(self.client.ttl("k"), 0)
        self.client.set("k", "v2")
        self.assertEqual(self.client.ttl("k"), -1)


class TestListCommands(KlyroClientTestCase):
    def test_lpush_and_rpush_order(self) -> None:
        self.assertEqual(self.client.lpush("l1", "a", "b", "c"), 3)
        self.assertEqual(self.client.lrange("l1", 0, -1), ["c", "b", "a"])

        self.assertEqual(self.client.rpush("l2", "a", "b", "c"), 3)
        self.assertEqual(self.client.lrange("l2", 0, -1), ["a", "b", "c"])

    def test_lpop_rpop_and_delete_on_empty(self) -> None:
        self.client.rpush("l", "a", "b")
        self.assertEqual(self.client.lpop("l"), "a")
        self.assertEqual(self.client.rpop("l"), "b")
        self.assertIsNone(self.client.lpop("l"))
        self.assertIsNone(self.client.type_of("l"))  # deleted once emptied

    def test_llen(self) -> None:
        self.assertEqual(self.client.llen("missing"), 0)
        self.client.rpush("l", "a", "b", "c")
        self.assertEqual(self.client.llen("l"), 3)

    def test_lpush_requires_at_least_one_value(self) -> None:
        with self.assertRaises(ValueError):
            self.client.lpush("l")


class TestHashCommands(KlyroClientTestCase):
    def test_hset_hget_hdel(self) -> None:
        self.client.hset("user", "name", "Alice")
        self.assertEqual(self.client.hget("user", "name"), "Alice")
        self.assertIsNone(self.client.hget("user", "missing_field"))
        self.assertTrue(self.client.hdel("user", "name"))
        self.assertFalse(self.client.hdel("user", "name"))

    def test_hgetall(self) -> None:
        self.client.hset("user", "name", "Alice")
        self.client.hset("user", "city", "NYC")
        self.assertEqual(self.client.hlen("user"), 2)
        self.assertEqual(self.client.hgetall("user"), {"name": "Alice", "city": "NYC"})

    def test_hgetall_empty(self) -> None:
        self.assertEqual(self.client.hgetall("missing"), {})

    def test_hset_value_may_contain_spaces(self) -> None:
        self.client.hset("user", "bio", "loves rust and python")
        self.assertEqual(self.client.hget("user", "bio"), "loves rust and python")


class TestSetCommands(KlyroClientTestCase):
    def test_sadd_dedup_and_smembers(self) -> None:
        self.assertEqual(self.client.sadd("tags", "a", "b", "a"), 2)
        self.assertEqual(self.client.smembers("tags"), {"a", "b"})
        self.assertEqual(self.client.scard("tags"), 2)

    def test_sismember(self) -> None:
        self.client.sadd("tags", "fast")
        self.assertTrue(self.client.sismember("tags", "fast"))
        self.assertFalse(self.client.sismember("tags", "slow"))

    def test_srem_deletes_when_emptied(self) -> None:
        self.client.sadd("tags", "only")
        self.assertTrue(self.client.srem("tags", "only"))
        self.assertFalse(self.client.srem("tags", "only"))
        self.assertIsNone(self.client.type_of("tags"))

    def test_smembers_empty(self) -> None:
        self.assertEqual(self.client.smembers("missing"), set())


class TestZSetCommands(KlyroClientTestCase):
    def test_zadd_zscore_zrange(self) -> None:
        added = self.client.zadd("board", (100, "alice"), (95.5, "bob"))
        self.assertEqual(added, 2)
        self.assertEqual(self.client.zscore("board", "alice"), 100.0)
        self.assertEqual(self.client.zcard("board"), 2)
        self.assertEqual(
            self.client.zrange("board", 0, -1),
            [("bob", 95.5), ("alice", 100.0)],
        )

    def test_zadd_reposition_does_not_count_as_added(self) -> None:
        self.assertEqual(self.client.zadd("board", (1, "alice")), 1)
        self.assertEqual(self.client.zadd("board", (2, "alice")), 0)
        self.assertEqual(self.client.zscore("board", "alice"), 2.0)

    def test_zrem_deletes_when_emptied(self) -> None:
        self.client.zadd("board", (1, "solo"))
        self.assertTrue(self.client.zrem("board", "solo"))
        self.assertFalse(self.client.zrem("board", "solo"))
        self.assertIsNone(self.client.type_of("board"))

    def test_zscore_missing(self) -> None:
        self.assertIsNone(self.client.zscore("board", "nobody"))

    def test_zadd_too_many_pairs_raises_locally(self) -> None:
        pairs = [(float(i), f"m{i}") for i in range(129)]
        with self.assertRaises(ValueError):
            self.client.zadd("board", *pairs)


class TestWrongTypeErrors(KlyroClientTestCase):
    def test_lpush_on_string_key(self) -> None:
        self.client.set("s", "v")
        with self.assertRaises(WrongTypeError):
            self.client.lpush("s", "x")

    def test_hset_on_list_key(self) -> None:
        self.client.rpush("l", "x")
        with self.assertRaises(WrongTypeError):
            self.client.hset("l", "f", "v")

    def test_sadd_on_string_key(self) -> None:
        self.client.set("s", "v")
        with self.assertRaises(WrongTypeError):
            self.client.sadd("s", "m")

    def test_zadd_on_hash_key(self) -> None:
        self.client.hset("h", "f", "v")
        with self.assertRaises(WrongTypeError):
            self.client.zadd("h", (1, "m"))

    def test_wrongtype_is_a_klyro_error(self) -> None:
        self.client.set("s", "v")
        with self.assertRaises(KlyroError):
            self.client.lpush("s", "x")

    def test_multiline_command_wrongtype_does_not_hang(self) -> None:
        """LRANGE (a multi-line-reply command) against a WRONGTYPE key
        must raise instead of blocking forever waiting for END, since
        the server sends only the ERR line with no terminator."""
        self.client.set("s", "v")
        with self.assertRaises(WrongTypeError):
            self.client.lrange("s", 0, -1)

    def test_multiline_smembers_wrongtype_does_not_hang(self) -> None:
        self.client.set("s", "v")
        with self.assertRaises(WrongTypeError):
            self.client.smembers("s")


class TestClientSideValidation(KlyroClientTestCase):
    def test_key_with_whitespace_rejected(self) -> None:
        with self.assertRaises(ValueError):
            self.client.get("bad key")

    def test_member_with_whitespace_rejected(self) -> None:
        with self.assertRaises(ValueError):
            self.client.sadd("s", "bad member")

    def test_value_with_newline_rejected(self) -> None:
        with self.assertRaises(ValueError):
            self.client.set("k", "line1\nline2")


if __name__ == "__main__":
    unittest.main()
