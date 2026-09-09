#ifndef KLYRO_SERVER_H_
#define KLYRO_SERVER_H_

/* Opaque connection handle; internals live in server.c. */
typedef struct Conn Conn;

void server_init(int port);
void server_run(void);
void server_shutdown(void);

/* Requests a graceful stop of server_run()'s loop. Safe to call from a
 * signal handler or from a command handler. */
void server_stop(void);

/* Queues a formatted reply on a connection's write buffer. */
void conn_reply(Conn *conn, const char *fmt, ...);

/* Marks a connection to be closed once its queued replies are flushed. */
void conn_request_close(Conn *conn);

#endif // !KLYRO_SERVER_H_
