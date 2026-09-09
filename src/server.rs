//! TCP networking and the poll()-based event loop. A single-threaded
//! model, same as the original C version: `Store` needs no
//! synchronization since only one thread ever touches it.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::App;
use crate::commands;

const MAX_MSG: usize = 64 * 1024;

pub struct Conn {
    stream: TcpStream,
    want_close: bool,
    rbuf: Vec<u8>,
    wbuf: Vec<u8>,
    wbuf_sent: usize,
}

impl Conn {
    fn new(stream: TcpStream) -> Self {
        Conn {
            stream,
            want_close: false,
            rbuf: Vec::new(),
            wbuf: Vec::new(),
            wbuf_sent: 0,
        }
    }

    /// Queues a reply on this connection's write buffer, truncating
    /// silently past `MAX_MSG` pending bytes (matches the original's
    /// fixed-size write buffer).
    pub fn reply(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let room = MAX_MSG.saturating_sub(self.wbuf.len());
        let n = bytes.len().min(room);
        self.wbuf.extend_from_slice(&bytes[..n]);
    }

    /// Marks this connection to be closed once its queued replies are
    /// flushed.
    pub fn request_close(&mut self) {
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

pub fn run(port: u16, app: &mut App) -> io::Result<()> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    listener.set_nonblocking(true)?;
    install_signal_handlers();

    println!("listening on port {}", port);

    let mut conns: HashMap<RawFd, Conn> = HashMap::new();

    while !SIGNAL_STOP.load(Ordering::SeqCst) && app.running {
        poll_once(&listener, &mut conns, app)?;
    }

    for conn in conns.values_mut() {
        handle_write(conn); // best-effort flush of any queued reply
    }

    Ok(())
}

fn accept_new_conns(listener: &TcpListener, conns: &mut HashMap<RawFd, Conn>) {
    loop {
        match listener.accept() {
            Ok((stream, _addr)) => {
                if stream.set_nonblocking(true).is_err() {
                    continue;
                }
                let _ = stream.set_nodelay(true);
                let fd = stream.as_raw_fd();
                conns.insert(fd, Conn::new(stream));
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
    if conn.rbuf.len() >= MAX_MSG {
        conn.reply("ERR line too long\r\n");
        conn.rbuf.clear();
        return;
    }

    let mut tmp = [0u8; MAX_MSG];
    let remaining = MAX_MSG - conn.rbuf.len();
    match conn.stream.read(&mut tmp[..remaining]) {
        Ok(0) => conn.want_close = true,
        Ok(n) => {
            conn.rbuf.extend_from_slice(&tmp[..n]);
            process_lines(conn, app);
        }
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
        Err(_) => conn.want_close = true,
    }
}

fn process_lines(conn: &mut Conn, app: &mut App) {
    while let Some(nl_pos) = conn.rbuf.iter().position(|&b| b == b'\n') {
        let line_bytes: Vec<u8> = conn.rbuf.drain(..=nl_pos).collect();
        let line = String::from_utf8_lossy(&line_bytes[..line_bytes.len() - 1]);
        commands::dispatch(app, conn, &line);

        if conn.want_close {
            break;
        }
    }
}

fn handle_write(conn: &mut Conn) {
    while conn.wbuf_sent < conn.wbuf.len() {
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
        if conn.wbuf_sent < conn.wbuf.len() {
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
        accept_new_conns(listener, conns);
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
        if conn.want_close && conn.wbuf_sent >= conn.wbuf.len() {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        conns.remove(&fd);
    }

    app.tick();
    Ok(())
}
