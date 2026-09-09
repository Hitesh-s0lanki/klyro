package klyro

import (
	"errors"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"sync/atomic"
	"testing"
	"time"
)

// klyroBinary locates the pre-built server binary, building it via
// `cargo build --release` if it's missing.
func klyroBinary(t *testing.T) string {
	t.Helper()
	// clients/go -> clients -> repo root
	repoRoot, err := filepath.Abs(filepath.Join("..", ".."))
	if err != nil {
		t.Fatalf("resolve repo root: %v", err)
	}
	bin := filepath.Join(repoRoot, "target", "release", "klyro")
	if _, err := os.Stat(bin); err == nil {
		return bin
	}
	t.Logf("klyro binary not found at %s; running cargo build --release", bin)
	cmd := exec.Command("cargo", "build", "--release")
	cmd.Dir = repoRoot
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("cargo build --release failed: %v\n%s", err, out)
	}
	if _, err := os.Stat(bin); err != nil {
		t.Fatalf("klyro binary still missing after cargo build: %v", err)
	}
	return bin
}

// nextPort hands out a unique port per test process run, avoiding
// collisions between tests that spawn their own server subprocess.
var nextPort int32 = 27171

func freePort() int {
	return int(atomic.AddInt32(&nextPort, 1))
}

// startServer spawns a klyro subprocess on its own port and temp dump
// file, waits for it to accept connections, and registers cleanup.
func startServer(t *testing.T) (addr string) {
	t.Helper()
	bin := klyroBinary(t)
	port := freePort()
	dumpPath := filepath.Join(t.TempDir(), "klyro.dump")
	addr = net.JoinHostPort("127.0.0.1", strconv.Itoa(port))

	cmd := exec.Command(bin, strconv.Itoa(port), dumpPath)
	cmd.Stdout = nil
	cmd.Stderr = nil
	if err := cmd.Start(); err != nil {
		t.Fatalf("failed to start klyro: %v", err)
	}
	t.Cleanup(func() {
		if cmd.Process != nil {
			_ = cmd.Process.Kill()
			_, _ = cmd.Process.Wait()
		}
	})

	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		conn, err := net.DialTimeout("tcp", addr, 100*time.Millisecond)
		if err == nil {
			_ = conn.Close()
			return addr
		}
		time.Sleep(20 * time.Millisecond)
	}
	t.Fatalf("klyro server on %s did not become ready in time", addr)
	return ""
}

func mustClient(t *testing.T, addr string) *Client {
	t.Helper()
	c, err := DialTimeout(addr, 2*time.Second)
	if err != nil {
		t.Fatalf("Dial(%s): %v", addr, err)
	}
	t.Cleanup(func() { _ = c.Close() })
	return c
}

func TestPing(t *testing.T) {
	c := mustClient(t, startServer(t))
	if err := c.Ping(); err != nil {
		t.Fatalf("Ping: %v", err)
	}
}

func TestGenericKeyCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	if _, ok, err := c.Get("missing"); err != nil || ok {
		t.Fatalf("Get(missing) = ok=%v err=%v, want ok=false err=nil", ok, err)
	}

	if err := c.Set("k1", "v1"); err != nil {
		t.Fatalf("Set: %v", err)
	}

	typ, err := c.TypeOf("k1")
	if err != nil || typ != "STRING" {
		t.Fatalf("TypeOf(k1) = %q, %v; want STRING, nil", typ, err)
	}
	if typ, err := c.TypeOf("missing"); err != nil || typ != "NONE" {
		t.Fatalf("TypeOf(missing) = %q, %v; want NONE, nil", typ, err)
	}

	ttl, err := c.TTL("k1")
	if err != nil || ttl != -1 {
		t.Fatalf("TTL(k1) = %d, %v; want -1, nil", ttl, err)
	}
	if ttl, err := c.TTL("missing"); err != nil || ttl != -2 {
		t.Fatalf("TTL(missing) = %d, %v; want -2, nil", ttl, err)
	}

	ok, err := c.Expire("k1", 100)
	if err != nil || !ok {
		t.Fatalf("Expire(k1) = %v, %v; want true, nil", ok, err)
	}
	if ttl, err := c.TTL("k1"); err != nil || ttl < 0 || ttl > 100 {
		t.Fatalf("TTL(k1) after Expire = %d, %v; want in [0,100]", ttl, err)
	}
	if ok, err := c.Expire("missing", 100); err != nil || ok {
		t.Fatalf("Expire(missing) = %v, %v; want false, nil", ok, err)
	}

	if err := c.Set("k2", "v2"); err != nil {
		t.Fatalf("Set k2: %v", err)
	}
	n, err := c.DBSize()
	if err != nil || n != 2 {
		t.Fatalf("DBSize = %d, %v; want 2, nil", n, err)
	}

	keys, err := c.Keys("")
	if err != nil {
		t.Fatalf("Keys: %v", err)
	}
	if len(keys) != 2 {
		t.Fatalf("Keys() returned %v, want 2 keys", keys)
	}

	keys, err = c.Keys("k1")
	if err != nil || len(keys) != 1 || keys[0] != "k1" {
		t.Fatalf("Keys(k1) = %v, %v; want [k1], nil", keys, err)
	}

	keys, err = c.Keys("nomatch*")
	if err != nil {
		t.Fatalf("Keys(nomatch*): %v", err)
	}
	if len(keys) != 0 {
		t.Fatalf("Keys(nomatch*) = %v, want empty", keys)
	}

	ok, err = c.Del("k1")
	if err != nil || !ok {
		t.Fatalf("Del(k1) = %v, %v; want true, nil", ok, err)
	}
	ok, err = c.Del("k1")
	if err != nil || ok {
		t.Fatalf("Del(k1) again = %v, %v; want false, nil", ok, err)
	}

	if err := c.Save(); err != nil {
		t.Fatalf("Save: %v", err)
	}
}

// A key like "ERRlog" starts with "ERR" but has no space after it -
// unlike a real "ERR <message>" reply line - so it must come back as
// ordinary data, not be misread as an error reply.
func TestKeyThatLooksLikeAnErrorLineIsNotMisread(t *testing.T) {
	c := mustClient(t, startServer(t))

	if err := c.Set("ERRlog", "x"); err != nil {
		t.Fatalf("Set(ERRlog): %v", err)
	}
	keys, err := c.Keys("ERRlog")
	if err != nil {
		t.Fatalf("Keys(ERRlog): %v", err)
	}
	if len(keys) != 1 || keys[0] != "ERRlog" {
		t.Fatalf("Keys(ERRlog) = %v, want [ERRlog]", keys)
	}
}

func TestScan(t *testing.T) {
	c := mustClient(t, startServer(t))

	want := map[string]bool{}
	for i := 0; i < 25; i++ {
		key := "scan:" + strconv.Itoa(i)
		if err := c.Set(key, "v"); err != nil {
			t.Fatalf("Set: %v", err)
		}
		want[key] = true
	}

	seen := map[string]bool{}
	var cursor uint64
	iterations := 0
	for {
		iterations++
		if iterations > 1000 {
			t.Fatalf("SCAN did not converge (possible infinite loop)")
		}
		keys, next, err := c.Scan(cursor, "scan:*", 5)
		if err != nil {
			t.Fatalf("Scan: %v", err)
		}
		for _, k := range keys {
			seen[k] = true
		}
		cursor = next
		if cursor == 0 {
			break
		}
	}

	for k := range want {
		if !seen[k] {
			t.Errorf("SCAN never returned key %q", k)
		}
	}
}

func TestStringCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	if err := c.Set("s", "hello"); err != nil {
		t.Fatalf("Set: %v", err)
	}
	v, ok, err := c.Get("s")
	if err != nil || !ok || v != "hello" {
		t.Fatalf("Get(s) = %q, %v, %v; want hello, true, nil", v, ok, err)
	}

	// Note: the server's line parser strips *leading* whitespace off a
	// rest-of-line value (it collapses the run of spaces that separated
	// it from the preceding token), so an appended value starting with
	// a space loses that leading space. Interior spaces are preserved.
	n, err := c.Append("s", "there")
	if err != nil || n != 10 {
		t.Fatalf("Append = %d, %v; want 10, nil", n, err)
	}
	v, _, err = c.Get("s")
	if err != nil || v != "hellothere" {
		t.Fatalf("Get after Append = %q, %v; want %q", v, err, "hellothere")
	}
	if err := c.Set("spaced", "hello   world"); err != nil {
		t.Fatalf("Set: %v", err)
	}
	v, _, err = c.Get("spaced")
	if err != nil || v != "hello   world" {
		t.Fatalf("Get(spaced) = %q, %v; want interior spaces preserved", v, err)
	}

	sub, err := c.GetRange("s", 0, 4)
	if err != nil || sub != "hello" {
		t.Fatalf("GetRange(0,4) = %q, %v; want hello, nil", sub, err)
	}
	sub, err = c.GetRange("s", -5, -1)
	if err != nil || sub != "there" {
		t.Fatalf("GetRange(-5,-1) = %q, %v; want there, nil", sub, err)
	}
	sub, err = c.GetRange("s", 1000, 2000)
	if err != nil || sub != "" {
		t.Fatalf("GetRange(out of range) = %q, %v; want empty, nil", sub, err)
	}

	n, err = c.SetRange("gap", 5, "X")
	if err != nil || n != 6 {
		t.Fatalf("SetRange = %d, %v; want 6, nil", n, err)
	}
	v, _, err = c.Get("gap")
	if err != nil || v != "     X" {
		t.Fatalf("Get(gap) = %q, %v; want %q", v, err, "     X")
	}

	if err := c.Set("counter", "10"); err != nil {
		t.Fatalf("Set counter: %v", err)
	}
	iv, err := c.Incr("counter")
	if err != nil || iv != 11 {
		t.Fatalf("Incr = %d, %v; want 11, nil", iv, err)
	}
	iv, err = c.Decr("counter")
	if err != nil || iv != 10 {
		t.Fatalf("Decr = %d, %v; want 10, nil", iv, err)
	}

	iv, err = c.Incr("brand-new-counter")
	if err != nil || iv != 1 {
		t.Fatalf("Incr(new) = %d, %v; want 1, nil", iv, err)
	}

	if err := c.Set("notint", "abc"); err != nil {
		t.Fatalf("Set notint: %v", err)
	}
	_, err = c.Incr("notint")
	var kerr *KlyroError
	if !errors.As(err, &kerr) {
		t.Fatalf("Incr(notint) error = %v, want *KlyroError", err)
	}
}

func TestListCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	n, err := c.LPush("list", "a", "b", "c")
	if err != nil || n != 3 {
		t.Fatalf("LPush = %d, %v; want 3, nil", n, err)
	}
	// LPUSH k a b c -> [c, b, a]
	vals, err := c.LRange("list", 0, -1)
	if err != nil {
		t.Fatalf("LRange: %v", err)
	}
	want := []string{"c", "b", "a"}
	if !equalStrings(vals, want) {
		t.Fatalf("LRange after LPush = %v, want %v", vals, want)
	}

	if err := c.Set("emptylist-target", "x"); err != nil {
		t.Fatalf("Set: %v", err)
	}

	n, err = c.RPush("rlist", "a", "b", "c")
	if err != nil || n != 3 {
		t.Fatalf("RPush = %d, %v; want 3, nil", n, err)
	}
	vals, err = c.LRange("rlist", 0, -1)
	if err != nil {
		t.Fatalf("LRange: %v", err)
	}
	want = []string{"a", "b", "c"}
	if !equalStrings(vals, want) {
		t.Fatalf("LRange after RPush = %v, want %v", vals, want)
	}

	l, err := c.LLen("rlist")
	if err != nil || l != 3 {
		t.Fatalf("LLen = %d, %v; want 3, nil", l, err)
	}

	v, ok, err := c.LPop("rlist")
	if err != nil || !ok || v != "a" {
		t.Fatalf("LPop = %q, %v, %v; want a, true, nil", v, ok, err)
	}
	v, ok, err = c.RPop("rlist")
	if err != nil || !ok || v != "c" {
		t.Fatalf("RPop = %q, %v, %v; want c, true, nil", v, ok, err)
	}

	// Empty the list and confirm the key is gone (NOT_FOUND on the
	// next pop) and LPush requires at least one value.
	if _, _, err := c.LPop("rlist"); err != nil {
		t.Fatalf("LPop last: %v", err)
	}
	if _, ok, err := c.LPop("rlist"); err != nil || ok {
		t.Fatalf("LPop(emptied) = ok=%v err=%v, want ok=false err=nil", ok, err)
	}

	if _, err := c.LPush("list"); err == nil {
		t.Fatalf("LPush with no values should error client-side")
	}
}

func TestHashCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	if err := c.HSet("h", "f1", "v1"); err != nil {
		t.Fatalf("HSet: %v", err)
	}
	if err := c.HSet("h", "f2", "v2"); err != nil {
		t.Fatalf("HSet: %v", err)
	}

	v, ok, err := c.HGet("h", "f1")
	if err != nil || !ok || v != "v1" {
		t.Fatalf("HGet = %q, %v, %v; want v1, true, nil", v, ok, err)
	}
	if _, ok, err := c.HGet("h", "missing"); err != nil || ok {
		t.Fatalf("HGet(missing field) = ok=%v err=%v, want false, nil", ok, err)
	}

	l, err := c.HLen("h")
	if err != nil || l != 2 {
		t.Fatalf("HLen = %d, %v; want 2, nil", l, err)
	}

	all, err := c.HGetAll("h")
	if err != nil {
		t.Fatalf("HGetAll: %v", err)
	}
	want := map[string]string{"f1": "v1", "f2": "v2"}
	if len(all) != len(want) || all["f1"] != "v1" || all["f2"] != "v2" {
		t.Fatalf("HGetAll = %v, want %v", all, want)
	}

	ok, err = c.HDel("h", "f1")
	if err != nil || !ok {
		t.Fatalf("HDel = %v, %v; want true, nil", ok, err)
	}
	ok, err = c.HDel("h", "f1")
	if err != nil || ok {
		t.Fatalf("HDel again = %v, %v; want false, nil", ok, err)
	}
}

func TestSetCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	n, err := c.SAdd("s", "a", "b", "c", "a")
	if err != nil || n != 3 {
		t.Fatalf("SAdd = %d, %v; want 3, nil", n, err)
	}

	card, err := c.SCard("s")
	if err != nil || card != 3 {
		t.Fatalf("SCard = %d, %v; want 3, nil", card, err)
	}

	members, err := c.SMembers("s")
	if err != nil {
		t.Fatalf("SMembers: %v", err)
	}
	want := map[string]struct{}{"a": {}, "b": {}, "c": {}}
	if len(members) != len(want) {
		t.Fatalf("SMembers = %v, want %v", members, want)
	}
	for m := range want {
		if _, ok := members[m]; !ok {
			t.Fatalf("SMembers missing %q: %v", m, members)
		}
	}

	is, err := c.SIsMember("s", "a")
	if err != nil || !is {
		t.Fatalf("SIsMember(a) = %v, %v; want true, nil", is, err)
	}
	is, err = c.SIsMember("s", "z")
	if err != nil || is {
		t.Fatalf("SIsMember(z) = %v, %v; want false, nil", is, err)
	}

	ok, err := c.SRem("s", "a")
	if err != nil || !ok {
		t.Fatalf("SRem = %v, %v; want true, nil", ok, err)
	}
	ok, err = c.SRem("s", "a")
	if err != nil || ok {
		t.Fatalf("SRem again = %v, %v; want false, nil", ok, err)
	}

	if _, err := c.SAdd("s2"); err == nil {
		t.Fatalf("SAdd with no members should error client-side")
	}
}

func TestZSetCommands(t *testing.T) {
	c := mustClient(t, startServer(t))

	n, err := c.ZAdd("z", ZPair{Score: 3, Member: "charlie"}, ZPair{Score: 1, Member: "alice"}, ZPair{Score: 2, Member: "bob"})
	if err != nil || n != 3 {
		t.Fatalf("ZAdd = %d, %v; want 3, nil", n, err)
	}

	// repositioning doesn't count as newly added
	n, err = c.ZAdd("z", ZPair{Score: 10, Member: "alice"})
	if err != nil || n != 0 {
		t.Fatalf("ZAdd reposition = %d, %v; want 0, nil", n, err)
	}

	score, ok, err := c.ZScore("z", "bob")
	if err != nil || !ok || score != 2 {
		t.Fatalf("ZScore(bob) = %v, %v, %v; want 2, true, nil", score, ok, err)
	}
	if _, ok, err := c.ZScore("z", "missing"); err != nil || ok {
		t.Fatalf("ZScore(missing) = ok=%v err=%v, want false, nil", ok, err)
	}

	card, err := c.ZCard("z")
	if err != nil || card != 3 {
		t.Fatalf("ZCard = %d, %v; want 3, nil", card, err)
	}

	members, err := c.ZRange("z", 0, -1)
	if err != nil {
		t.Fatalf("ZRange: %v", err)
	}
	wantOrder := []string{"bob", "charlie", "alice"} // scores 2, 3, 10 ascending
	if len(members) != len(wantOrder) {
		t.Fatalf("ZRange = %v, want %d members", members, len(wantOrder))
	}
	for i, m := range members {
		if m.Member != wantOrder[i] {
			t.Fatalf("ZRange[%d] = %q, want %q (full: %v)", i, m.Member, wantOrder[i], members)
		}
	}

	ok, err = c.ZRem("z", "bob")
	if err != nil || !ok {
		t.Fatalf("ZRem = %v, %v; want true, nil", ok, err)
	}
	ok, err = c.ZRem("z", "bob")
	if err != nil || ok {
		t.Fatalf("ZRem again = %v, %v; want false, nil", ok, err)
	}

	if _, err := c.ZAdd("z2"); err == nil {
		t.Fatalf("ZAdd with no pairs should error client-side")
	}
}

