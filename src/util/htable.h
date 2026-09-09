#ifndef KLYRO_HTABLE_H_
#define KLYRO_HTABLE_H_

#include <stddef.h>
#include <stdint.h>

/* A small string-keyed, chaining hashtable: the shared building block
 * behind the top-level keyspace (store.c) and the Hash/Set data types.
 * Each node owns its own copy of the key; the `value` pointer is owned
 * and interpreted by the caller. */

typedef struct HNode {
  char *key;
  void *value;
  struct HNode *next;
} HNode;

typedef struct {
  HNode **buckets;
  size_t nbuckets;
  size_t size;
} HTable;

void htable_init(HTable *t);

/* Frees the table's bookkeeping. `free_value` (may be NULL) is called on
 * every node's value before its node is freed. */
void htable_clear(HTable *t, void (*free_value)(void *value));

HNode *htable_find(HTable *t, const char *key);

/* Inserts a new node for `key` (which must not already exist), taking
 * ownership of `value`. Resizes the table if the load factor warrants it. */
void htable_insert(HTable *t, const char *key, void *value);

/* Unlinks and returns the node for `key` (caller owns and must free its
 * key/value/node), or NULL if absent. */
HNode *htable_remove(HTable *t, const char *key);

size_t htable_size(HTable *t);

typedef void (*HTableEachFn)(HNode *node, void *userdata);
void htable_foreach(HTable *t, HTableEachFn fn, void *userdata);

#endif // !KLYRO_HTABLE_H_
