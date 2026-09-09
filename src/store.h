#ifndef KLYRO_STORE_H_
#define KLYRO_STORE_H_

#include <stdbool.h>
#include <stddef.h>

#include "types/hash.h"
#include "types/list.h"
#include "types/set.h"
#include "types/zset.h"

/* The in-memory keyspace: maps keys to typed values (string, list, hash,
 * set, or sorted set), with optional per-key expiry (lazy on lookup plus
 * a periodic active sweep). */

typedef enum {
  STORE_STRING,
  STORE_LIST,
  STORE_HASH,
  STORE_SET,
  STORE_ZSET,
} StoreType;

void store_init(void);
void store_shutdown(void);

/* Generic key operations, valid regardless of type. */
bool store_exists(const char *key);
bool store_type_of(const char *key, StoreType *out_type);
bool store_del(const char *key);
/* Deletes key if it holds an empty collection (list/hash/set/zset with no
 * elements left) - matches Redis's "empty collections don't exist". */
void store_delete_if_empty(const char *key);

bool store_expire(const char *key, int seconds);
long store_ttl(const char *key); /* -2 missing, -1 no expiry, else seconds left */

size_t store_size(void);
void store_sweep_expired(void);

typedef void (*StoreEachFn)(const char *key, void *userdata);
void store_foreach_key(StoreEachFn fn, void *userdata);

/* Resumable key iteration for the SCAN command - see htable_scan's doc
 * comment for the cursor convention and its caveats. */
size_t store_scan(size_t start_cursor, size_t min_count, StoreEachFn fn, void *userdata);

typedef void (*StoreEachEntryFn)(const char *key, StoreType type, void *userdata);
/* Like store_foreach_key, but also passes each entry's type - the hook
 * persistence uses to dump the whole keyspace. */
void store_foreach_entry(StoreEachEntryFn fn, void *userdata);

/* Counts mutations (set/push/add/del/expire/...) since the store was
 * created or since the last store_reset_dirty() - used to decide when a
 * periodic autosave is worth doing. */
size_t store_dirty_count(void);
void store_reset_dirty(void);

/* String type. store_set_string clears any existing expiry, matching
 * Redis's SET; store_update_string keeps it, matching Redis's
 * INCR/DECR/APPEND/SETRANGE (an in-place mutation, not a fresh SET). */
void store_set_string(const char *key, const char *value);
void store_update_string(const char *key, const char *value);
const char *store_get_string(const char *key); /* NULL if missing or wrong type */

/* Collection types. "get_or_create" makes a new empty collection if the
 * key is absent; "get_existing" never creates. Both return NULL if the
 * key holds a different type. */
List *store_get_or_create_list(const char *key);
List *store_get_existing_list(const char *key);

Hash *store_get_or_create_hash(const char *key);
Hash *store_get_existing_hash(const char *key);

Set *store_get_or_create_set(const char *key);
Set *store_get_existing_set(const char *key);

Zset *store_get_or_create_zset(const char *key);
Zset *store_get_existing_zset(const char *key);

#endif // !KLYRO_STORE_H_
