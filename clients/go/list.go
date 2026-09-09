package klyro

import (
	"fmt"
	"strings"
)

// LPush pushes one or more values onto the head of key, each in turn (so
// LPush("k", "a", "b", "c") ends up ["c", "b", "a"]), and returns the
// list's length afterward.
func (c *Client) LPush(key string, values ...string) (int64, error) {
	return c.push("LPUSH", key, values)
}

// RPush pushes one or more values onto the tail of key, each in turn (so
// RPush("k", "a", "b", "c") ends up ["a", "b", "c"]), and returns the
// list's length afterward.
func (c *Client) RPush(key string, values ...string) (int64, error) {
	return c.push("RPUSH", key, values)
}

func (c *Client) push(op, key string, values []string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	if len(values) == 0 {
		return 0, fmt.Errorf("klyro: %s requires at least one value", op)
	}
	if err := validateTokens("value", values); err != nil {
		return 0, err
	}
	line, err := c.send(op + " " + key + " " + strings.Join(values, " "))
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// LPop removes and returns the head of key. ok is false (nil error) if
// key does not exist. Deletes key once emptied.
func (c *Client) LPop(key string) (string, bool, error) {
	return c.pop("LPOP", key)
}

// RPop removes and returns the tail of key. ok is false (nil error) if
// key does not exist. Deletes key once emptied.
func (c *Client) RPop(key string) (string, bool, error) {
	return c.pop("RPOP", key)
}

func (c *Client) pop(op, key string) (string, bool, error) {
	if err := validateToken("key", key); err != nil {
		return "", false, err
	}
	line, err := c.send(op + " " + key)
	if err != nil {
		return "", false, err
	}
	return valueOrNotFound(line)
}

// LLen returns the length of the list at key.
func (c *Client) LLen(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("LLEN " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// LRange returns the inclusive range of values from start to stop
// (negative indices count from the end).
func (c *Client) LRange(key string, start, stop int64) ([]string, error) {
	if err := validateToken("key", key); err != nil {
		return nil, err
	}
	return c.sendMultiEnd(fmt.Sprintf("LRANGE %s %d %d", key, start, stop))
}
