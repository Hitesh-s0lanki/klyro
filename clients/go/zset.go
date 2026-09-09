package klyro

import (
	"fmt"
	"strconv"
	"strings"
)

// ZPair is one score/member argument to ZAdd.
type ZPair struct {
	Score  float64
	Member string
}

// ZMember is one member/score result from ZRange.
type ZMember struct {
	Member string
	Score  float64
}

// ZAdd adds or repositions one or more score/member pairs in the sorted
// set at key, returning the count of members newly added (repositioning
// an existing member's score doesn't count). Klyro accepts at most 128
// pairs per call.
func (c *Client) ZAdd(key string, pairs ...ZPair) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	if len(pairs) == 0 {
		return 0, fmt.Errorf("klyro: ZADD requires at least one score/member pair")
	}
	args := make([]string, 0, len(pairs)*2+1)
	args = append(args, "ZADD", key)
	for _, p := range pairs {
		if err := validateToken("member", p.Member); err != nil {
			return 0, err
		}
		args = append(args, formatScore(p.Score), p.Member)
	}
	line, err := c.send(strings.Join(args, " "))
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "ADDED")
}

// ZScore returns member's score in the sorted set at key. ok is false
// (nil error) if key or member does not exist.
func (c *Client) ZScore(key, member string) (float64, bool, error) {
	if err := validateToken("key", key); err != nil {
		return 0, false, err
	}
	if err := validateToken("member", member); err != nil {
		return 0, false, err
	}
	line, err := c.send("ZSCORE " + key + " " + member)
	if err != nil {
		return 0, false, err
	}
	raw, ok, err := valueOrNotFound(line)
	if err != nil || !ok {
		return 0, ok, err
	}
	score, err := strconv.ParseFloat(raw, 64)
	if err != nil {
		return 0, false, fmt.Errorf("klyro: invalid score in reply %q: %w", line, err)
	}
	return score, true, nil
}

// ZRem removes member from the sorted set at key (deleting key once
// emptied). Only one member is removed per call. It returns (true, nil)
// if member existed, or (false, nil) for NOT_FOUND.
func (c *Client) ZRem(key, member string) (bool, error) {
	if err := validateToken("key", key); err != nil {
		return false, err
	}
	if err := validateToken("member", member); err != nil {
		return false, err
	}
	line, err := c.send("ZREM " + key + " " + member)
	if err != nil {
		return false, err
	}
	return okOrNotFound(line)
}

// ZCard returns the number of members in the sorted set at key.
func (c *Client) ZCard(key string) (int64, error) {
	if err := validateToken("key", key); err != nil {
		return 0, err
	}
	line, err := c.send("ZCARD " + key)
	if err != nil {
		return 0, err
	}
	return parseIntReply(line, "LEN")
}

// ZRange returns the inclusive range of members from start to stop
// (negative indices count from the end), ascending by score.
func (c *Client) ZRange(key string, start, stop int64) ([]ZMember, error) {
	if err := validateToken("key", key); err != nil {
		return nil, err
	}
	lines, err := c.sendMultiEnd(fmt.Sprintf("ZRANGE %s %d %d", key, start, stop))
	if err != nil {
		return nil, err
	}
	result := make([]ZMember, 0, len(lines))
	for _, line := range lines {
		fields := strings.Fields(line)
		if len(fields) != 2 {
			return nil, fmt.Errorf("klyro: unexpected ZRANGE line %q", line)
		}
		score, err := strconv.ParseFloat(fields[1], 64)
		if err != nil {
			return nil, fmt.Errorf("klyro: invalid score in ZRANGE line %q: %w", line, err)
		}
		result = append(result, ZMember{Member: fields[0], Score: score})
	}
	return result, nil
}

// formatScore formats a float the same way ZADD's argument is expected
// on the wire: the shortest decimal representation that round-trips.
func formatScore(f float64) string {
	return strconv.FormatFloat(f, 'g', -1, 64)
}
