//! Shared test-support code: spawn a klyro server subprocess, talk RESP
//! to it over a socket, and clean up afterward. Not a test module
//! itself - included via `mod common;` by each integration test file.
//!
//! Each test gets its own dedicated server: klyro starts in
//! milliseconds, and per-test isolation means `Drop` alone (no shared
//! static, no cross-test key namespacing) guarantees cleanup even when
//! a test panics.
#![allow(dead_code)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

fn remove_if_exists(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}

// A monotonic counter, not an OS-assigned ephemeral port: binding to
// port 0 and then dropping the listener to "free" it for the child
// process is a TOCTOU race under cargo test's default parallel test
// threads - two tests can grab the same just-freed port before either
// child binds it. A shared counter hands out unique ports
// deterministically.
static NEXT_PORT: AtomicU16 = AtomicU16::new(17300);

fn free_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

/// A parsed RESP reply.
///
/// RESP3's map and set types are not represented: the test client
/// negotiates nothing, so the server answers in RESP2, where both
/// arrive as arrays. The RESP3 spellings have their own tests.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Simple(String),
    Error(String),
    Int(i64),
    Bulk(Vec<u8>),
    Nil,
    Array(Vec<Value>),
    NilArray,
}

/// `+OK`, the reply most mutating commands give.
pub fn ok() -> Value {
    Value::Simple("OK".into())
}

pub fn int(n: i64) -> Value {
    Value::Int(n)
}

pub fn bulk(text: &str) -> Value {
    Value::Bulk(text.as_bytes().to_vec())
}

pub fn nil() -> Value {
    Value::Nil
}

pub fn array(items: Vec<Value>) -> Value {
    Value::Array(items)
}

/// An array of bulk strings - what most listing commands return.
pub fn strings(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|s| bulk(s)).collect())
}

impl Value {
    /// The payload of a bulk reply, as text. Panics on anything else,
    /// which is what a test wants when the reply type is wrong.
    pub fn text(&self) -> String {
        match self {
            Value::Bulk(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            Value::Simple(s) => s.clone(),
            other => panic!("expected a bulk or simple reply, got {other:?}"),
        }
    }

    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Value::Bulk(bytes) => bytes.clone(),
            other => panic!("expected a bulk reply, got {other:?}"),
        }
    }

    pub fn integer(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            other => panic!("expected an integer reply, got {other:?}"),
        }
    }

    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            other => panic!("expected an array reply, got {other:?}"),
        }
    }

    /// An array's elements as text, in the order sent.
    pub fn list(&self) -> Vec<String> {
        self.items().iter().map(Value::text).collect()
    }

    /// An array's elements as text, sorted - for the commands whose
    /// order is unspecified (sets and hashes).
    pub fn sorted(&self) -> Vec<String> {
        let mut items = self.list();
        items.sort();
        items
    }

    /// The message of an error reply.
    pub fn error(&self) -> String {
        match self {
            Value::Error(message) => message.clone(),
            other => panic!("expected an error reply, got {other:?}"),
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, Value::Error(_))
    }

    /// Any scalar rendered as text. Unlike `text`, this accepts an
    /// integer, because a map's values are not all one type.
    pub fn display(&self) -> String {
        match self {
            Value::Int(n) => n.to_string(),
            Value::Nil => "(nil)".to_string(),
            Value::Array(items) => format!("({} items)", items.len()),
            other => other.text(),
        }
    }

    /// A flat field/value array read back as pairs, sorted by field -
    /// the shape HGETALL and CONFIG GET return over RESP2.
    pub fn pairs(&self) -> Vec<(String, String)> {
        let items = self.items();
        assert!(
            items.len().is_multiple_of(2),
            "expected an even number of elements, got {items:?}"
        );
        let mut pairs: Vec<(String, String)> = items
            .chunks(2)
            .map(|pair| (pair[0].display(), pair[1].display()))
            .collect();
        pairs.sort();
        pairs
    }
}

/// A connected socket speaking RESP.
pub struct KlyroClient {
    stream: TcpStream,
    buffer: Vec<u8>,
}

impl KlyroClient {
    /// Sends a command written as one line, split on whitespace.
    /// Convenient, but it cannot express an argument containing a
    /// space - use [`KlyroClient::call`] for those.
    pub fn send(&mut self, line: &str) -> Value {
        let args: Vec<&str> = line.split_whitespace().collect();
        self.call(&args)
    }

    /// Sends a command as an explicit argument list.
    pub fn call(&mut self, args: &[&str]) -> Value {
        let owned: Vec<Vec<u8>> = args.iter().map(|a| a.as_bytes().to_vec()).collect();
        self.call_bytes(&owned)
    }

