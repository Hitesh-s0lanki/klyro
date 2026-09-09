#ifndef KLYRO_COMMANDS_H_
#define KLYRO_COMMANDS_H_

#include "server.h"

void commands_init(void);
void commands_shutdown(void);

/* Parses and executes one command line, replying on conn. */
void commands_dispatch(Conn *conn, char *line);

/* Periodic housekeeping (e.g. expired-key sweep); called from the event loop. */
void commands_tick(void);

#endif // !KLYRO_COMMANDS_H_
