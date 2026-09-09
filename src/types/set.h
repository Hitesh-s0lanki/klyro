#ifndef KLYRO_SET_H_
#define KLYRO_SET_H_

#include <stdbool.h>
#include <stddef.h>

/* An unordered collection of unique string members - the backing store
 * for the Redis-style Set data type. */

typedef struct Set Set;

Set *set_new(void);
void set_free(Set *set);

bool set_add(Set *set, const char *member);      /* true if newly added */
bool set_rem(Set *set, const char *member);      /* true if it was present */
bool set_contains(Set *set, const char *member);

size_t set_size(Set *set);

typedef void (*SetEachFn)(const char *member, void *userdata);
void set_foreach(Set *set, SetEachFn fn, void *userdata);

#endif // !KLYRO_SET_H_
