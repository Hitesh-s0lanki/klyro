"""Shared test-support code: spawn a klyro server subprocess, talk its
line protocol over a socket, and clean up afterward.

Not a test module itself - imported by the test_*.py files.
"""

import itertools
import os
import socket
import subprocess
import tempfile
import time

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KLYRO_BIN = os.path.join(REPO_ROOT, "klyro")

_port_counter = itertools.count(7300)


def _remove_if_exists(path):
    try:
        os.remove(path)
    except FileNotFoundError:
        pass


class KlyroClient:
    """A connected socket speaking Klyro's CRLF line protocol."""

    def __init__(self, sock):
        self.sock = sock

    def send(self, line, nbytes=8192):
        """Sends one command line and returns the raw reply text."""
        self.sock.sendall((line + "\r\n").encode())
        time.sleep(0.03)  # replies are small and prompt; a short wait is enough
        return self.sock.recv(nbytes).decode()

    def close(self):
        self.sock.close()


class KlyroServer:
    """Runs a klyro server subprocess on its own port and dump file."""

    def __init__(self, port=None, dump_path=None):
        if not os.path.exists(KLYRO_BIN):
            raise RuntimeError(f"{KLYRO_BIN} not found - run `make` before the tests")

        self.port = port if port is not None else next(_port_counter)
        if dump_path is not None:
            # Caller passed an explicit path - likely to reload an existing
            # dump (e.g. simulating a restart), so leave it alone.
            self.dump_path = dump_path
        else:
            self.dump_path = os.path.join(tempfile.gettempdir(), f"klyro_test_{self.port}.dump")
            _remove_if_exists(self.dump_path)
            _remove_if_exists(self.dump_path + ".tmp")

        self.proc = subprocess.Popen(
            [KLYRO_BIN, str(self.port), self.dump_path],
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        try:
            self._wait_until_ready()
        except Exception:
            self.proc.kill()
            self.proc.wait(timeout=3)
            raise

    def _wait_until_ready(self, timeout=3):
        deadline = time.time() + timeout
        last_err = None
        while time.time() < deadline:
            if self.proc.poll() is not None:
                raise RuntimeError(
                    f"klyro exited early (code {self.proc.returncode}): "
                    f"{self.proc.stdout.read()}"
                )
            try:
                probe = socket.create_connection(("127.0.0.1", self.port), timeout=0.5)
                probe.close()
                return
            except OSError as e:
                last_err = e
                time.sleep(0.05)
        raise RuntimeError(f"klyro on port {self.port} never became ready: {last_err}")

    def connect(self):
        sock = socket.create_connection(("127.0.0.1", self.port), timeout=2)
        sock.settimeout(2)
        return KlyroClient(sock)

    def kill(self):
        """Hard-stops the server without saving - use when the test doesn't
        care about the dump file's final contents."""
        if self.proc.poll() is None:
            self.proc.kill()
        self.proc.wait(timeout=3)
        self.proc.stdout.close()

    def shutdown(self):
        """Gracefully stops the server via SHUTDOWN (saves first)."""
        c = self.connect()
        c.send("SHUTDOWN")
        c.close()
        self.proc.wait(timeout=3)
        self.proc.stdout.close()

    def output(self):
        data = self.proc.stdout.read()
        self.proc.stdout.close()  # safe even if kill()/shutdown() already did
        return data

    def cleanup_dump(self):
        _remove_if_exists(self.dump_path)
        _remove_if_exists(self.dump_path + ".tmp")
