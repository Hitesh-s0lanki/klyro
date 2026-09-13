package klyro_test

import (
	"context"
	"os"
	"testing"

	"github.com/Hitesh-s0lanki/klyro/go"
	"github.com/redis/go-redis/v9"
)

func TestTypedMemoryClient(t *testing.T) {
	addr := os.Getenv("KLYRO_TEST_ADDR")
	if addr == "" {
		t.Skip("set KLYRO_TEST_ADDR to run against a Klyro server")
	}
	ctx := context.Background()
	db := klyro.NewClient(&redis.Options{Addr: addr})
	t.Cleanup(func() { _ = db.Close() })

	const key = "go:test:memory"
	_ = db.Del(ctx, key).Err()
	if err := db.Memory.Create(ctx, key, klyro.CreateOptions{Mode: klyro.Hybrid, Dim: 2}); err != nil {
		t.Fatal(err)
	}
	id, err := db.Memory.Add(ctx, key, klyro.AddOptions{Text: "postgres database", Vector: []float32{0.1, 0.9}})
	if err != nil {
		t.Fatal(err)
	}
	hits, err := db.Memory.Query(ctx, key, klyro.QueryOptions{
		Text:          "postgres",
		Vector:        []float32{0.1, 0.9},
		SearchOptions: klyro.SearchOptions{TopK: 1, WithScores: true, ReturnOptions: klyro.ReturnOptions{WithVector: true}},
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(hits) != 1 || hits[0].ID != id || len(hits[0].Vector) != 2 {
		t.Fatalf("unexpected query result: %#v", hits)
	}
}
