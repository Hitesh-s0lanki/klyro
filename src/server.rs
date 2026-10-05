//! TCP networking and the poll()-based event loop. A single-threaded
//! model, same as the original C version: `Store` needs no
//! synchronization since only one thread ever touches it.
//!
//! Framing is RESP. A connection's read buffer is handed to
//! [`resp::parse_request`] until it stops yielding whole commands, and
//! each reply is encoded straight into the write buffer, so several
//! pipelined commands cost one read and one write.
//!
//! Three things make a connection more than a request/reply loop, and
//! all three are handled here rather than in the command layer:
//! a parked client waiting on BLPOP, a pub/sub message arriving for a
//! connection that asked for nothing, and a blocking command's
//! deadline passing while the server is otherwise idle.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::app::App;
use crate::client::Client;
use crate::commands;
use crate::resp::{self, Reply};
use crate::util::bytes::Bytes;

/// How much to read from a socket at a time.
const READ_CHUNK: usize = 16 * 1024;

/// The longest the loop sleeps with nothing to do. Also the coarsest a
/// blocking command's timeout could be, if the deadline did not shorten
/// it.
const POLL_INTERVAL_MS: libc::c_int = 1000;

pub struct Conn {
    stream: TcpStream,
    want_close: bool,
    /// Everything about this connection that a command can see or
    /// change: its protocol version, its MULTI queue, its
    /// subscriptions, whether it is parked.
    client: Client,
    rbuf: Vec<u8>,
    wbuf: Vec<u8>,
    wbuf_sent: usize,
}

impl Conn {
    fn new(stream: TcpStream, client: Client) -> Self {
        Conn {
            stream,
            want_close: false,
            client,
            rbuf: Vec::new(),
            wbuf: Vec::new(),
            wbuf_sent: 0,
        }
    }

    /// Queues one reply. Unlike the old line protocol, nothing is
    /// truncated: a reply too large to buffer closes the connection
    /// (see [`Conn::over_output_limit`]) rather than silently returning
    /// a partial answer.
    fn push(&mut self, reply: &Reply) {
        resp::encode(reply, self.client.protocol, &mut self.wbuf);
    }

    fn over_output_limit(&self, limit: usize) -> bool {
        self.wbuf.len() - self.wbuf_sent > limit
    }

    fn pending_output(&self) -> bool {
        self.wbuf_sent < self.wbuf.len()
    }

    /// Marks this connection to be closed once its queued replies are
    /// flushed.
    fn request_close(&mut self) {
        self.want_close = true;
    }
}

/// The connections, plus the id index the registries on `App` address
/// them by. Pub/sub and the blocking registry both store client ids,
/// because neither has any business knowing what a file descriptor is.
struct Connections {
    conns: HashMap<RawFd, Conn>,
    by_id: HashMap<u64, RawFd>,
}

impl Connections {
    fn new() -> Connections {
        Connections {
            conns: HashMap::new(),
            by_id: HashMap::new(),
        }
    }

    fn insert(&mut self, fd: RawFd, conn: Conn) {
        self.by_id.insert(conn.client.id, fd);
        self.conns.insert(fd, conn);
    }

    fn get_mut(&mut self, fd: RawFd) -> Option<&mut Conn> {
        self.conns.get_mut(&fd)
    }

    fn fd_of(&self, id: u64) -> Option<RawFd> {
        self.by_id.get(&id).copied()
    }

    fn by_client(&mut self, id: u64) -> Option<&mut Conn> {
        let fd = self.fd_of(id)?;
        self.conns.get_mut(&fd)
    }

    /// Closes one connection and releases everything it held in the
    /// shared registries.
    fn remove(&mut self, fd: RawFd, app: &mut App) {
        if let Some(mut conn) = self.conns.remove(&fd) {
            self.by_id.remove(&conn.client.id);
            app.forget_client(&mut conn.client);
        }
    }

    fn len(&self) -> usize {
        self.conns.len()
    }
}

static SIGNAL_STOP: AtomicBool = AtomicBool::new(false);

extern "C" fn handle_stop_signal(_sig: libc::c_int) {
    SIGNAL_STOP.store(true, Ordering::SeqCst);
}

fn install_signal_handlers() {
    unsafe {
        libc::signal(
            libc::SIGINT,
            handle_stop_signal as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGTERM,
            handle_stop_signal as *const () as libc::sighandler_t,
        );
    }
}

