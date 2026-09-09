#ifndef KLYRO_HASH_H_
#define KLYRO_HASH_H_

#include <stdbool.h>
#include <stddef.h>

/* A field -> value map - the backing store for the Redis-style Hash data
 * type. */

typedef struct Hash Hash;

Hash *hash_new(void);
void hash_free(Hash *hash);

void hash_set(Hash *hash, const char *field, const char *value);
const char *hash_get(Hash *hash, const char *field);
bool hash_del(Hash *hash, const char *field);

size_t hash_len(Hash *hash);

typedef void (*HashEachFn)(const char *field, const char *value, void *userdata);
void hash_foreach(Hash *hash, HashEachFn fn, void *userdata);

#endif // !KLYRO_HASH_H_
