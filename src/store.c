/* store.c - the in-memory keyspace */
#include "store.h"
#include "util/htable.h"

#include <stdlib.h>
#include <string.h>
#include <time.h>

typedef struct Entry {
  StoreType type;
  union {
    char *str;
    List *list;
    Hash *hash;
    Set *set;
    Zset *zset;
  } value;
  time_t expire_at; /* 0 = never expires */
} Entry;

static HTable keyspace;
static size_t dirty = 0; /* mutation count since the last save, for autosave */

size_t store_dirty_count(void) {
  return dirty;
}

void store_reset_dirty(void) {
  dirty = 0;
}

static void entry_free_value(Entry *e) {
  switch (e->type) {
    case STORE_STRING: free(e->value.str); break;
    case STORE_LIST: list_free(e->value.list); break;
    case STORE_HASH: hash_free(e->value.hash); break;
    case STORE_SET: set_free(e->value.set); break;
    case STORE_ZSET: zset_free(e->value.zset); break;
  }
}

static void entry_free_cb(void *value) {
  Entry *e = value;
  entry_free_value(e);
  free(e);
}

void store_init(void) {
  htable_init(&keyspace);
}

void store_shutdown(void) {
  htable_clear(&keyspace, entry_free_cb);
}

/* Deletes the htable node + entry for `key` outright, no expiry check. */
static void store_erase(const char *key) {
  HNode *node = htable_remove(&keyspace, key);
  if (!node) return;
  dirty++;
  free(node->key);
  entry_free_value(node->value);
  free(node->value);
  free(node);
}

/* Finds a live (non-expired) entry for `key`, lazily erasing it if it has
 * expired. */
static Entry *store_find(const char *key) {
  HNode *node = htable_find(&keyspace, key);
  if (!node) return NULL;

  Entry *e = node->value;
  if (e->expire_at != 0 && e->expire_at <= time(NULL)) {
    store_erase(key);
    return NULL;
  }
  return e;
}

static Entry *entry_new(StoreType type) {
  Entry *e = malloc(sizeof(Entry));
  e->type = type;
  e->expire_at = 0;
  return e;
}

bool store_exists(const char *key) {
  return store_find(key) != NULL;
}

bool store_type_of(const char *key, StoreType *out_type) {
  Entry *e = store_find(key);
  if (!e) return false;
  *out_type = e->type;
  return true;
}

bool store_del(const char *key) {
  if (!store_find(key)) return false; /* also lazily expires */
  store_erase(key);
  return true;
}

void store_delete_if_empty(const char *key) {
  Entry *e = store_find(key);
  if (!e) return;

  size_t size;
  switch (e->type) {
    case STORE_STRING: return; /* strings are never emptied this way */
    case STORE_LIST: size = list_len(e->value.list); break;
    case STORE_HASH: size = hash_len(e->value.hash); break;
    case STORE_SET: size = set_size(e->value.set); break;
    case STORE_ZSET: size = zset_size(e->value.zset); break;
    default: return;
  }
  if (size == 0) store_erase(key);
}

bool store_expire(const char *key, int seconds) {
  Entry *e = store_find(key);
  if (!e) return false;
  e->expire_at = time(NULL) + seconds;
  dirty++;
  return true;
}

long store_ttl(const char *key) {
  Entry *e = store_find(key);
  if (!e) return -2;
  if (e->expire_at == 0) return -1;
  long remaining = (long)(e->expire_at - time(NULL));
  return remaining > 0 ? remaining : 0;
}

size_t store_size(void) {
  return htable_size(&keyspace);
}

typedef struct {
  char **keys;
  size_t count;
  size_t capacity;
} KeyList;

static void collect_if_expired(HNode *node, void *userdata) {
  KeyList *list = userdata;
  Entry *e = node->value;
  if (e->expire_at != 0 && e->expire_at <= time(NULL)) {
    if (list->count == list->capacity) {
      list->capacity = list->capacity ? list->capacity * 2 : 8;
      list->keys = realloc(list->keys, list->capacity * sizeof(char *));
    }
    list->keys[list->count++] = node->key; /* borrowed */
  }
}

void store_sweep_expired(void) {
  KeyList expired = {0};
  htable_foreach(&keyspace, collect_if_expired, &expired);
  for (size_t i = 0; i < expired.count; i++) {
    store_erase(expired.keys[i]);
  }
  free(expired.keys);
}

typedef struct {
  StoreEachFn fn;
  void *userdata;
} ForeachCtx;

static void foreach_key_cb(HNode *node, void *userdata) {
  ForeachCtx *ctx = userdata;
  Entry *e = node->value;
  if (e->expire_at == 0 || e->expire_at > time(NULL)) {
    ctx->fn(node->key, ctx->userdata);
  }
}

void store_foreach_key(StoreEachFn fn, void *userdata) {
  ForeachCtx ctx = {fn, userdata};
  htable_foreach(&keyspace, foreach_key_cb, &ctx);
}

typedef struct {
  StoreEachEntryFn fn;
  void *userdata;
} ForeachEntryCtx;

static void foreach_entry_cb(HNode *node, void *userdata) {
  ForeachEntryCtx *ctx = userdata;
  Entry *e = node->value;
  if (e->expire_at == 0 || e->expire_at > time(NULL)) {
    ctx->fn(node->key, e->type, ctx->userdata);
  }
}

void store_foreach_entry(StoreEachEntryFn fn, void *userdata) {
  ForeachEntryCtx ctx = {fn, userdata};
  htable_foreach(&keyspace, foreach_entry_cb, &ctx);
}

void store_set_string(const char *key, const char *value) {
  dirty++;
  Entry *e = store_find(key);
  if (e) {
    entry_free_value(e);
    e->type = STORE_STRING;
    e->value.str = strdup(value);
    e->expire_at = 0; /* SET clears any previous expiry, matching Redis */
    return;
  }

  e = entry_new(STORE_STRING);
  e->value.str = strdup(value);
  htable_insert(&keyspace, key, e);
}

const char *store_get_string(const char *key) {
  Entry *e = store_find(key);
  if (!e || e->type != STORE_STRING) return NULL;
  return e->value.str;
}

#define DEFINE_COLLECTION_ACCESSORS(Type, type_enum, field, ctor)      \
  Type *store_get_or_create_##field(const char *key) {                \
    dirty++; /* every caller is about to mutate the returned value */ \
    Entry *e = store_find(key);                                       \
    if (e) return e->type == (type_enum) ? e->value.field : NULL;     \
    e = entry_new(type_enum);                                         \
    e->value.field = ctor();                                         \
    htable_insert(&keyspace, key, e);                                 \
    return e->value.field;                                            \
  }                                                                    \
  Type *store_get_existing_##field(const char *key) {                 \
    Entry *e = store_find(key);                                       \
    if (!e || e->type != (type_enum)) return NULL;                    \
    return e->value.field;                                            \
  }

DEFINE_COLLECTION_ACCESSORS(List, STORE_LIST, list, list_new)
DEFINE_COLLECTION_ACCESSORS(Hash, STORE_HASH, hash, hash_new)
DEFINE_COLLECTION_ACCESSORS(Set, STORE_SET, set, set_new)
DEFINE_COLLECTION_ACCESSORS(Zset, STORE_ZSET, zset, zset_new)

#undef DEFINE_COLLECTION_ACCESSORS