func TestWrongType(t *testing.T) {
	c := mustClient(t, startServer(t))

	if err := c.Set("str", "value"); err != nil {
		t.Fatalf("Set: %v", err)
	}

	_, err := c.LPush("str", "x")
	if err == nil {
		t.Fatalf("LPush on a string key should fail with WRONGTYPE")
	}
	var wte *WrongTypeError
	if !errors.As(err, &wte) {
		t.Fatalf("LPush on string key error = %v (%T), want *WrongTypeError", err, err)
	}
	if !errors.Is(err, ErrWrongType) {
		t.Fatalf("errors.Is(err, ErrWrongType) = false, want true (err=%v)", err)
	}
	var kerr *KlyroError
	if !errors.As(err, &kerr) {
		t.Fatalf("errors.As(err, *KlyroError) = false, want true (err=%v)", err)
	}
}

func TestUnknownCommandMapsToKlyroError(t *testing.T) {
	// Exercise the low-level send() path directly against a raw reply
	// the typed API doesn't expose, to confirm generic ERR mapping.
	c := mustClient(t, startServer(t))
	_, err := c.send("BOGUS")
	if err == nil {
		t.Fatalf("expected an error for an unknown command")
	}
	var kerr *KlyroError
	if !errors.As(err, &kerr) {
		t.Fatalf("error = %v (%T), want *KlyroError", err, err)
	}
	if errors.As(err, new(*WrongTypeError)) {
		t.Fatalf("unknown-command error should not be a *WrongTypeError")
	}
}

func TestClientSideValidation(t *testing.T) {
	c := mustClient(t, startServer(t))

	if err := c.Set("bad key", "v"); err == nil {
		t.Fatalf("Set with a whitespace key should be rejected client-side")
	}
	if err := c.Set("key", "line1\nline2"); err == nil {
		t.Fatalf("Set with a newline in the value should be rejected client-side")
	}
	if _, err := c.SAdd("set", "bad member"); err == nil {
		t.Fatalf("SAdd with a whitespace member should be rejected client-side")
	}
}

func TestClose(t *testing.T) {
	addr := startServer(t)
	c, err := DialTimeout(addr, 2*time.Second)
	if err != nil {
		t.Fatalf("Dial: %v", err)
	}
	if err := c.Ping(); err != nil {
		t.Fatalf("Ping before Close: %v", err)
	}
	if err := c.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	// Close should be idempotent.
	if err := c.Close(); err != nil {
		t.Fatalf("second Close: %v", err)
	}
	// Using the client after Close should error, not panic.
	if err := c.Ping(); err == nil {
		t.Fatalf("Ping after Close should error")
	}
}

func TestNewWithOptions(t *testing.T) {
	addr := startServer(t)
	host, portStr, err := net.SplitHostPort(addr)
	if err != nil {
		t.Fatalf("SplitHostPort: %v", err)
	}
	port, err := strconv.Atoi(portStr)
	if err != nil {
		t.Fatalf("Atoi: %v", err)
	}
	c, err := New(WithHost(host), WithPort(port), WithTimeout(2*time.Second))
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { _ = c.Close() })
	if err := c.Ping(); err != nil {
		t.Fatalf("Ping: %v", err)
	}
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
