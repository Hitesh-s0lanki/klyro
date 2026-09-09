package klyro

import "fmt"

// HSet sets field in the hash at key to value (rest-of-line, may contain
// spaces but not a newline).
func (c *Client) HSet(key, field, value string) error {
	if err := validateToken("key", key); err != nil {
		return err
	}
	if err := validateToken("field", field); err != nil {
		return err
	}
	if err := validateLineValue("value", value); err != nil {
		return err
	}
	line, err := c.send("HSET " + key + " " + field + " " + value)
	if err != nil {
		return err
	}
	if line != "OK" {
		return fmt.Errorf("klyro: unexpected reply to HSET: %q", line)
	}
	return nil
}

// HGet returns field's value in the hash at key. ok is false (nil error)
// if key or field does not exist.
func (c *Client) HGet(key, field string) (string, bool, error) {
	if err := validateToken("key", key); err != nil {
		return "", false, err
	}
	if err := validateToken("field", field); err != nil {
		return "", false, err
	}
	line, err := c.send("HGET " + key + " " + field)
	if err != nil {
		return "", false, err
	}
	return valueOrNotFound(line)
}

// HDel removes field from the hash at key (deleting key once emptied).
// It returns (true, nil) if field existed, or (false, nil) for
// NOT_FOUND.
func (c *Client) HDel(key, field string) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	if err := validateToken("field", field); err != nil {
		return false, err
	}
	line, err := c.send("HDEL " + key + " " + field)
	if err != nil {
		return false, err
	}
	return okOrNotFound(line)
}

// HLen returns the number of fields in the hash at key.
func (c *Client) HLen(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("HLEN " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// HGetAll returns every field/value pair in the hash at key.
func (c *Client) HGetAll(key string) (map[string]string, error) {
	if err := validateToken("key", key); err != nil {
		return nil, err
	}
	lines, err := c.sendMultiEnd("HGETALL " + key)
	if err != nil {
		return nil, err
	}
	if len(lines)%2 != 0 {
		return nil, fmt.Errorf("klyro: HGETALL returned an odd number of lines (%d)", len(lines))
	}
	result := make(map[string]string, len(lines)/2)
	for i := 0; i < len(lines); i += 2 {
		result[lines[i]] = lines[i+1]
	}
	return result, nil
}
