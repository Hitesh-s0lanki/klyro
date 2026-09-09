#ifndef KLYRO_LIST_H_
#define KLYRO_LIST_H_

#include <stddef.h>

/* An ordered sequence of strings - the backing store for the Redis-style
 * List data type. */

typedef struct List List;

List *list_new(void);
void list_free(List *list);

void list_push_left(List *list, const char *value);
void list_push_right(List *list, const char *value);

/* Removes and returns the value at that end (caller frees it), or NULL
 * if the list is empty. */
char *list_pop_left(List *list);
char *list_pop_right(List *list);

size_t list_len(List *list);

/* Returns a malloc'd array of `*count` malloc'd strings (caller frees the
 * array and every string) covering the inclusive range [start, stop];
 * negative indices count from the end, as in Redis's LRANGE. */
char **list_range(List *list, long start, long stop, size_t *count);

typedef void (*ListEachFn)(const char *value, void *userdata);
/* Iterates values head to tail, e.g. for dumping the whole list. */
void list_foreach(List *list, ListEachFn fn, void *userdata);

#endif // !KLYRO_LIST_H_
