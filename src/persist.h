#ifndef KLYRO_PERSIST_H_
#define KLYRO_PERSIST_H_

/* Saves/loads the whole keyspace to a single dump file on disk, so data
 * survives a restart. */

void persist_set_path(const char *path);

/* Loads the dump file at the configured path into the store, if one
 * exists. Call once at startup, after store_init() and before accepting
 * connections. */
void persist_load(void);

/* Writes the whole keyspace to the configured path (atomically, via a
 * temp file + rename). Safe to call any time the store is initialized -
 * on shutdown, or from the SAVE command. */
void persist_save(void);

/* Call periodically from the event loop; autosaves if the store has
 * pending changes and the autosave interval has elapsed. */
void persist_tick(void);

#endif // !KLYRO_PERSIST_H_
