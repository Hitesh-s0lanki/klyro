package klyro

import "github.com/redis/go-redis/v9"

// Client exposes the standard go-redis API and typed Klyro memory commands.
type Client struct {
	redis.UniversalClient
	Memory *MemoryClient
}

// NewClient connects to Klyro at 127.0.0.1:7171 unless options override it.
func NewClient(options *redis.Options) *Client {
	if options == nil {
		options = &redis.Options{Addr: "127.0.0.1:7171"}
	} else if options.Addr == "" {
		copy := *options
		copy.Addr = "127.0.0.1:7171"
		options = &copy
	}
	rdb := redis.NewClient(options)
	client := &Client{UniversalClient: rdb}
	client.Memory = &MemoryClient{redis: rdb}
	return client
}

// Wrap adds typed memory commands to an existing go-redis client.
func Wrap(client redis.UniversalClient) *Client {
	return &Client{UniversalClient: client, Memory: &MemoryClient{redis: client}}
}
