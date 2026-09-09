# klyro-client

Official Python client for [Klyro](https://github.com/Hitesh-s0lanki/klyro), a
small Redis-style in-memory data server written in Rust.

- Synchronous, `socket`-based, zero runtime dependencies (stdlib only).
- One connection per `KlyroClient` instance, no auto-reconnect (fail fast -
  see the main repo's `docs/client-libraries.md` for the reasoning).
- Type-hinted throughout, ships a `py.typed` marker for downstream type
  checkers.
- Python >= 3.9.

This package is not published to PyPI yet; install it locally for
development.

## Install (local dev)

From this directory:

```sh
pip install -e .
```

To also pull in the test extra (`pytest`):

```sh
pip install -e ".[dev]"
```

## Quick start

```python
from klyro_client import KlyroClient, KlyroError, WrongTypeError

with KlyroClient(host="127.0.0.1", port=7171) as client:
    # Strings
    client.set("foo", "bar")
    client.get("foo")                    # -> "bar"
    client.incr("counter")               # -> 1

    # Lists
    client.rpush("mylist", "a", "b", "c")
    client.lrange("mylist", 0, -1)        # -> ["a", "b", "c"]

    # Hashes
    client.hset("user", "name", "Alice")
    client.hgetall("user")                # -> {"name": "Alice"}

    # Sets
    client.sadd("tags", "fast", "small")
    client.smembers("tags")               # -> {"fast", "small"}

    # Sorted sets
    client.zadd("board", (100, "alice"), (95.5, "bob"))
    client.zrange("board", 0, -1)         # -> [("bob", 95.5), ("alice", 100.0)]
```

`KlyroClient` does **not** connect on construction - call `.connect()`
yourself, or (as above) use it as a context manager, which connects on
`__enter__` and closes on `__exit__`. You can also manage the lifecycle
manually:

```python
client = KlyroClient()
client.connect()
try:
    client.set("foo", "bar")
finally:
    client.close()
```

## Error handling

Any server reply starting with `ERR` raises `KlyroError`, which carries the
raw error text (`str(exc)` and `exc.raw`). The one error callers are likely
to want to special-case - a command run against a key holding the wrong
data type (e.g. `LPUSH` on a key created by `SET`) - raises `WrongTypeError`,
a subclass of `KlyroError`, so you can catch either depending on how
specific you need to be:

```python
try:
    client.lpush("a_string_key", "x")
except WrongTypeError:
    print("wrong type - not a list")
except KlyroError as exc:
    print("some other server error:", exc.raw)
```

Commands whose reply is a simple `OK`/`NOT_FOUND` outcome (`DEL`, `EXPIRE`,
`HDEL`, `SREM`, `ZREM`) return a `bool` instead of raising - `True` for
`OK`, `False` for `NOT_FOUND`.

Keys, hash fields, and set/sorted-set members must not contain whitespace
(the wire protocol has no escaping mechanism); `SET`/`HSET` values may
contain spaces but never a newline. The client validates these client-side
and raises `ValueError` immediately rather than sending a malformed command.

## Method reference

One method per server command, `snake_case`, e.g. `ttl`, `type_of` (named
to avoid shadowing the `type` builtin), `dbsize`, `lpush`/`rpush`,
`hgetall`, `smembers`, `zadd`/`zrange`. See the docstrings in
[`src/klyro_client/client.py`](src/klyro_client/client.py) for the full
surface and exact return types, and the top-level repo's `README.md`
"Commands" section for the underlying wire protocol.

## Tests

Requires the `klyro` server binary built at `../../target/release/klyro`
(from the repo root: `cargo build --release`); the test suite builds it
automatically if missing. Tests spawn a real server subprocess per test
and talk to it over actual TCP - no mocking.

```sh
python3 -m unittest discover -s tests
# or, with the [dev] extra installed:
python3 -m pytest tests/
```
