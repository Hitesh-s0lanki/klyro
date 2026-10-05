# Klyro Go client

The Go client keeps the standard `go-redis` API and adds typed methods for all
Klyro `MEM.*` commands.

```sh
go get github.com/Hitesh-s0lanki/klyro/go
```

```go
package main

import (
	"context"

	klyro "github.com/Hitesh-s0lanki/klyro/go"
)

func main() {
	ctx := context.Background()
	db := klyro.NewClient(nil)
	defer db.Close()

	// Standard go-redis methods remain available.
	db.Set(ctx, "session:42", "active", 0)

	// Klyro-specific commands have typed options and results.
	db.Memory.Create(ctx, "user:123", klyro.CreateOptions{
		Mode: klyro.Hybrid,
		Dim:  384,
	})
	db.Memory.Add(ctx, "user:123", klyro.AddOptions{
		Text:   "User prefers PostgreSQL.",
		Vector: embedding,
	})
	hits, err := db.Memory.Query(ctx, "user:123", klyro.QueryOptions{
		Text:   "preferred database?",
		Vector: queryEmbedding,
		SearchOptions: klyro.SearchOptions{TopK: 5},
	})
	_, _ = hits, err
}
```

Use `klyro.Wrap(client)` to add the typed memory API to an existing
`redis.UniversalClient`.

## Typed memory methods

`Create`, `Info`, `Config`, `Card`, `Add`, `Get`, `MGet`, `Delete`,
`SetMetadata`, `DeleteMetadata`, `Expire`, `Scan`, `Search`, `VectorSearch`, and
`Query` cover every memory command implemented by Klyro 0.1.1. Float slices are
encoded as little-endian float32 vectors, and replies are decoded into Go
structs.
