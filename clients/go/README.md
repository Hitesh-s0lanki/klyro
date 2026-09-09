# klyro (Go client)

The official Go client for [Klyro](../../README.md), a small Redis-style
in-memory data server. Pure standard library, zero dependencies.

## Install

```sh
go get github.com/Hitesh-s0lanki/klyro/clients/go
```

This only resolves once the module is pushed/tagged in the main repo. To
use it locally before that (e.g. while developing against a checkout),
add a `replace` directive to your own module's `go.mod`:

```
require github.com/Hitesh-s0lanki/klyro/clients/go v0.0.0

replace github.com/Hitesh-s0lanki/klyro/clients/go => /path/to/own-database/clients/go
```

## Quick start

```go
package main

import (
	"fmt"
	"log"

	"github.com/Hitesh-s0lanki/klyro/clients/go"
)

func main() {
	c, err := klyro.Dial("127.0.0.1:7171") // or klyro.New() for the same default
	if err != nil {
		log.Fatal(err)
	}
	defer c.Close()

	if err := c.Set("foo", "bar"); err != nil {
		log.Fatal(err)
	}
	value, ok, err := c.Get("foo")
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println(value, ok) // bar true

	// Lists
	if _, err := c.RPush("mylist", "a", "b", "c"); err != nil {
		log.Fatal(err)
	}
	items, err := c.LRange("mylist", 0, -1)
	fmt.Println(items, err) // [a b c] <nil>

	// Hashes
	_ = c.HSet("user", "name", "Alice")
	all, _ := c.HGetAll("user")
	fmt.Println(all) // map[name:Alice]

	// Sorted sets
	_, _ = c.ZAdd("board", klyro.ZPair{Score: 100, Member: "alice"})
	ranked, _ := c.ZRange("board", 0, -1)
	fmt.Println(ranked) // [{alice 100}]
}
```

`klyro.New` layers functional options on top of the same default
(`127.0.0.1:7171`):

```go
c, err := klyro.New(
	klyro.WithHost("db.internal"),
	klyro.WithPort(7171),
	klyro.WithTimeout(3 * time.Second), // connect + per-request deadline
)
```

## Error handling

Every server `ERR ...` reply becomes a `*klyro.KlyroError` (its `Raw`
field carries the exact reply text). The `WRONGTYPE` case specifically
is a `*klyro.WrongTypeError`, which wraps `*KlyroError` — check for it
either way:

```go
_, err := c.LPush("a-string-key", "x")
if errors.Is(err, klyro.ErrWrongType) {
	// key holds a different type
}
var kerr *klyro.KlyroError
if errors.As(err, &kerr) {
	fmt.Println(kerr.Raw) // e.g. "ERR WRONGTYPE Operation against a key holding the wrong kind of value"
}
```

For "missing value" commands (`Get`, `HGet`, `ZScore`, `LPop`, `RPop`),
NOT_FOUND is not an error — it's `ok == false, err == nil`:

```go
v, ok, err := c.Get("missing")
// err == nil, ok == false, v == ""
```

For OK/NOT_FOUND mutating commands (`Del`, `Expire`, `HDel`, `SRem`,
`ZRem`), the same idea applies via `(bool, error)`:

```go
existed, err := c.Del("missing")
// err == nil, existed == false
```

Keys/fields/members must not contain whitespace, and `SET`/`HSET`
values must not contain a newline — the wire protocol has no escaping,
so the client validates these client-side and returns a plain `error`
rather than sending a malformed command. Note also that the server's own
line parser strips *leading* whitespace off a rest-of-line value (`SET`,
`APPEND`, `HSET`, ...) — interior spaces are preserved, but a value that
starts with a space will have it stripped server-side.

## Concurrency

A `*Client` serializes command execution internally with a `sync.Mutex`,
so it's safe to share one across goroutines — but the wire protocol has
no request IDs to demultiplex replies, so concurrent calls just queue up
and run one at a time rather than in parallel over the one TCP
connection. For real concurrency, use one `*Client` per goroutine (or a
small pool of them).

## Testing

```sh
go build ./...
go vet ./...
go test ./...
```

`client_test.go` spawns the real compiled `klyro` binary
(`target/release/klyro` at the repo root, built via `cargo build
--release` automatically if missing) per test, on its own port and a
temp dump file, and drives it through this client — no mocking.
