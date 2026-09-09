/* commands.c - command line parsing and dispatch */
#include "commands.h"
#include "persist.h"
#include "store.h"
#include "types/hash.h"
#include "types/list.h"
#include "types/set.h"
#include "types/zset.h"
#include "util/glob.h"
#include "util/strutil.h"

#include <limits.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <time.h>

#define SWEEP_INTERVAL_SEC 1
#define MAX_ZADD_PAIRS 128
#define MAX_STRING_LEN (64 * 1024) /* matches the network protocol's line-length cap */

void commands_init(void) {
  store_init();
}

void commands_shutdown(void) {
  store_shutdown();
}

void commands_tick(void) {
  static time_t last_sweep = 0;
  time_t now = time(NULL);
  if (now - last_sweep >= SWEEP_INTERVAL_SEC) {
    store_sweep_expired();
    last_sweep = now;
  }
  persist_tick();
}

/* Replies WRONGTYPE and returns false if `key` exists with a type other
 * than `want`; otherwise (missing, or already the right type) returns
 * true and replies nothing. */
static bool check_type(Conn *conn, const char *key, StoreType want) {
  StoreType t;
  if (store_type_of(key, &t) && t != want) {
    conn_reply(conn, "ERR WRONGTYPE Operation against a key holding the wrong kind of value\r\n");
    return false;
  }
  return true;
}

static void append_line(const char *item, void *userdata) {
  conn_reply((Conn *)userdata, "%s\r\n", item);
}

typedef struct {
  Conn *conn;
  const char *pattern; /* NULL = match every key, used by KEYS and SCAN */
} KeyFilterCtx;

static void emit_key_if_match(const char *key, void *userdata) {
  KeyFilterCtx *ctx = userdata;
  if (!ctx->pattern || glob_match(ctx->pattern, key)) {
    conn_reply(ctx->conn, "%s\r\n", key);
  }
}

static void append_field_value_lines(const char *field, const char *value, void *userdata) {
  conn_reply((Conn *)userdata, "%s\r\n%s\r\n", field, value);
}

static void append_member_score_line(const char *member, double score, void *userdata) {
  conn_reply((Conn *)userdata, "%s %g\r\n", member, score);
}

