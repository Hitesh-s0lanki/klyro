// Package klyro is the official Go client for Klyro, a small Redis-style
// in-memory data server (github.com/Hitesh-s0lanki/klyro).
//
// # Conventions
//
// "Missing value" methods (Get, HGet, ZScore, LPop, RPop) return
// (value, ok bool, err error): ok is false and err is nil when the server
// replied NOT_FOUND, so a caller writes
//
//	v, ok, err := c.Get("key")
//	if err != nil { ... }
//	if !ok { ... }
//
// "OK / NOT_FOUND" mutating commands (Del, Expire, HDel, SRem, ZRem)
// return (bool, error) with the same shape: false, nil for NOT_FOUND, not
// an error.
//
// Any server "ERR ..." reply becomes a *KlyroError (or, for the WRONGTYPE
// case specifically, a *WrongTypeError — see errors.go). Connection- and
// protocol-level failures (I/O errors, unexpected reply shapes) are
// returned as plain wrapped errors.
//
// # Concurrency
//
// A *Client serializes command execution internally with a sync.Mutex, so
// it is safe to share a single *Client across goroutines. The wire
// protocol has no request IDs to demultiplex replies, so concurrent calls
// simply queue up and run one at a time — they do not run in parallel
// over the one TCP connection. For real concurrency, use one *Client per
// goroutine (or a pool of them).
package klyro

import (
	"bufio"
	"fmt"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"
)

// DefaultHost and DefaultPort are Klyro's default listen address, used by
// New when no WithHost/WithPort option overrides them.
const (
	DefaultHost = "127.0.0.1"
	DefaultPort = 7171
)

// Client is a connection to a Klyro server. Create one with Dial or New;
// release it with Close when done. The zero Client is not usable.
type Client struct {
	mu      sync.Mutex
	conn    net.Conn
	r       *bufio.Reader
	timeout time.Duration // per-operation I/O deadline; 0 = none
	closed  bool
}

// Dial connects to a Klyro server at addr (host:port, e.g. "127.0.0.1:7171")
// using net.Dial's default (no) timeout. Use New with WithTimeout, or
// DialTimeout, if you want a bounded connect/I-O deadline.
func Dial(addr string) (*Client, error) {
	conn, err := net.Dial("tcp", addr)
	if err != nil {
		return nil, fmt.Errorf("klyro: dial %s: %w", addr, err)
	}
	return newClient(conn, 0), nil
}

// DialTimeout connects to a Klyro server at addr, bounding both the
// connect and every subsequent request/reply round trip by timeout.
func DialTimeout(addr string, timeout time.Duration) (*Client, error) {
	conn, err := net.DialTimeout("tcp", addr, timeout)
	if err != nil {
		return nil, fmt.Errorf("klyro: dial %s: %w", addr, err)
	}
	return newClient(conn, timeout), nil
}

// Option configures New.
type Option func(*config)

type config struct {
	host    string
	port    int
	timeout time.Duration
}

// WithHost sets the server host. Default: DefaultHost ("127.0.0.1").
func WithHost(host string) Option {
	return func(c *config) { c.host = host }
}

// WithPort sets the server port. Default: DefaultPort (7171).
func WithPort(port int) Option {
	return func(c *config) { c.port = port }
}

// WithTimeout bounds both the initial connect and every subsequent
// request/reply round trip. Default: 5s. Pass 0 for no timeout at all.
func WithTimeout(d time.Duration) Option {
	return func(c *config) { c.timeout = d }
}

// New connects to a Klyro server, configured via functional options
// (WithHost, WithPort, WithTimeout). With no options it dials
// 127.0.0.1:7171 with a 5s timeout.
func New(opts ...Option) (*Client, error) {
	cfg := config{host: DefaultHost, port: DefaultPort, timeout: 5 * time.Second}
	for _, opt := range opts {
		opt(&cfg)
	}
	addr := net.JoinHostPort(cfg.host, strconv.Itoa(cfg.port))
	var conn net.Conn
	var err error
	if cfg.timeout > 0 {
		conn, err = net.DialTimeout("tcp", addr, cfg.timeout)
	} else {
		conn, err = net.Dial("tcp", addr)
	}
	if err != nil {
		return nil, fmt.Errorf("klyro: dial %s: %w", addr, err)
	}
	return newClient(conn, cfg.timeout), nil
}

func newClient(conn net.Conn, timeout time.Duration) *Client {
	return &Client{conn: conn, r: bufio.NewReader(conn), timeout: timeout}
}

// Close closes the underlying TCP connection. It does not send QUIT
// first; call Quit explicitly if you want the server to see a clean
// disconnect. Close is safe to call more than once.
func (c *Client) Close() error {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.closed {
		return nil
	}
	c.closed = true
	return c.conn.Close()
}

// ---- low-level protocol I/O -----------------------------------------
//
// All of it assumes c.mu is already held by the caller (every exported
// command method takes the lock via one of send/sendMultiEnd/
// sendMultiCursor below, so this internal machinery does not lock again).

func (c *Client) applyDeadline() {
	if c.timeout > 0 {
		_ = c.conn.SetDeadline(time.Now().Add(c.timeout))
	}
}

func (c *Client) writeLine(line string) error {
	if c.closed {
		return fmt.Errorf("klyro: client is closed")
	}
	c.applyDeadline()
	if _, err := c.conn.Write([]byte(line + "\r\n")); err != nil {
		return fmt.Errorf("klyro: write: %w", err)
	}
	return nil
}

