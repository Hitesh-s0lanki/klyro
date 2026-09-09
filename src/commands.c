/* commands.c - command line parsing and dispatch */
#include "commands.h"
#include "persist.h"
#include "store.h"
#include "types/hash.h"
#include "types/list.h"
#include "types/set.h"
#include "types/zset.h"
#include "util/strutil.h"

#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <time.h>

#define SWEEP_INTERVAL_SEC 1
#define MAX_ZADD_PAIRS 128

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
    store_foreach_key(append_line, conn);
    conn_reply(conn, "END\r\n");

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