void commands_dispatch(Conn *conn, char *line) {
  line = trim(line);
  if (*line == '\0') return;

  char *cmd = next_token(&line);
  char *rest = line;

  if (strcasecmp(cmd, "PING") == 0) {
    conn_reply(conn, "PONG\r\n");

  /* --- generic key commands --- */

  } else if (strcasecmp(cmd, "DEL") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: DEL key\r\n"); return; }
    conn_reply(conn, store_del(key) ? "OK\r\n" : "NOT_FOUND\r\n");

  } else if (strcasecmp(cmd, "EXPIRE") == 0) {
    char *key = next_token(&rest);
    int secs;
    if (!key || !parse_int(rest, &secs)) {
      conn_reply(conn, "ERR usage: EXPIRE key seconds\r\n");
      return;
    }
    conn_reply(conn, store_expire(key, secs) ? "OK\r\n" : "NOT_FOUND\r\n");

  } else if (strcasecmp(cmd, "TTL") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: TTL key\r\n"); return; }
    conn_reply(conn, "TTL %ld\r\n", store_ttl(key));

  } else if (strcasecmp(cmd, "TYPE") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: TYPE key\r\n"); return; }
    StoreType t;
    if (!store_type_of(key, &t)) { conn_reply(conn, "NONE\r\n"); return; }
    static const char *names[] = {"STRING", "LIST", "HASH", "SET", "ZSET"};
    conn_reply(conn, "%s\r\n", names[t]);

  } else if (strcasecmp(cmd, "KEYS") == 0) {
    char *pattern = trim(rest); /* empty = every key, matching prior behavior */
    KeyFilterCtx ctx = {conn, *pattern ? pattern : NULL};
    store_foreach_key(emit_key_if_match, &ctx);
    conn_reply(conn, "END\r\n");

  } else if (strcasecmp(cmd, "SCAN") == 0) {
    char *cursor_str = next_token(&rest);
    long cursor;
    if (!cursor_str || !parse_long(cursor_str, &cursor) || cursor < 0) {
      conn_reply(conn, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
      return;
    }

    const char *pattern = NULL;
    long count = 10;
    for (char *opt = next_token(&rest); opt; opt = next_token(&rest)) {
      if (strcasecmp(opt, "MATCH") == 0) {
        pattern = next_token(&rest);
        if (!pattern) {
          conn_reply(conn, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
          return;
        }
      } else if (strcasecmp(opt, "COUNT") == 0) {
        char *count_str = next_token(&rest);
        if (!count_str || !parse_long(count_str, &count) || count <= 0) {
          conn_reply(conn, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
          return;
        }
      } else {
        conn_reply(conn, "ERR usage: SCAN cursor [MATCH pattern] [COUNT count]\r\n");
        return;
      }
    }

    KeyFilterCtx ctx = {conn, pattern};
    size_t next_cursor = store_scan((size_t)cursor, (size_t)count, emit_key_if_match, &ctx);
    conn_reply(conn, "CURSOR %zu\r\n", next_cursor);

  } else if (strcasecmp(cmd, "DBSIZE") == 0) {
    conn_reply(conn, "COUNT %zu\r\n", store_size());

  /* --- string commands --- */

  } else if (strcasecmp(cmd, "SET") == 0) {
    char *key = next_token(&rest);
    char *value = rest;
    if (!key || *value == '\0') {
      conn_reply(conn, "ERR usage: SET key value\r\n");
      return;
    }
    store_set_string(key, value);
    conn_reply(conn, "OK\r\n");

  } else if (strcasecmp(cmd, "GET") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: GET key\r\n"); return; }
    if (!check_type(conn, key, STORE_STRING)) return;
    const char *value = store_get_string(key);
    if (value) conn_reply(conn, "VALUE %s\r\n", value);
    else conn_reply(conn, "NOT_FOUND\r\n");

  } else if (strcasecmp(cmd, "INCR") == 0 || strcasecmp(cmd, "DECR") == 0) {
    char *key = trim(rest);
    if (*key == '\0') {
      conn_reply(conn, "ERR usage: %s key\r\n", cmd);
      return;
    }
    if (!check_type(conn, key, STORE_STRING)) return;
    const char *current = store_get_string(key);
    long value = 0;
    if (current && !parse_long(current, &value)) {
      conn_reply(conn, "ERR value is not an integer\r\n");
      return;
    }
    bool incr = strcasecmp(cmd, "INCR") == 0;
    if ((incr && value == LONG_MAX) || (!incr && value == LONG_MIN)) {
      conn_reply(conn, "ERR increment or decrement would overflow\r\n");
      return;
    }
    value += incr ? 1 : -1;
    char buf[32];
    snprintf(buf, sizeof(buf), "%ld", value);
    store_update_string(key, buf);
    conn_reply(conn, "VALUE %ld\r\n", value);

  } else if (strcasecmp(cmd, "APPEND") == 0) {
    char *key = next_token(&rest);
    char *value = rest;
    if (!key || *value == '\0') {
      conn_reply(conn, "ERR usage: APPEND key value\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_STRING)) return;
    const char *current = store_get_string(key);
    size_t old_len = current ? strlen(current) : 0;
    size_t add_len = strlen(value);
    if (old_len + add_len > MAX_STRING_LEN) {
      conn_reply(conn, "ERR resulting string too long\r\n");
      return;
    }
    char *combined = malloc(old_len + add_len + 1);
    if (current) memcpy(combined, current, old_len);
    memcpy(combined + old_len, value, add_len + 1); /* + the value's '\0' */
    store_update_string(key, combined);
    free(combined);
    conn_reply(conn, "LEN %zu\r\n", old_len + add_len);

  } else if (strcasecmp(cmd, "GETRANGE") == 0) {
    char *key = next_token(&rest);
    char *start_str = next_token(&rest);
    long start, end;
    if (!key || !start_str || !parse_long(start_str, &start) || !parse_long(rest, &end)) {
      conn_reply(conn, "ERR usage: GETRANGE key start end\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_STRING)) return;
    const char *value = store_get_string(key);
    if (!value) value = "";
    long len = (long)strlen(value);

    if (start < 0) start += len;
    if (end < 0) end += len;
    if (start < 0) start = 0;
    if (end >= len) end = len - 1;

    if (len == 0 || start > end || start >= len) {
      conn_reply(conn, "VALUE \r\n");
      return;
    }
    conn_reply(conn, "VALUE %.*s\r\n", (int)(end - start + 1), value + start);

  } else if (strcasecmp(cmd, "SETRANGE") == 0) {
    char *key = next_token(&rest);
    char *offset_str = next_token(&rest);
    char *value = rest;
    long offset;
    if (!key || !offset_str || !parse_long(offset_str, &offset) || offset < 0 ||
        *value == '\0') {
      conn_reply(conn, "ERR usage: SETRANGE key offset value\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_STRING)) return;
    const char *current = store_get_string(key);
    size_t old_len = current ? strlen(current) : 0;
    size_t add_len = strlen(value);
    size_t new_len = (size_t)offset + add_len;
    if (new_len < old_len) new_len = old_len; /* value ends before the current end */
    if (new_len > MAX_STRING_LEN) {
      conn_reply(conn, "ERR resulting string too long\r\n");
      return;
    }

    char *buf = malloc(new_len + 1);
    memset(buf, ' ', new_len); /* pad any gap before offset with spaces */
    if (current) memcpy(buf, current, old_len);
    memcpy(buf + offset, value, add_len);
    buf[new_len] = '\0';
    store_update_string(key, buf);
    free(buf);
    conn_reply(conn, "LEN %zu\r\n", new_len);

  /* --- list commands --- */

  } else if (strcasecmp(cmd, "LPUSH") == 0 || strcasecmp(cmd, "RPUSH") == 0) {
    char *key = next_token(&rest);
    char *value = next_token(&rest);
    if (!key || !value) {
      conn_reply(conn, "ERR usage: %s key value [value ...]\r\n", cmd);
      return;
    }
    if (!check_type(conn, key, STORE_LIST)) return;
    List *list = store_get_or_create_list(key);
    bool push_left = strcasecmp(cmd, "LPUSH") == 0;
    for (; value; value = next_token(&rest)) {
      if (push_left) list_push_left(list, value);
      else list_push_right(list, value);
    }
    conn_reply(conn, "LEN %zu\r\n", list_len(list));

  } else if (strcasecmp(cmd, "LPOP") == 0 || strcasecmp(cmd, "RPOP") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: %s key\r\n", cmd); return; }
    if (!check_type(conn, key, STORE_LIST)) return;
    List *list = store_get_existing_list(key);
    char *value = NULL;
    if (list) value = strcasecmp(cmd, "LPOP") == 0 ? list_pop_left(list) : list_pop_right(list);
    if (value) {
      conn_reply(conn, "VALUE %s\r\n", value);
      free(value);
      store_delete_if_empty(key);
    } else {
      conn_reply(conn, "NOT_FOUND\r\n");
    }

  } else if (strcasecmp(cmd, "LLEN") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: LLEN key\r\n"); return; }
    if (!check_type(conn, key, STORE_LIST)) return;
    List *list = store_get_existing_list(key);
    conn_reply(conn, "LEN %zu\r\n", list ? list_len(list) : 0);

  } else if (strcasecmp(cmd, "LRANGE") == 0) {
    char *key = next_token(&rest);
    char *start_str = next_token(&rest);
    int start, stop;
    if (!key || !start_str || !parse_int(start_str, &start) || !parse_int(rest, &stop)) {
      conn_reply(conn, "ERR usage: LRANGE key start stop\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_LIST)) return;
    List *list = store_get_existing_list(key);
    if (list) {
      size_t count;
      char **items = list_range(list, start, stop, &count);
      for (size_t i = 0; i < count; i++) {
        conn_reply(conn, "%s\r\n", items[i]);
        free(items[i]);
      }
      free(items);
    }
    conn_reply(conn, "END\r\n");

  /* --- hash commands --- */

  } else if (strcasecmp(cmd, "HSET") == 0) {
    char *key = next_token(&rest);
    char *field = next_token(&rest);
    char *value = rest;
    if (!key || !field || *value == '\0') {
      conn_reply(conn, "ERR usage: HSET key field value\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_HASH)) return;
    Hash *hash = store_get_or_create_hash(key);
    hash_set(hash, field, value);
    conn_reply(conn, "OK\r\n");

  } else if (strcasecmp(cmd, "HGET") == 0) {
    char *key = next_token(&rest);
    char *field = rest;
    if (!key || *field == '\0') {
      conn_reply(conn, "ERR usage: HGET key field\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_HASH)) return;
    Hash *hash = store_get_existing_hash(key);
    const char *value = hash ? hash_get(hash, field) : NULL;
    if (value) conn_reply(conn, "VALUE %s\r\n", value);
    else conn_reply(conn, "NOT_FOUND\r\n");

  } else if (strcasecmp(cmd, "HDEL") == 0) {
    char *key = next_token(&rest);
    char *field = rest;
    if (!key || *field == '\0') {
      conn_reply(conn, "ERR usage: HDEL key field\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_HASH)) return;
    Hash *hash = store_get_existing_hash(key);
    if (hash && hash_del(hash, field)) {
      conn_reply(conn, "OK\r\n");
      store_delete_if_empty(key);
    } else {
      conn_reply(conn, "NOT_FOUND\r\n");
    }

  } else if (strcasecmp(cmd, "HLEN") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: HLEN key\r\n"); return; }
    if (!check_type(conn, key, STORE_HASH)) return;
    Hash *hash = store_get_existing_hash(key);
    conn_reply(conn, "LEN %zu\r\n", hash ? hash_len(hash) : 0);

  } else if (strcasecmp(cmd, "HGETALL") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: HGETALL key\r\n"); return; }
    if (!check_type(conn, key, STORE_HASH)) return;
    Hash *hash = store_get_existing_hash(key);
    if (hash) hash_foreach(hash, append_field_value_lines, conn);
    conn_reply(conn, "END\r\n");

  /* --- set commands --- */

  } else if (strcasecmp(cmd, "SADD") == 0) {
    char *key = next_token(&rest);
    char *member = next_token(&rest);
    if (!key || !member) { conn_reply(conn, "ERR usage: SADD key member [member ...]\r\n"); return; }
    if (!check_type(conn, key, STORE_SET)) return;
    Set *set = store_get_or_create_set(key);
    size_t added = 0;
    for (; member; member = next_token(&rest)) {
      if (set_add(set, member)) added++;
    }
    conn_reply(conn, "ADDED %zu\r\n", added);

  } else if (strcasecmp(cmd, "SREM") == 0) {
    char *key = next_token(&rest);
    char *member = rest;
    if (!key || *member == '\0') { conn_reply(conn, "ERR usage: SREM key member\r\n"); return; }
    if (!check_type(conn, key, STORE_SET)) return;
    Set *set = store_get_existing_set(key);
    if (set && set_rem(set, member)) {
      conn_reply(conn, "OK\r\n");
      store_delete_if_empty(key);
    } else {
      conn_reply(conn, "NOT_FOUND\r\n");
    }

  } else if (strcasecmp(cmd, "SISMEMBER") == 0) {
    char *key = next_token(&rest);
    char *member = rest;
    if (!key || *member == '\0') { conn_reply(conn, "ERR usage: SISMEMBER key member\r\n"); return; }
    if (!check_type(conn, key, STORE_SET)) return;
    Set *set = store_get_existing_set(key);
    conn_reply(conn, (set && set_contains(set, member)) ? "TRUE\r\n" : "FALSE\r\n");

  } else if (strcasecmp(cmd, "SCARD") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: SCARD key\r\n"); return; }
    if (!check_type(conn, key, STORE_SET)) return;
    Set *set = store_get_existing_set(key);
    conn_reply(conn, "LEN %zu\r\n", set ? set_size(set) : 0);

  } else if (strcasecmp(cmd, "SMEMBERS") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: SMEMBERS key\r\n"); return; }
    if (!check_type(conn, key, STORE_SET)) return;
    Set *set = store_get_existing_set(key);
    if (set) set_foreach(set, append_line, conn);
    conn_reply(conn, "END\r\n");

  /* --- sorted set commands --- */

  } else if (strcasecmp(cmd, "ZADD") == 0) {
    char *key = next_token(&rest);
    char *pairs[2 * MAX_ZADD_PAIRS];
    int npairs = 0;
    bool dangling = false;
    for (char *tok = next_token(&rest); tok; tok = next_token(&rest)) {
      if (npairs == MAX_ZADD_PAIRS) {
        conn_reply(conn, "ERR too many score/member pairs\r\n");
        return;
      }
      char *member = next_token(&rest);
      if (!member) { dangling = true; break; } /* score with no matching member */
      pairs[2 * npairs] = tok;
      pairs[2 * npairs + 1] = member;
      npairs++;
    }

    double scores[MAX_ZADD_PAIRS];
    bool valid = key && npairs > 0 && !dangling;
    for (int i = 0; valid && i < npairs; i++) {
      valid = parse_double(pairs[2 * i], &scores[i]);
    }
    if (!valid) {
      conn_reply(conn, "ERR usage: ZADD key score member [score member ...]\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_ZSET)) return;
    Zset *zset = store_get_or_create_zset(key);
    size_t added = 0;
    for (int i = 0; i < npairs; i++) {
      if (zset_add(zset, pairs[2 * i + 1], scores[i])) added++;
    }
    conn_reply(conn, "ADDED %zu\r\n", added);

  } else if (strcasecmp(cmd, "ZSCORE") == 0) {
    char *key = next_token(&rest);
    char *member = rest;
    if (!key || *member == '\0') { conn_reply(conn, "ERR usage: ZSCORE key member\r\n"); return; }
    if (!check_type(conn, key, STORE_ZSET)) return;
    Zset *zset = store_get_existing_zset(key);
    double score;
    if (zset && zset_score(zset, member, &score)) conn_reply(conn, "VALUE %g\r\n", score);
    else conn_reply(conn, "NOT_FOUND\r\n");

  } else if (strcasecmp(cmd, "ZREM") == 0) {
    char *key = next_token(&rest);
    char *member = rest;
    if (!key || *member == '\0') { conn_reply(conn, "ERR usage: ZREM key member\r\n"); return; }
    if (!check_type(conn, key, STORE_ZSET)) return;
    Zset *zset = store_get_existing_zset(key);
    if (zset && zset_rem(zset, member)) {
      conn_reply(conn, "OK\r\n");
      store_delete_if_empty(key);
    } else {
      conn_reply(conn, "NOT_FOUND\r\n");
    }

  } else if (strcasecmp(cmd, "ZCARD") == 0) {
    char *key = trim(rest);
    if (*key == '\0') { conn_reply(conn, "ERR usage: ZCARD key\r\n"); return; }
    if (!check_type(conn, key, STORE_ZSET)) return;
    Zset *zset = store_get_existing_zset(key);
    conn_reply(conn, "LEN %zu\r\n", zset ? zset_size(zset) : 0);

  } else if (strcasecmp(cmd, "ZRANGE") == 0) {
    char *key = next_token(&rest);
    char *start_str = next_token(&rest);
    int start, stop;
    if (!key || !start_str || !parse_int(start_str, &start) || !parse_int(rest, &stop)) {
      conn_reply(conn, "ERR usage: ZRANGE key start stop\r\n");
      return;
    }
    if (!check_type(conn, key, STORE_ZSET)) return;
    Zset *zset = store_get_existing_zset(key);
    if (zset) zset_range(zset, start, stop, append_member_score_line, conn);
    conn_reply(conn, "END\r\n");

  /* --- connection / server commands --- */

  } else if (strcasecmp(cmd, "SAVE") == 0) {
    persist_save();
    conn_reply(conn, "OK\r\n");

  } else if (strcasecmp(cmd, "QUIT") == 0) {
    conn_reply(conn, "BYE\r\n");
    conn_request_close(conn);

  } else if (strcasecmp(cmd, "SHUTDOWN") == 0) {
    conn_reply(conn, "SHUTTING_DOWN\r\n");
    server_stop();

  } else {
    conn_reply(conn, "ERR unknown command\r\n");
  }
}
