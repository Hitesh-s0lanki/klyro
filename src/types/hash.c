/* hash.c - a field/value map, built on the shared htable */
#include "hash.h"
#include "util/htable.h"

#include <stdlib.h>
#include <string.h>

struct Hash {
  HTable fields;
};

Hash *hash_new(void) {
  Hash *hash = malloc(sizeof(Hash));
  htable_init(&hash->fields);
  return hash;
}

void hash_free(Hash *hash) {
  htable_clear(&hash->fields, free);
  free(hash);
}

void hash_set(Hash *hash, const char *field, const char *value) {
  HNode *node = htable_find(&hash->fields, field);
  if (node) {
    free(node->value);
    node->value = strdup(value);
    return;
  }
  htable_insert(&hash->fields, field, strdup(value));
}

const char *hash_get(Hash *hash, const char *field) {
  HNode *node = htable_find(&hash->fields, field);
  return node ? (const char *)node->value : NULL;
}

bool hash_del(Hash *hash, const char *field) {
  HNode *node = htable_remove(&hash->fields, field);
  if (!node) return false;
  free(node->key);
  free(node->value);
  free(node);
  return true;
}

size_t hash_len(Hash *hash) {
  return htable_size(&hash->fields);
}

typedef struct {
  HashEachFn fn;
  void *userdata;
} ForeachCtx;

static void foreach_cb(HNode *node, void *userdata) {
  ForeachCtx *ctx = userdata;
  ctx->fn(node->key, (const char *)node->value, ctx->userdata);
}

void hash_foreach(Hash *hash, HashEachFn fn, void *userdata) {
  ForeachCtx ctx = {fn, userdata};
  htable_foreach(&hash->fields, foreach_cb, &ctx);
}