    /// Sends a command whose arguments are raw bytes.
    pub fn call_bytes(&mut self, args: &[Vec<u8>]) -> Value {
        let mut request = Vec::new();
        request.extend_from_slice(format!("*{}\r\n", args.len()).as_bytes());
        for arg in args {
            request.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
            request.extend_from_slice(arg);
            request.extend_from_slice(b"\r\n");
        }
        self.stream.write_all(&request).expect("write");
        self.read_value()
    }

    /// Sends a raw byte string and reads one reply - for exercising the
    /// inline protocol and malformed frames. An empty `raw` reads the
    /// next reply already in flight, which is how a pipelined batch is
    /// drained.
    pub fn send_raw(&mut self, raw: &[u8]) -> Value {
        self.stream.write_all(raw).expect("write");
        self.read_value()
    }

    /// Writes bytes without waiting for a reply, for feeding a request
    /// in pieces.
    pub fn send_no_reply(&mut self, raw: &[u8]) {
        self.stream.write_all(raw).expect("write");
    }

    /// Whether the server has closed this connection.
    pub fn closed(&mut self) -> bool {
        let mut chunk = [0u8; 64];
        match self.stream.read(&mut chunk) {
            Ok(0) => true,
            Ok(_) => false,
            Err(_) => false,
        }
    }

    /// Reads until a complete reply has arrived, then returns it.
    fn read_value(&mut self) -> Value {
        loop {
            if let Some((value, consumed)) = parse(&self.buffer) {
                self.buffer.drain(..consumed);
                return value;
            }
            let mut chunk = [0u8; 16 * 1024];
            match self.stream.read(&mut chunk) {
                Ok(0) => panic!("server closed the connection mid-reply"),
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
                Err(e) => panic!("read failed: {e}"),
            }
        }
    }
}

/// Parses one reply, returning it and how many bytes it used.
fn parse(buf: &[u8]) -> Option<(Value, usize)> {
    let (line, after) = read_line(buf, 1)?;
    let text = String::from_utf8_lossy(line).into_owned();
    match buf.first()? {
        b'+' => Some((Value::Simple(text), after)),
        b'-' => Some((Value::Error(text), after)),
        b':' => Some((Value::Int(text.trim().parse().ok()?), after)),
        b'_' => Some((Value::Nil, after)),
        b',' => Some((Value::Bulk(line.to_vec()), after)),
        b'#' => Some((Value::Int(if line == b"t" { 1 } else { 0 }), after)),
        b'$' => {
            let len: i64 = text.trim().parse().ok()?;
            if len < 0 {
                return Some((Value::Nil, after));
            }
            let len = len as usize;
            if after + len + 2 > buf.len() {
                return None;
            }
            Some((
                Value::Bulk(buf[after..after + len].to_vec()),
                after + len + 2,
            ))
        }
        // RESP3 maps and sets arrive here too; both are read as a
        // flat array of their elements, which is all these tests need.
        b'*' | b'~' | b'%' => {
            let mut count: i64 = text.trim().parse().ok()?;
            if count < 0 {
                return Some((Value::NilArray, after));
            }
            if buf[0] == b'%' {
                count *= 2; // a map's length counts pairs
            }
            let mut items = Vec::with_capacity(count as usize);
            let mut at = after;
            for _ in 0..count {
                let (item, consumed) = parse(&buf[at..])?;
                items.push(item);
                at += consumed;
            }
            Some((Value::Array(items), at))
        }
        other => panic!("unexpected reply marker {:?}", *other as char),
    }
}

fn read_line(buf: &[u8], from: usize) -> Option<(&[u8], usize)> {
    let mut i = from;
    while i + 1 < buf.len() {
        if buf[i] == b'\r' && buf[i + 1] == b'\n' {
            return Some((&buf[from..i], i + 2));
        }
        i += 1;
    }
    None
}

/// Runs a klyro server subprocess on its own port and dump file.
pub struct KlyroServer {
    pub port: u16,
    pub dump_path: PathBuf,
    /// Set only when the server was started from a config file, so
    /// `Drop` can clean the file up.
    config_path: Option<PathBuf>,
    child: Option<Child>,
}

impl KlyroServer {
    pub fn new() -> Self {
        let port = free_port();
        let dump_path = std::env::temp_dir().join(format!("klyro_test_{port}.dump"));
        remove_if_exists(&dump_path);
        remove_if_exists(&PathBuf::from(format!("{}.tmp", dump_path.display())));
        Self::spawn(port, dump_path)
    }

