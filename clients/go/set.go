package klyro

import (
	"fmt"
	"strings"
)

// SAdd adds one or more members to the set at key and returns the count
// of members newly added (duplicates already present don't count).
func (c *Client) SAdd(key string, members ...string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	if len(members) == 0 {
		return 0, fmt.Errorf("klyro: SADD requires at least one member")
	}
	if err := validateTokens("member", members); err != nil {
		return 0, err
	}
	line, err := c.send("SADD " + key + " " + strings.Join(members, " "))
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "ADDED")
}

// SRem removes member from the set at key (deleting key once emptied).
// Only one member is removed per call. It returns (true, nil) if member
// existed, or (false, nil) for NOT_FOUND.
func (c *Client) SRem(key, member string) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	if err := validateToken("member", member); err != nil {
		return false, err
	}
	line, err := c.send("SREM " + key + " " + member)
	if err != nil {
		return false, err
	}
	return okOrNotFound(line)
}

// SIsMember reports whether member is in the set at key.
func (c *Client) SIsMember(key, member string) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	if err := validateToken("member", member); err != nil {
		return false, err
	}
	line, err := c.send("SISMEMBER " + key + " " + member)
	if err != nil {
		return false, err
	}
	switch line {
	case "TRUE":
		return true, nil
	case "FALSE":
		return false, nil
	default:
		return false, fmt.Errorf("klyro: unexpected reply to SISMEMBER: %q", line)
	}
}

// SCard returns the number of members in the set at key.
func (c *Client) SCard(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("SCARD " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// SMembers returns every member of the set at key, as an idiomatic Go
// set (map to struct{}).
func (c *Client) SMembers(key string) (map[string]struct{}, error) {
	if err := validateToken("key", key); err != nil {
		return nil, err
	}
	lines, err := c.sendMultiEnd("SMEMBERS " + key)
	if err != nil {
		return nil, err
	}
	result := make(map[string]struct{}, len(lines))
	for _, m := range lines {
		result[m] = struct{}{}
	}
	return result, nil
}
