#ifndef KLYRO_ZSET_H_
#define KLYRO_ZSET_H_

#include <stdbool.h>
#include <stddef.h>

/* A set where each member carries a score and is kept ordered by
 * (score, member) - the backing store for the Redis-style Sorted Set. */

typedef struct Zset Zset;

Zset *zset_new(void);
void zset_free(Zset *zset);

/* Adds member with score, or repositions it if it already exists.
 * Returns true if member is newly added, false if it already existed. */
bool zset_add(Zset *zset, const char *member, double score);
bool zset_rem(Zset *zset, const char *member);
bool zset_score(Zset *zset, const char *member, double *out_score);

size_t zset_size(Zset *zset);

typedef void (*ZsetEachFn)(const char *member, double score, void *userdata);
/* Iterates members in ascending order over the inclusive range
 * [start, stop]; negative indices count from the end, as in Redis's
 * ZRANGE. */
void zset_range(Zset *zset, long start, long stop, ZsetEachFn fn, void *userdata);

#endif // !KLYRO_ZSET_H_