    /// Runs against an existing dump file, leaving it alone (unlike
    /// `new()`, which clears it first). Pass port 0 to be assigned one.
    pub fn with_dump(port: u16, dump_path: PathBuf) -> Self {
        let port = if port == 0 { free_port() } else { port };
        Self::spawn(port, dump_path)
    }

    /// Starts a server from a generated config file holding `settings`
    /// (one `name value` per line). The port and dump path are filled
    /// in automatically, so a test only writes the lines it cares
    /// about.
    pub fn with_config(settings: &str) -> Self {
        let port = free_port();
        let dump_path = std::env::temp_dir().join(format!("klyro_test_{port}.dump"));
        remove_if_exists(&dump_path);
        let config_path = std::env::temp_dir().join(format!("klyro_test_{port}.conf"));
        std::fs::write(
            &config_path,
            format!(
                "port {}\ndbfilename {}\n{}\n",
                port,
                dump_path.display(),
                settings
            ),
        )
        .expect("write the test config file");

        let mut server = Self::spawn_with_args(
            &[config_path.to_string_lossy().into_owned()],
            port,
            dump_path,
        );
        server.config_path = Some(config_path);
        server
    }

    /// Runs the binary with `args`, returning its captured stderr if it
    /// exits instead of staying up - so a test can assert on a startup
    /// failure.
    pub fn try_spawn_with_args(args: &[String]) -> Result<Child, String> {
        let bin = env!("CARGO_BIN_EXE_klyro");
        let mut child = Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn the klyro binary");

        // Poll rather than sleeping a fixed span: under `cargo test`'s
        // parallel threads a process can take a while just to start.
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            if child.try_wait().expect("try_wait").is_some() {
                let mut out = String::new();
                if let Some(mut e) = child.stderr.take() {
                    let _ = e.read_to_string(&mut out);
                }
                return Err(out);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        Ok(child)
    }

    /// Starts a fresh server (on a newly picked port) against an
    /// existing dump file - simulating a restart that reloads it.
    pub fn reload(dump_path: PathBuf) -> Self {
        Self::spawn(free_port(), dump_path)
    }

    fn spawn(port: u16, dump_path: PathBuf) -> Self {
        let args = vec![port.to_string(), dump_path.to_string_lossy().into_owned()];
        Self::spawn_with_args(&args, port, dump_path)
    }

    fn spawn_with_args(args: &[String], port: u16, dump_path: PathBuf) -> Self {
        let bin = env!("CARGO_BIN_EXE_klyro");
        let mut child = Command::new(bin)
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn the klyro binary");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().expect("try_wait") {
                panic!("klyro exited early with status {status:?}");
            }
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                break;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                panic!("klyro on port {port} never became ready");
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        KlyroServer {
            port,
            dump_path,
            config_path: None,
            child: Some(child),
        }
    }

    pub fn connect(&self) -> KlyroClient {
        let stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        KlyroClient {
            stream,
            buffer: Vec::new(),
        }
    }

    /// Hard-stops the server without saving - use when the test doesn't
    /// care about the dump file's final contents.
    pub fn kill(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// Gracefully stops the server via SHUTDOWN (saves first) and waits
    /// for the process to exit.
    pub fn shutdown(&mut self) {
        let mut client = self.connect();
        client.send("SHUTDOWN");
        self.wait_for_exit(Duration::from_secs(5));
    }

    /// Waits up to `timeout` for the process to exit on its own,
    /// returning its exit status.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let status = {
                let child = self.child.as_mut().expect("child already reaped");
                child.try_wait().expect("try_wait")
            };
            if let Some(status) = status {
                // Leave `self.child` in place (still `Some`) so its
                // piped stdout/stderr stay readable via `output()`.
                return status;
            }
            if Instant::now() > deadline {
                panic!("klyro did not exit within {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The process's combined stdout/stderr so far. Only meaningful
    /// after the process has exited (e.g. via `wait_for_exit`).
    pub fn output(&mut self) -> String {
        let mut out = String::new();
        if let Some(child) = self.child.as_mut() {
            if let Some(mut o) = child.stdout.take() {
                let _ = o.read_to_string(&mut out);
            }
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut out);
            }
        }
        out
    }

    pub fn cleanup_dump(&self) {
        remove_if_exists(&self.dump_path);
        remove_if_exists(&PathBuf::from(format!("{}.tmp", self.dump_path.display())));
    }
}

impl Drop for KlyroServer {
    fn drop(&mut self) {
        self.kill();
        if let Some(path) = &self.config_path {
            remove_if_exists(path);
        }
    }
}

/// The WRONGTYPE message, which many tests assert on.
pub const WRONGTYPE: &str = "WRONGTYPE Operation against a key holding the wrong kind of value";
