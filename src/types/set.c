/* set.c - a set of unique string members, built on the shared htable */
#include "set.h"
#include "util/htable.h"

#include <stdlib.h>

struct Set {
  HTable members;
};

Set *set_new(void) {
  Set *set = malloc(sizeof(Set));
  htable_init(&set->members);
  return set;
}

void set_free(Set *set) {
  htable_clear(&set->members, NULL);
  free(set);
}

bool set_add(Set *set, const char *member) {
  if (htable_find(&set->members, member)) return false;
  htable_insert(&set->members, member, NULL);
  return true;
}

bool set_rem(Set *set, const char *member) {
  HNode *node = htable_remove(&set->members, member);
  if (!node) return false;
  free(node->key);
  free(node);
  return true;
}

bool set_contains(Set *set, const char *member) {
  return htable_find(&set->members, member) != NULL;
}

size_t set_size(Set *set) {
  return htable_size(&set->members);
}

typedef struct {
  SetEachFn fn;
  void *userdata;
} ForeachCtx;

static void foreach_cb(HNode *node, void *userdata) {
  ForeachCtx *ctx = userdata;
  ctx->fn(node->key, ctx->userdata);
}

void set_foreach(Set *set, SetEachFn fn, void *userdata) {
  ForeachCtx ctx = {fn, userdata};
  htable_foreach(&set->members, foreach_cb, &ctx);
}
