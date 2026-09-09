/* main.c */
#include "klyro.h"
#include "commands.h"
#include "persist.h"
#include "server.h"

#include <stdio.h>
#include <stdlib.h>

#define DEFAULT_PORT 7171
#define DEFAULT_DUMP_PATH "klyro.dump"

int main(int argc, char *argv[]) {
  int port = DEFAULT_PORT;
  if (argc > 1) {
    port = atoi(argv[1]);
    if (port <= 0 || port > 65535) {
      fprintf(stderr, "usage: %s [port] [dump-file]\n", argv[0]);
      return EXIT_FAILURE;
    }
  }
  const char *dump_path = argc > 2 ? argv[2] : DEFAULT_DUMP_PATH;

  printf("%s %s - %s\n", KLYRO_NAME, KLYRO_VERSION, KLYRO_TAGLINE);

  commands_init();
  persist_set_path(dump_path);
  persist_load();

  server_init(port);
  printf("listening on port %d\n", port);

  server_run();

  server_shutdown();
  persist_save();
  commands_shutdown();

  printf("ok\n");
  return EXIT_SUCCESS;
}
