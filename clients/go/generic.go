package klyro

import "fmt"

// Ping checks connectivity, expecting the server's PONG reply.
func (c *Client) Ping() error {
	line, err := c.send("PING")
	if err != nil {
		return err
	}
	if line != "PONG" {
		return fmt.Errorf("klyro: unexpected reply to PING: %q", line)
	}
	return nil
}

// Del deletes key. It returns (true, nil) if the key existed and was
// deleted, or (false, nil) if it did not exist (NOT_FOUND is not an
// error).
func (c *Client) Del(key string) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	line, err := c.send("DEL " + key)
	if err != nil {
		return false, err
	}
	return okOrNotFound(line)
}

// Expire sets key's remaining TTL to seconds (a signed integer offset).
// It returns (true, nil) if the key exists, or (false, nil) for
// NOT_FOUND.
func (c *Client) Expire(key string, seconds int64) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	line, err := c.send(fmt.Sprintf("EXPIRE %s %d", key, seconds))
	if err != nil {
		return false, err
	}
	return okOrNotFound(line)
}

// TTL returns key's remaining time-to-live in seconds: -2 if the key is
// missing, -1 if it has no expiry, otherwise the seconds remaining
// (>= 0).
func (c *Client) TTL(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("TTL " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "TTL")
}

// TypeOf returns key's type: "STRING", "LIST", "HASH", "SET", "ZSET", or
// "NONE" if the key does not exist.
func (c *Client) TypeOf(key string) (string, error) {
	if err := validateToken("key", key); err != nil {
		return "", err
	}
	return c.send("TYPE " + key)
}

// Keys returns every key matching pattern (a glob: '*' any run, '?' one
// char, '[...]' a character class, '\' escapes). An empty pattern
// returns every key (bare KEYS, no pattern argument).
func (c *Client) Keys(pattern string) ([]string, error) {
	cmd := "KEYS"
	if pattern != "" {
		if err := validateToken("pattern", pattern); err != nil {
			return nil, err
		}
		cmd += " " + pattern
	}
	return c.sendMultiEnd(cmd)
}

// Scan returns one batch of keys starting at cursor, along with the
// cursor to pass on the next call. Start with cursor 0 and keep calling
// until the returned cursor is 0 again, meaning the whole keyspace has
// been covered. An empty match means no MATCH filter; count <= 0 means
// no COUNT hint is sent (the server defaults to 10).
func (c *Client) Scan(cursor uint64, match string, count int64) ([]string, uint64, error) {
	cmd := fmt.Sprintf("SCAN %d", cursor)
	if match != "" {
		if err := validateToken("match pattern", match); err != nil {
			return nil, 0, err
		}
		cmd += " MATCH " + match
	}
	if count > 0 {
		cmd += fmt.Sprintf(" COUNT %d", count)
	}
	return c.sendMultiCursor(cmd)
}

// DBSize returns the number of keys in the keyspace.
func (c *Client) DBSize() (int64, error) {
	line, err := c.send("DBSIZE")
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "COUNT")
}

// Save writes the dump file immediately.
func (c *Client) Save() error {
	line, err := c.send("SAVE")
	if err != nil {
		return err
	}
	if line != "OK" {
		return fmt.Errorf("klyro: unexpected reply to SAVE: %q", line)
	}
	return nil
}

// Quit tells the server this connection is done (it replies BYE and
// closes its side); Quit then closes the local connection too. Calling
// any other method afterward returns an error.
func (c *Client) Quit() error {
	line, err := c.send("QUIT")
	if err != nil {
		return err
	}
	_ = c.Close()
	if line != "BYE" {
		return fmt.Errorf("klyro: unexpected reply to QUIT: %q", line)
	}
	return nil
}

// Shutdown asks the server to save and exit. Treat the connection (and
// this Client) as gone afterward; Shutdown closes it locally too.
func (c *Client) Shutdown() error {
	line, err := c.send("SHUTDOWN")
	if err != nil {
		return err
	}
	_ = c.Close()
	if line != "SHUTTING_DOWN" {
		return fmt.Errorf("klyro: unexpected reply to SHUTDOWN: %q", line)
	}
	return nil
}
