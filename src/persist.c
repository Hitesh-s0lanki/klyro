/* persist.c - saves/loads the whole keyspace to a single dump file */
#include "persist.h"
#include "store.h"
#include "types/hash.h"
#include "types/list.h"
#include "types/set.h"
#include "types/zset.h"
#include "util/strutil.h"

#include <stdbool.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#define DUMP_MAGIC "KLYRO-DUMP 1"
#define DUMP_LINE_MAX (80 * 1024) /* headroom over the 64 KiB protocol value cap */
#define AUTOSAVE_INTERVAL_SEC 60

static char dump_path[4096] = "klyro.dump";

void persist_set_path(const char *path) {
  snprintf(dump_path, sizeof(dump_path), "%s", path);
}

/* --- saving --- */

static void write_list_elem(const char *value, void *userdata) {
  fprintf((FILE *)userdata, "%s\n", value);
}

static void write_hash_field(const char *field, const char *value, void *userdata) {
  fprintf((FILE *)userdata, "%s %s\n", field, value);
}

static void write_set_member(const char *member, void *userdata) {
  fprintf((FILE *)userdata, "%s\n", member);
}

static void write_zset_member(const char *member, double score, void *userdata) {
  fprintf((FILE *)userdata, "%s %.17g\n", member, score);
}

static void dump_entry(const char *key, StoreType type, void *userdata) {
  FILE *f = userdata;

  switch (type) {
    case STORE_STRING:
      fprintf(f, "STRING %s %s\n", key, store_get_string(key));
      break;
    case STORE_LIST: {
      List *list = store_get_existing_list(key);
      fprintf(f, "LIST %s %zu\n", key, list_len(list));
      list_foreach(list, write_list_elem, f);
      break;
    }
    case STORE_HASH: {
      Hash *hash = store_get_existing_hash(key);
      fprintf(f, "HASH %s %zu\n", key, hash_len(hash));
      hash_foreach(hash, write_hash_field, f);
      break;
    }
    case STORE_SET: {
      Set *set = store_get_existing_set(key);
      fprintf(f, "SET %s %zu\n", key, set_size(set));
      set_foreach(set, write_set_member, f);
      break;
    }
    case STORE_ZSET: {
      Zset *zset = store_get_existing_zset(key);
      fprintf(f, "ZSET %s %zu\n", key, zset_size(zset));
      zset_range(zset, 0, -1, write_zset_member, f);
      break;
    }
  }

  long ttl = store_ttl(key);
  if (ttl >= 0) {
    fprintf(f, "EXPIREAT %s %ld\n", key, (long)time(NULL) + ttl);
  }
}

void persist_save(void) {
  char tmp_path[sizeof(dump_path) + 8];
  snprintf(tmp_path, sizeof(tmp_path), "%s.tmp", dump_path);

  FILE *f = fopen(tmp_path, "w");
  if (!f) {
    perror("persist_save: fopen");
    return;
  }

  fprintf(f, "%s\n", DUMP_MAGIC);
  store_foreach_entry(dump_entry, f);
  fclose(f);

  if (rename(tmp_path, dump_path) != 0) {
    perror("persist_save: rename");
  }
  store_reset_dirty();
}

/* --- loading --- */

static void load_list(FILE *f, const char *key, int count) {
  List *list = store_get_or_create_list(key);
  char line[DUMP_LINE_MAX];
  for (int i = 0; i < count && fgets(line, sizeof(line), f); i++) {
    list_push_right(list, trim(line));
  }
}

static void load_hash(FILE *f, const char *key, int count) {
  Hash *hash = store_get_or_create_hash(key);
  char line[DUMP_LINE_MAX];
  for (int i = 0; i < count && fgets(line, sizeof(line), f); i++) {
    char *rest = trim(line);
    char *field = next_token(&rest);
    if (field) hash_set(hash, field, rest);
  }
}

static void load_set(FILE *f, const char *key, int count) {
  Set *set = store_get_or_create_set(key);
  char line[DUMP_LINE_MAX];
  for (int i = 0; i < count && fgets(line, sizeof(line), f); i++) {
    set_add(set, trim(line));
  }
}

static void load_zset(FILE *f, const char *key, int count) {
  Zset *zset = store_get_or_create_zset(key);
  char line[DUMP_LINE_MAX];
  for (int i = 0; i < count && fgets(line, sizeof(line), f); i++) {
    char *rest = trim(line);
    char *member = next_token(&rest);
    double score;
    if (member && parse_double(rest, &score)) zset_add(zset, member, score);
  }
}

void persist_load(void) {
  FILE *f = fopen(dump_path, "r");
  if (!f) return; /* no dump yet; nothing to load */

  char line[DUMP_LINE_MAX];
  if (!fgets(line, sizeof(line), f) || strncmp(line, "KLYRO-DUMP", 10) != 0) {
    fprintf(stderr, "persist_load: %s is not a valid Klyro dump, ignoring\n", dump_path);
    fclose(f);
    return;
  }

  size_t loaded = 0;
  while (fgets(line, sizeof(line), f)) {
    char *rest = trim(line);
    char *type_tag = next_token(&rest);
    char *key = next_token(&rest);
    if (!type_tag || !key) continue;

    int count;
    if (strcmp(type_tag, "STRING") == 0) {
      store_set_string(key, rest);
      loaded++;
    } else if (strcmp(type_tag, "LIST") == 0) {
      if (parse_int(rest, &count)) { load_list(f, key, count); loaded++; }
    } else if (strcmp(type_tag, "HASH") == 0) {
      if (parse_int(rest, &count)) { load_hash(f, key, count); loaded++; }
    } else if (strcmp(type_tag, "SET") == 0) {
      if (parse_int(rest, &count)) { load_set(f, key, count); loaded++; }
    } else if (strcmp(type_tag, "ZSET") == 0) {
      if (parse_int(rest, &count)) { load_zset(f, key, count); loaded++; }
    } else if (strcmp(type_tag, "EXPIREAT") == 0) {
      long expire_at;
      if (parse_long(rest, &expire_at)) {
        long remaining = expire_at - (long)time(NULL);
        if (remaining <= 0) store_del(key);
        else store_expire(key, (int)remaining);
      }
    }
  }

  fclose(f);
  store_reset_dirty(); /* freshly loaded from disk; nothing pending to save */
  printf("loaded %zu key(s) from %s\n", loaded, dump_path);
}

void persist_tick(void) {
  static time_t last_check = 0;
  static bool seeded = false;
  time_t now = time(NULL);
  if (!seeded) {
    last_check = now;
    seeded = true;
    return;
  }
  if (now - last_check < AUTOSAVE_INTERVAL_SEC) return;
  last_check = now;

  if (store_dirty_count() > 0) persist_save();
}
