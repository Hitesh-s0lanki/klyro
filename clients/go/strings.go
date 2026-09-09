package klyro

import "fmt"

// Set sets key to value (the rest of the line — may contain spaces, but
// not a newline). Always clears any existing TTL on key.
func (c *Client) Set(key, value string) error {
	if err := validateToken("key", key); err != nil {
		return err
	}
	if err := validateLineValue("value", value); err != nil {
		return err
	}
	line, err := c.send("SET " + key + " " + value)
	if err != nil {
		return err
	}
	if line != "OK" {
		return fmt.Errorf("klyro: unexpected reply to SET: %q", line)
	}
	return nil
}

// Get returns key's value. ok is false (with a nil error) if key does
// not exist.
func (c *Client) Get(key string) (string, bool, error) {
	if err := validateToken("key", key); err != nil {
		return "", false, err
	}
	line, err := c.send("GET " + key)
	if err != nil {
		return "", false, err
	}
	return valueOrNotFound(line)
}

// Incr increments key by 1 (missing key starts at 0, preserving any
// existing TTL) and returns the new value.
func (c *Client) Incr(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("INCR " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "VALUE")
}

// Decr decrements key by 1 (missing key starts at 0, preserving any
// existing TTL) and returns the new value.
func (c *Client) Decr(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("DECR " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "VALUE")
}

// Append appends value to key (creating it if missing, preserving any
// TTL) and returns the new total length.
func (c *Client) Append(key, value string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	if err := validateLineValue("value", value); err != nil {
		return 0, err
	}
	line, err := c.send("APPEND " + key + " " + value)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// GetRange returns the inclusive substring of key from start to end
// (negative indices count from the end). An out-of-range request
// returns an empty string, not an error.
func (c *Client) GetRange(key string, start, end int64) (string, error) {
	if err := validateToken("key", key); err != nil {
		return "", err
	}
	line, err := c.send(fmt.Sprintf("GETRANGE %s %d %d", key, start, end))
	if err != nil {
		return "", err
	}
	value, _, err := valueOrNotFound(line)
	return value, err
}

// SetRange overwrites key starting at offset with value (padding any gap
// before offset with ASCII spaces; preserves any existing TTL) and
// returns the new total length.
func (c *Client) SetRange(key string, offset int64, value string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	if err := validateLineValue("value", value); err != nil {
		return 0, err
	}
	line, err := c.send(fmt.Sprintf("SETRANGE %s %d %s", key, offset, value))
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}
