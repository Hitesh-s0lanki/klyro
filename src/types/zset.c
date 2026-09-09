/* zset.c - a sorted array of (member, score) pairs kept ordered by
 * (score, member); simpler than Redis's skip list, adequate at the scale
 * this project targets. */
#include "zset.h"

#include <stdlib.h>
#include <string.h>

typedef struct {
  char *member;
  double score;
} ZEntry;

struct Zset {
  ZEntry *entries;
  size_t len;
  size_t cap;
};

Zset *zset_new(void) {
  Zset *zset = malloc(sizeof(Zset));
  zset->entries = NULL;
  zset->len = 0;
  zset->cap = 0;
  return zset;
}

void zset_free(Zset *zset) {
  for (size_t i = 0; i < zset->len; i++) free(zset->entries[i].member);
  free(zset->entries);
  free(zset);
}

static int cmp(double score_a, const char *member_a, double score_b, const char *member_b) {
  if (score_a != score_b) return score_a < score_b ? -1 : 1;
  return strcmp(member_a, member_b);
}

static size_t find_index(Zset *zset, const char *member) {
  for (size_t i = 0; i < zset->len; i++) {
    if (strcmp(zset->entries[i].member, member) == 0) return i;
  }
  return zset->len; /* not found */
}

static void remove_at(Zset *zset, size_t idx) {
  free(zset->entries[idx].member);
  memmove(&zset->entries[idx], &zset->entries[idx + 1],
          (zset->len - idx - 1) * sizeof(ZEntry));
  zset->len--;
}

static void insert_sorted(Zset *zset, char *member, double score) {
  if (zset->len == zset->cap) {
    zset->cap = zset->cap ? zset->cap * 2 : 8;
    zset->entries = realloc(zset->entries, zset->cap * sizeof(ZEntry));
  }

  size_t idx = 0;
  while (idx < zset->len &&
         cmp(zset->entries[idx].score, zset->entries[idx].member, score, member) < 0) {
    idx++;
  }

  memmove(&zset->entries[idx + 1], &zset->entries[idx], (zset->len - idx) * sizeof(ZEntry));
  zset->entries[idx].member = member;
  zset->entries[idx].score = score;
  zset->len++;
}

bool zset_add(Zset *zset, const char *member, double score) {
  size_t idx = find_index(zset, member);
  bool is_new = idx == zset->len;
  if (!is_new) remove_at(zset, idx); /* re-insert to keep order */
  insert_sorted(zset, strdup(member), score);
  return is_new;
}

bool zset_rem(Zset *zset, const char *member) {
  size_t idx = find_index(zset, member);
  if (idx == zset->len) return false;
  remove_at(zset, idx);
  return true;
}

bool zset_score(Zset *zset, const char *member, double *out_score) {
  size_t idx = find_index(zset, member);
  if (idx == zset->len) return false;
  *out_score = zset->entries[idx].score;
  return true;
}

size_t zset_size(Zset *zset) {
  return zset->len;
}

void zset_range(Zset *zset, long start, long stop, ZsetEachFn fn, void *userdata) {
  long len = (long)zset->len;

  if (start < 0) start += len;
  if (stop < 0) stop += len;
  if (start < 0) start = 0;
  if (stop >= len) stop = len - 1;

  if (len == 0 || start > stop || start >= len) return;

  for (long i = start; i <= stop; i++) {
    fn(zset->entries[i].member, zset->entries[i].score, userdata);
  }
}
