/* htable.c - a small string-keyed chaining hashtable */
#include "htable.h"

#include <stdlib.h>
#include <string.h>

#define HTABLE_INITIAL_BUCKETS 16
#define HTABLE_MAX_LOAD 4

static uint64_t hash_key(const char *key) {
  /* FNV-1a */
  uint64_t h = 0xcbf29ce484222325ULL;
  for (const unsigned char *p = (const unsigned char *)key; *p; p++) {
    h ^= *p;
    h *= 0x100000001b3ULL;
  }
  return h;
}

void htable_init(HTable *t) {
  t->nbuckets = HTABLE_INITIAL_BUCKETS;
  t->buckets = calloc(t->nbuckets, sizeof(HNode *));
  t->size = 0;
}

void htable_clear(HTable *t, void (*free_value)(void *value)) {
  for (size_t i = 0; i < t->nbuckets; i++) {
    HNode *n = t->buckets[i];
    while (n) {
      HNode *next = n->next;
      free(n->key);
      if (free_value) free_value(n->value);
      free(n);
      n = next;
    }
  }
  free(t->buckets);
  t->buckets = NULL;
  t->nbuckets = 0;
  t->size = 0;
}

static void htable_resize(HTable *t, size_t new_nbuckets) {
  HNode **new_buckets = calloc(new_nbuckets, sizeof(HNode *));
  for (size_t i = 0; i < t->nbuckets; i++) {
    HNode *n = t->buckets[i];
    while (n) {
      HNode *next = n->next;
      size_t idx = (size_t)(hash_key(n->key) % new_nbuckets);
      n->next = new_buckets[idx];
      new_buckets[idx] = n;
      n = next;
    }
  }
  free(t->buckets);
  t->buckets = new_buckets;
  t->nbuckets = new_nbuckets;
}

HNode *htable_find(HTable *t, const char *key) {
  size_t idx = (size_t)(hash_key(key) % t->nbuckets);
  for (HNode *n = t->buckets[idx]; n; n = n->next) {
    if (strcmp(n->key, key) == 0) return n;
  }
  return NULL;
}

void htable_insert(HTable *t, const char *key, void *value) {
  if (t->size + 1 > t->nbuckets * HTABLE_MAX_LOAD) {
    htable_resize(t, t->nbuckets * 2);
  }

  size_t idx = (size_t)(hash_key(key) % t->nbuckets);
  HNode *n = malloc(sizeof(HNode));
  n->key = strdup(key);
  n->value = value;
  n->next = t->buckets[idx];
  t->buckets[idx] = n;
  t->size++;
}

HNode *htable_remove(HTable *t, const char *key) {
  size_t idx = (size_t)(hash_key(key) % t->nbuckets);
  HNode **prev = &t->buckets[idx];
  while (*prev) {
    HNode *n = *prev;
    if (strcmp(n->key, key) == 0) {
      *prev = n->next;
      t->size--;
      return n;
    }
    prev = &n->next;
  }
  return NULL;
}

size_t htable_size(HTable *t) {
  return t->size;
}

void htable_foreach(HTable *t, HTableEachFn fn, void *userdata) {
  for (size_t i = 0; i < t->nbuckets; i++) {
    for (HNode *n = t->buckets[i]; n; n = n->next) {
      fn(n, userdata);
    }
  }
}

size_t htable_scan(HTable *t, size_t start_bucket, size_t min_count, HTableEachFn fn,
                    void *userdata) {
  size_t visited = 0;
  for (size_t i = start_bucket; i < t->nbuckets; i++) {
    for (HNode *n = t->buckets[i]; n; n = n->next) {
      fn(n, userdata);
      visited++;
    }
    if (min_count > 0 && visited >= min_count) {
      size_t next = i + 1;
      return next >= t->nbuckets ? 0 : next;
    }
  }
  return 0;
}
