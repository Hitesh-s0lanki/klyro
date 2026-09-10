//! TCP networking and the poll()-based event loop. A single-threaded
//! model, same as the original C version: `Store` needs no
//! synchronization since only one thread ever touches it.
//!
//! Framing is RESP. A connection's read buffer is handed to
//! [`resp::parse_request`] until it stops yielding whole commands, and
//! each reply is encoded straight into the write buffer, so several
//! pipelined commands cost one read and one write.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::App;
use crate::commands;
use crate::resp::{self, Protocol, Reply};

/// How much to read from a socket at a time.
const READ_CHUNK: usize = 16 * 1024;

pub struct Conn {
    stream: TcpStream,
    want_close: bool,
    /// Negotiated by HELLO, and RESP2 until then - the version every
    /// client starts out speaking.
    protocol: Protocol,
    rbuf: Vec<u8>,
    wbuf: Vec<u8>,
    wbuf_sent: usize,
}

impl Conn {
    fn new(stream: TcpStream) -> Self {
        Conn {
            stream,
            want_close: false,
            protocol: Protocol::Resp2,
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
        resp::encode(reply, self.protocol, &mut self.wbuf);
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

    let mut conns: HashMap<RawFd, Conn> = HashMap::new();

    while !SIGNAL_STOP.load(Ordering::SeqCst) && app.running {
        poll_once(&listener, &mut conns, app)?;
    }

    for conn in conns.values_mut() {
        handle_write(conn); // best-effort flush of any queued reply
    }

    Ok(())
}

fn accept_new_conns(listener: &TcpListener, conns: &mut HashMap<RawFd, Conn>, app: &mut App) {
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                let fd = stream.as_raw_fd();
                app.stats.total_connections += 1;

                let mut conn = Conn::new(stream);
                // Over the ceiling: say so and close, rather than
                // dropping the connection without explanation.
                if conns.len() >= app.config.maxclients {
                    app.stats.rejected_connections += 1;
                    conn.push(&Reply::error("ERR max number of clients reached"));
                    conn.request_close();
                }
                conns.insert(fd, conn);
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
fn process_requests(conn: &mut Conn, app: &mut App) {
    let max_bulk = app.config.proto_max_bulk_len;
    let output_limit = app.config.client_output_buffer_limit;

    loop {
        match resp::parse_request(&conn.rbuf, max_bulk) {
            // Not a whole command yet; wait for more bytes.
            Ok(None) => break,
            Ok(Some(request)) => {
                conn.rbuf.drain(..request.consumed);
                if let Some(response) = commands::dispatch(app, &request.argv) {
                    // HELLO's own reply goes out in the version it
                    // switched to, which is what clients expect.
                    if let Some(protocol) = response.protocol {
                        conn.protocol = protocol;
                    }
                    conn.push(&response.reply);
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

fn poll_once(
    listener: &TcpListener,
    conns: &mut HashMap<RawFd, Conn>,
    app: &mut App,
) -> io::Result<()> {
    let mut fds: Vec<libc::pollfd> = Vec::with_capacity(conns.len() + 1);
    fds.push(libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    });

    let mut order: Vec<RawFd> = Vec::with_capacity(conns.len());
    for (&fd, conn) in conns.iter() {
        let mut events = 0;
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

    let nready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 1000) };
    if nready < 0 {
        let err = io::Error::last_os_error();
        if err.kind() == io::ErrorKind::Interrupted {
            return Ok(());
        }
        return Err(err);
    }

    if fds[0].revents & libc::POLLIN != 0 {
        accept_new_conns(listener, conns, app);
    }

    let mut to_close = Vec::new();
    for (i, &fd) in order.iter().enumerate() {
        let re = fds[i + 1].revents;
        let conn = conns
            .get_mut(&fd)
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
        if conn.want_close && !conn.pending_output() {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        conns.remove(&fd);
    }

    app.stats.connected_clients = conns.len();
    app.tick();
    Ok(())
}