func (c *Client) readLine() (string, error) {
	c.applyDeadline()
	line, err := c.r.ReadString('\n')
	if err != nil {
		return "", fmt.Errorf("klyro: read: %w", err)
	}
	return strings.TrimRight(line, "\r\n"), nil
}

// send issues a single-line command and returns its single-line reply
// (with any "ERR ..." reply turned into an error).
func (c *Client) send(cmdLine string) (string, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.writeLine(cmdLine); err != nil {
		return "", err
	}
	line, err := c.readLine()
	if err != nil {
		return "", err
	}
	// Every real error reply is "ERR <message>" (always a space after
	// ERR) - checking for that space avoids misreading a data line that
	// merely *starts with* "ERR" (e.g. a key named "ERRlog" as the
	// first line of a KEYS/SMEMBERS/... reply) as an error.
	if strings.HasPrefix(line, "ERR ") {
		return "", errFromLine(line)
	}
	return line, nil
}

// sendMultiEnd issues a command whose reply is zero or more data lines
// followed by a literal "END" line (KEYS, LRANGE, HGETALL, SMEMBERS,
// ZRANGE) — or, on error, a single "ERR ..." line with no terminator.
func (c *Client) sendMultiEnd(cmdLine string) ([]string, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.writeLine(cmdLine); err != nil {
		return nil, err
	}
	line, err := c.readLine()
	if err != nil {
		return nil, err
	}
	// Every real error reply is "ERR <message>" (always a space after
	// ERR) - checking for that space avoids misreading a data line that
	// merely *starts with* "ERR" (e.g. a key named "ERRlog" as the
	// first line of a KEYS/SMEMBERS/... reply) as an error.
	if strings.HasPrefix(line, "ERR ") {
		return nil, errFromLine(line)
	}
	var lines []string
	for line != "END" {
		lines = append(lines, line)
		line, err = c.readLine()
		if err != nil {
			return nil, err
		}
	}
	return lines, nil
}

// sendMultiCursor issues SCAN: zero or more key lines followed by a
// "CURSOR <n>" line — or, on error, a single "ERR ..." line with no
// terminator.
func (c *Client) sendMultiCursor(cmdLine string) ([]string, uint64, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if err := c.writeLine(cmdLine); err != nil {
		return nil, 0, err
	}
	line, err := c.readLine()
	if err != nil {
		return nil, 0, err
	}
	// Every real error reply is "ERR <message>" (always a space after
	// ERR) - checking for that space avoids misreading a data line that
	// merely *starts with* "ERR" (e.g. a key named "ERRlog" as the
	// first line of a KEYS/SMEMBERS/... reply) as an error.
	if strings.HasPrefix(line, "ERR ") {
		return nil, 0, errFromLine(line)
	}
	var lines []string
	for !strings.HasPrefix(line, "CURSOR ") {
		lines = append(lines, line)
		line, err = c.readLine()
		if err != nil {
			return nil, 0, err
		}
	}
	cursor, err := strconv.ParseUint(strings.TrimPrefix(line, "CURSOR "), 10, 64)
	if err != nil {
		return nil, 0, fmt.Errorf("klyro: invalid cursor in reply %q: %w", line, err)
	}
	return lines, cursor, nil
}

// ---- reply parsing helpers --------------------------------------------

// parseIntReply parses a "PREFIX <n>" reply, e.g. "LEN 3" with prefix "LEN".
func parseIntReply(line, prefix string) (int64, error) {
	rest, ok := strings.CutPrefix(line, prefix+" ")
	if !ok {
		return 0, fmt.Errorf("klyro: unexpected reply %q (want %q prefix)", line, prefix)
	}
	n, err := strconv.ParseInt(rest, 10, 64)
	if err != nil {
		return 0, fmt.Errorf("klyro: invalid integer in reply %q: %w", line, err)
	}
	return n, nil
}

// valueOrNotFound handles the common "VALUE <value>" | "NOT_FOUND" shape.
func valueOrNotFound(line string) (string, bool, error) {
	if line == "NOT_FOUND" {
		return "", false, nil
	}
	rest, ok := strings.CutPrefix(line, "VALUE ")
	if !ok {
		return "", false, fmt.Errorf("klyro: unexpected reply %q (want \"VALUE \" prefix)", line)
	}
	return rest, true, nil
}

// okOrNotFound handles the common "OK" | "NOT_FOUND" shape.
func okOrNotFound(line string) (bool, error) {
	switch line {
	case "OK":
		return true, nil
	case "NOT_FOUND":
		return false, nil
	default:
		return false, fmt.Errorf("klyro: unexpected reply %q (want \"OK\" or \"NOT_FOUND\")", line)
	}
}

// ---- client-side validation --------------------------------------------
//
// The wire protocol has no escaping mechanism: keys/fields/members are
// whitespace-delimited tokens, and SET/HSET values are "rest of the
// line" but must not contain a newline (which would otherwise be parsed
// as the start of a second command). We validate client-side rather than
// silently send a malformed command that the server would misparse.

func validateToken(kind, s string) error {
	if strings.ContainsAny(s, " \t\r\n") {
		return fmt.Errorf("klyro: %s must not contain whitespace: %q", kind, s)
	}
	return nil
}

func validateTokens(kind string, ss []string) error {
	for _, s := range ss {
		if err := validateToken(kind, s); err != nil {
			return err
		}
	}
	return nil
}

func validateLineValue(kind, s string) error {
	if strings.ContainsRune(s, '\n') {
		return fmt.Errorf("klyro: %s must not contain a newline: %q", kind, s)
	}
	return nil
}
