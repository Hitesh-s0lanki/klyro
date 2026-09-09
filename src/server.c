/* server.c - TCP networking and the poll()-based event loop */
#include "server.h"
#include "commands.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define MAX_MSG (64 * 1024)

struct Conn {
  int fd;
  bool want_close;
  char rbuf[MAX_MSG];
  size_t rbuf_size;
  char wbuf[MAX_MSG];
  size_t wbuf_size;
  size_t wbuf_sent;
};

static bool running;
static int listen_fd = -1;
static Conn **conns = NULL;
static size_t conns_len = 0;

static void die(const char *msg) {
  perror(msg);
  exit(EXIT_FAILURE);
}

static void set_nonblocking(int fd) {
  int flags = fcntl(fd, F_GETFL, 0);
  if (flags == -1) die("fcntl(F_GETFL)");
  if (fcntl(fd, F_SETFL, flags | O_NONBLOCK) == -1) die("fcntl(F_SETFL)");
}

static void on_signal(int sig) {
  (void)sig;
  running = false;
}

void server_stop(void) {
  running = false;
}

static void conn_put(Conn *conn) {
  if ((size_t)conn->fd >= conns_len) {
    size_t new_len = conns_len == 0 ? 16 : conns_len * 2;
    while (new_len <= (size_t)conn->fd) new_len *= 2;
    conns = realloc(conns, new_len * sizeof(Conn *));
    for (size_t i = conns_len; i < new_len; i++) conns[i] = NULL;
    conns_len = new_len;
  }
  conns[conn->fd] = conn;
}

static void conn_close(Conn *conn) {
  close(conn->fd);
  conns[conn->fd] = NULL;
  free(conn);
}

static void wbuf_append(Conn *conn, const char *data, size_t len) {
  if (conn->wbuf_size + len > sizeof(conn->wbuf)) {
    len = sizeof(conn->wbuf) - conn->wbuf_size;
  }
  memcpy(conn->wbuf + conn->wbuf_size, data, len);
  conn->wbuf_size += len;
}

void conn_reply(Conn *conn, const char *fmt, ...) {
  char buf[1024];
  va_list args;
  va_start(args, fmt);
  int n = vsnprintf(buf, sizeof(buf), fmt, args);
  va_end(args);
  if (n > 0) wbuf_append(conn, buf, (size_t)n);
}

void conn_request_close(Conn *conn) {
  conn->want_close = true;
}

static void conn_process_lines(Conn *conn) {
  for (;;) {
    char *nl = memchr(conn->rbuf, '\n', conn->rbuf_size);
    if (!nl) break;

    size_t line_len = (size_t)(nl - conn->rbuf);
    conn->rbuf[line_len] = '\0';
    commands_dispatch(conn, conn->rbuf);

    size_t consumed = line_len + 1;
    memmove(conn->rbuf, conn->rbuf + consumed, conn->rbuf_size - consumed);
    conn->rbuf_size -= consumed;

    if (conn->want_close) break;
  }
}

static void conn_handle_read(Conn *conn) {
  if (conn->rbuf_size == sizeof(conn->rbuf)) {
    conn_reply(conn, "ERR line too long\r\n");
    conn->rbuf_size = 0;
    return;
  }

  ssize_t n = read(conn->fd, conn->rbuf + conn->rbuf_size, sizeof(conn->rbuf) - conn->rbuf_size);
  if (n == 0) {
    conn->want_close = true;
    return;
  }
  if (n < 0) {
    if (errno == EAGAIN || errno == EWOULDBLOCK) return;
    conn->want_close = true;
    return;
  }

  conn->rbuf_size += (size_t)n;
  conn_process_lines(conn);
}

static void conn_handle_write(Conn *conn) {
  while (conn->wbuf_sent < conn->wbuf_size) {
    ssize_t n = write(conn->fd, conn->wbuf + conn->wbuf_sent, conn->wbuf_size - conn->wbuf_sent);
    if (n < 0) {
      if (errno == EAGAIN || errno == EWOULDBLOCK) return;
      conn->want_close = true;
      return;
    }
    conn->wbuf_sent += (size_t)n;
  }
  conn->wbuf_size = 0;
  conn->wbuf_sent = 0;
}

static void accept_new_conns(void) {
  for (;;) {
    struct sockaddr_in addr;
    socklen_t addr_len = sizeof(addr);
    int fd = accept(listen_fd, (struct sockaddr *)&addr, &addr_len);
    if (fd < 0) {
      if (errno != EAGAIN && errno != EWOULDBLOCK) perror("accept");
      return;
    }

    set_nonblocking(fd);
    int one = 1;
    setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one));

    Conn *conn = calloc(1, sizeof(Conn));
    conn->fd = fd;
    conn_put(conn);
  }
}

static void poll_once(void) {
  static struct pollfd *poll_fds = NULL;
  static size_t poll_fds_cap = 0;

  size_t nfds_needed = conns_len + 1;
  if (nfds_needed > poll_fds_cap) {
    poll_fds_cap = nfds_needed;
    poll_fds = realloc(poll_fds, poll_fds_cap * sizeof(struct pollfd));
  }

  size_t nfds = 0;
  poll_fds[nfds].fd = listen_fd;
  poll_fds[nfds].events = POLLIN;
  nfds++;

  for (size_t i = 0; i < conns_len; i++) {
    Conn *conn = conns[i];
    if (!conn) continue;
    short events = 0;
    if (!conn->want_close) events |= POLLIN;
    if (conn->wbuf_size > conn->wbuf_sent) events |= POLLOUT;
    poll_fds[nfds].fd = conn->fd;
    poll_fds[nfds].events = events;
    nfds++;
  }

  int nready = poll(poll_fds, nfds, 1000);
  if (nready < 0) {
    if (errno == EINTR) return;
    die("poll");
  }

  if (poll_fds[0].revents & POLLIN) accept_new_conns();

  for (size_t i = 1; i < nfds; i++) {
    int fd = poll_fds[i].fd;
    Conn *conn = conns[fd];
    if (!conn) continue;
    short re = poll_fds[i].revents;

    if (re & (POLLERR | POLLHUP)) conn->want_close = true;
    if (re & POLLIN) conn_handle_read(conn);
    if (re & POLLOUT) conn_handle_write(conn);

    if (conn->want_close && conn->wbuf_sent >= conn->wbuf_size) {
      conn_close(conn);
    }
  }

  commands_tick();
}

void server_init(int port) {
  listen_fd = socket(AF_INET, SOCK_STREAM, 0);
  if (listen_fd < 0) die("socket");

  int val = 1;
  setsockopt(listen_fd, SOL_SOCKET, SO_REUSEADDR, &val, sizeof(val));

  struct sockaddr_in addr = {0};
  addr.sin_family = AF_INET;
  addr.sin_addr.s_addr = htonl(INADDR_ANY);
  addr.sin_port = htons((uint16_t)port);

  if (bind(listen_fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) die("bind");
  if (listen(listen_fd, 128) < 0) die("listen");

  set_nonblocking(listen_fd);

  signal(SIGPIPE, SIG_IGN);
  signal(SIGINT, on_signal);
  signal(SIGTERM, on_signal);
}

void server_run(void) {
  running = true;
  while (running) {
    poll_once();
  }
}

void server_shutdown(void) {
  for (size_t i = 0; i < conns_len; i++) {
    if (conns[i]) {
      conn_handle_write(conns[i]); /* best-effort flush of any queued reply */
      conn_close(conns[i]);
    }
  }
  free(conns);
  conns = NULL;
  conns_len = 0;

  close(listen_fd);
  listen_fd = -1;
}
