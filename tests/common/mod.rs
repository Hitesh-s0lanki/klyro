//! Shared test-support code: spawn a klyro server subprocess, talk its
//! line protocol over a socket, and clean up afterward. Not a test
//! module itself - included via `mod common;` by each integration test
//! file.
//!
//! Unlike the old Python suite (which shared one server per test class
//! to cut process-spawn overhead), each Rust test gets its own
//! dedicated server: klyro starts in milliseconds, and per-test
//! isolation means `Drop` alone (no shared static, no cross-test key
//! namespacing) guarantees cleanup even when a test panics.
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
// child binds it. A shared counter (mirroring the old Python helper's
// `itertools.count(7300)`) hands out unique ports deterministically.
static NEXT_PORT: AtomicU16 = AtomicU16::new(17300);

fn free_port() -> u16 {
    NEXT_PORT.fetch_add(1, Ordering::Relaxed)
}

/// A connected socket speaking Klyro's CRLF line protocol.
pub struct KlyroClient {
    stream: TcpStream,
}

impl KlyroClient {
    /// Sends one command line and returns the raw reply text.
    pub fn send(&mut self, line: &str) -> String {
        self.stream.write_all(line.as_bytes()).expect("write");
        self.stream.write_all(b"\r\n").expect("write");
        // replies are small and prompt; a short wait is enough
        std::thread::sleep(Duration::from_millis(30));
        let mut buf = [0u8; 8192];
        match self.stream.read(&mut buf) {
            Ok(n) => String::from_utf8_lossy(&buf[..n]).into_owned(),
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                String::new()
            }
            Err(e) => panic!("read failed: {e}"),
        }
    }
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

    /// Reuses an explicit port/dump path - for simulating a restart
    /// against an existing dump file (leaves it alone, unlike `new()`).
    pub fn with_dump(port: u16, dump_path: PathBuf) -> Self {
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

    /// Runs the binary with `args` and waits for it to accept
    /// connections on `port`. Returns the process's captured output
    /// instead of panicking if it exits early, so a test can assert on
    /// a startup failure.
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

        let deadline = Instant::now() + Duration::from_secs(3);
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
            std::thread::sleep(Duration::from_millis(50));
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
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        KlyroClient { stream }
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
        self.wait_for_exit(Duration::from_secs(3));
    }

    /// Waits up to `timeout` for the process to exit on its own (e.g.
    /// after sending SHUTDOWN directly), returning its exit status.
    pub fn wait_for_exit(&mut self, timeout: Duration) -> ExitStatus {
        let deadline = Instant::now() + timeout;
        loop {
            let status = {
                let child = self.child.as_mut().expect("child already reaped");
                child.try_wait().expect("try_wait")
            };
            if let Some(status) = status {
                // Leave `self.child` in place (still `Some`) so its piped
                // stdout/stderr stay readable via `output()`. Killing or
                // waiting an already-exited child again later (from
                // `kill()`/`Drop`) is harmless - the errors are ignored.
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

/// Splits a reply like "a\r\nb\r\nEND\r\n" into (["a", "b"], had the
/// trailing terminator). Panics if the terminator isn't present.
pub fn lines_before_terminator<'a>(resp: &'a str, terminator: &str) -> Vec<&'a str> {
    assert!(
        resp.ends_with(&format!("{terminator}\r\n")),
        "expected reply to end with {terminator:?}, got {resp:?}"
    );
    let mut lines: Vec<&str> = resp.trim_end_matches("\r\n").split("\r\n").collect();
    lines.pop(); // drop the terminator itself
    lines
}