pub fn run(app: &mut App) -> io::Result<()> {
    let (bind, port) = (app.config.bind.clone(), app.config.port);
    let listener = TcpListener::bind((bind.as_str(), port))?;
    listener.set_nonblocking(true)?;
    install_signal_handlers();

    println!("listening on {}:{}", bind, port);

    let mut connections = Connections::new();

    while !SIGNAL_STOP.load(Ordering::SeqCst) && app.running {
        poll_once(&listener, &mut connections, app)?;
    }

    for conn in connections.conns.values_mut() {
        handle_write(conn); // best-effort flush of any queued reply
    }

    Ok(())
}

fn accept_new_conns(listener: &TcpListener, connections: &mut Connections, app: &mut App) {
    loop {
        match listener.accept() {
            Ok((stream, addr)) => {
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                let fd = stream.as_raw_fd();
                app.stats.total_connections += 1;

                let client = Client::new(app.take_client_id(), addr.to_string());
                let mut conn = Conn::new(stream, client);
                // Over the ceiling: say so and close, rather than
                // dropping the connection without explanation.
                if connections.len() >= app.config.maxclients {
                    app.stats.rejected_connections += 1;
                    conn.push(&Reply::error("ERR max number of clients reached"));
                    conn.request_close();
                }
                connections.insert(fd, conn);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
            Err(e) => {
                eprintln!("accept: {}", e);
                return;
            }
        }
    }
}

fn handle_read(conn: &mut Conn, app: &mut App) {
    let mut chunk = [0u8; READ_CHUNK];
    match conn.stream.read(&mut chunk) {
        Ok(0) => conn.want_close = true,
        Ok(n) => {
            conn.rbuf.extend_from_slice(&chunk[..n]);
            process_requests(conn, app);
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
        Err(_) => conn.want_close = true,
    }
}

/// Drains every complete command sitting in the read buffer.
///
/// A parked client stops the drain where it is: its remaining bytes
/// stay buffered until whatever it is waiting for arrives, which is
/// what makes a blocking command block rather than answer.
fn process_requests(conn: &mut Conn, app: &mut App) {
    let max_bulk = app.config.proto_max_bulk_len;
    let output_limit = app.config.client_output_buffer_limit;

    loop {
        if conn.client.blocked {
            break;
        }
        match resp::parse_request(&conn.rbuf, max_bulk) {
            // Not a whole command yet; wait for more bytes.
            Ok(None) => break,
            Ok(Some(request)) => {
                conn.rbuf.drain(..request.consumed);
                if let Some(response) = commands::dispatch(app, &mut conn.client, &request.argv) {
                    // HELLO's own reply goes out in the version it
                    // switched to, which is what clients expect.
                    if let Some(protocol) = response.protocol {
                        conn.client.protocol = protocol;
                    }
                    if let Some(block) = response.block {
                        conn.client.blocked = true;
                        app.blocked.push(block);
                        break;
                    }
                    for reply in &response.replies {
                        conn.push(reply);
                    }
                    if response.close {
                        conn.request_close();
                    }
                }
            }
            Err(error) => {
                // The parser can no longer tell where the next command
                // starts, so the connection has to go.
                conn.push(&Reply::error(error.0));
                conn.request_close();
                conn.rbuf.clear();
                break;
            }
        }

        if conn.want_close || conn.over_output_limit(output_limit) {
            if conn.over_output_limit(output_limit) {
                conn.push(&Reply::error(
                    "ERR reply exceeds the client output buffer limit",
                ));
                conn.request_close();
            }
            break;
        }
    }
}

fn handle_write(conn: &mut Conn) {
    while conn.pending_output() {
        match conn.stream.write(&conn.wbuf[conn.wbuf_sent..]) {
            Ok(0) => {
                conn.want_close = true;
                return;
            }
            Ok(n) => conn.wbuf_sent += n,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return,
            Err(_) => {
                conn.want_close = true;
                return;
            }
        }
    }
    conn.wbuf.clear();
    conn.wbuf_sent = 0;
}

/// Retries every parked command whose keys have just been written.
///
/// Waiters are served oldest first, so the client that has waited
/// longest on a queue gets the next value pushed to it. A retry is the
/// original command run again from the top, which is why it can itself
/// make another key ready - a BLMOVE feeding the list a second client
/// is blocked on - and why this drains rather than making one pass.
fn serve_ready_keys(connections: &mut Connections, app: &mut App) {
    while !app.ready_keys.is_empty() {
        let ready: Vec<Bytes> = std::mem::take(&mut app.ready_keys);
        let mut woken: Vec<RawFd> = Vec::new();

        let mut index = 0;
        while index < app.blocked.len() {
            if !app.blocked[index]
                .keys
                .iter()
                .any(|key| ready.contains(key))
            {
                index += 1;
                continue;
            }
            // Out of the registry while it runs: the command may write
            // keys of its own, and it must not find itself waiting on
            // them.
            let waiter = app.blocked.remove(index);
            let Some(fd) = connections.fd_of(waiter.id) else {
                continue; // the connection is already gone
            };
            let conn = connections
                .get_mut(fd)
                .expect("the id index and the connection map agree");
            // Hung up while it waited: dropping the waiter here is what
            // keeps the value in the queue for whoever asks next.
            if conn.want_close {
                continue;
            }

            match commands::retry(app, &mut conn.client, &waiter.argv) {
                Some(reply) => {
                    conn.client.blocked = false;
                    conn.push(&reply);
                    woken.push(fd);
                }
                // Someone else took the value first; keep waiting on
                // the deadline it started with.
                None => {
                    app.blocked.insert(index, waiter);
                    index += 1;
                }
            }
        }

        resume(connections, app, &woken);
    }
}

/// Answers the parked commands whose deadline has passed.
fn serve_timeouts(connections: &mut Connections, app: &mut App) {
    let mut woken = Vec::new();
    for waiter in app.take_timed_out() {
        let Some(conn) = connections.by_client(waiter.id) else {
            continue;
        };
        if conn.want_close {
            continue;
        }
        conn.client.blocked = false;
        conn.push(&waiter.on_timeout);
        if let Some(fd) = connections.fd_of(waiter.id) {
            woken.push(fd);
        }
    }
    resume(connections, app, &woken);
}

/// Picks up where an unblocked connection left off: anything it
/// pipelined behind the blocking command is still sitting in its read
/// buffer.
fn resume(connections: &mut Connections, app: &mut App, woken: &[RawFd]) {
    for &fd in woken {
        if let Some(conn) = connections.get_mut(fd) {
            process_requests(conn, app);
        }
    }
}

/// Hands every pub/sub message to the connection it is addressed to. A
/// message for a client that has since disconnected is dropped, which
/// is the whole of pub/sub's delivery guarantee.
fn deliver_outbox(connections: &mut Connections, app: &mut App) {
    for (id, frame) in std::mem::take(&mut app.outbox) {
        if let Some(conn) = connections.by_client(id) {
            conn.push(&frame);
        }
    }
}

/// How long poll() may sleep: until the next blocking command has to
/// be given up on, and never more than [`POLL_INTERVAL_MS`].
fn poll_timeout(app: &App) -> libc::c_int {
    match app.next_block_deadline() {
        None => POLL_INTERVAL_MS,
        Some(deadline) => {
            let remaining = deadline
                .saturating_duration_since(Instant::now())
                .as_millis();
            (remaining as libc::c_int).clamp(1, POLL_INTERVAL_MS)
        }
    }
}

fn poll_once(
    listener: &TcpListener,
    connections: &mut Connections,
    app: &mut App,
) -> io::Result<()> {
    let mut fds: Vec<libc::pollfd> = Vec::with_capacity(connections.len() + 1);
    fds.push(libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    });

    let mut order: Vec<RawFd> = Vec::with_capacity(connections.len());
    for (&fd, conn) in connections.conns.iter() {
        let mut events = 0;
        // A parked client is still read from. Its commands go no
        // further than the read buffer until it wakes, but the read
        // itself is how a disconnect is noticed - otherwise a value
        // pushed to a queue would be handed to a socket that is
        // already gone.
        if !conn.want_close {
            events |= libc::POLLIN;
        }
        if conn.pending_output() {
            events |= libc::POLLOUT;
        }
        fds.push(libc::pollfd {
            fd,
            events,
            revents: 0,
        });
        order.push(fd);
    }

    let timeout = poll_timeout(app);
    let nready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, timeout) };
    if nready < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(err);
    }

    if fds[0].revents & libc::POLLIN != 0 {
        accept_new_conns(listener, connections, app);
    }

    for (i, &fd) in order.iter().enumerate() {
        let re = fds[i + 1].revents;
        let conn = connections
            .get_mut(fd)
            .expect("fd was in `order`, so it's in `conns`");

        if re & (libc::POLLERR | libc::POLLHUP) != 0 {
            conn.want_close = true;
        }
        if re & libc::POLLIN != 0 {
            handle_read(conn, app);
        }
        if re & libc::POLLOUT != 0 {
            handle_write(conn);
        }
    }

    // Everything a command produced for somebody else: woken waiters
    // first, since serving them can itself publish or make another key
    // ready.
    serve_ready_keys(connections, app);
    serve_timeouts(connections, app);
    deliver_outbox(connections, app);

    let mut to_close = Vec::new();
    for (&fd, conn) in connections.conns.iter_mut() {
        if conn.pending_output() {
            handle_write(conn);
        }
        if conn.want_close && !conn.pending_output() {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        connections.remove(fd, app);
    }

    app.stats.connected_clients = connections.len();
    app.tick();
    Ok(())
}
