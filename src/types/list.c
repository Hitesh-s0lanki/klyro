/* list.c - a doubly linked list of strings */
#include "list.h"

#include <stdlib.h>
#include <string.h>

typedef struct ListNode {
  char *value;
  struct ListNode *prev;
  struct ListNode *next;
} ListNode;

struct List {
  ListNode *head;
  ListNode *tail;
  size_t len;
};

List *list_new(void) {
  List *list = malloc(sizeof(List));
  list->head = list->tail = NULL;
  list->len = 0;
  return list;
}

void list_free(List *list) {
  ListNode *n = list->head;
  while (n) {
    ListNode *next = n->next;
    free(n->value);
    free(n);
    n = next;
  }
  free(list);
}

void list_push_left(List *list, const char *value) {
  ListNode *n = malloc(sizeof(ListNode));
  n->value = strdup(value);
  n->prev = NULL;
  n->next = list->head;
  if (list->head) list->head->prev = n;
  list->head = n;
  if (!list->tail) list->tail = n;
  list->len++;
}

void list_push_right(List *list, const char *value) {
  ListNode *n = malloc(sizeof(ListNode));
  n->value = strdup(value);
  n->next = NULL;
  n->prev = list->tail;
  if (list->tail) list->tail->next = n;
  list->tail = n;
  if (!list->head) list->head = n;
  list->len++;
}

char *list_pop_left(List *list) {
  ListNode *n = list->head;
  if (!n) return NULL;

  list->head = n->next;
  if (list->head) list->head->prev = NULL;
  else list->tail = NULL;
  list->len--;

  char *value = n->value;
  free(n);
  return value;
}

char *list_pop_right(List *list) {
  ListNode *n = list->tail;
  if (!n) return NULL;

  list->tail = n->prev;
  if (list->tail) list->tail->next = NULL;
  else list->head = NULL;
  list->len--;

  char *value = n->value;
  free(n);
  return value;
}

size_t list_len(List *list) {
  return list->len;
}

char **list_range(List *list, long start, long stop, size_t *count) {
  long len = (long)list->len;

  if (start < 0) start += len;
  if (stop < 0) stop += len;
  if (start < 0) start = 0;
  if (stop >= len) stop = len - 1;

  if (len == 0 || start > stop || start >= len) {
    *count = 0;
    return NULL;
  }

  size_t n = (size_t)(stop - start + 1);
  char **out = malloc(n * sizeof(char *));

  ListNode *node = list->head;
  for (long i = 0; i < start; i++) node = node->next;

  size_t written = 0;
  for (long i = start; i <= stop; i++, node = node->next) {
    out[written++] = strdup(node->value);
  }
  *count = written;
  return out;
}

void list_foreach(List *list, ListEachFn fn, void *userdata) {
  for (ListNode *node = list->head; node; node = node->next) {
    fn(node->value, userdata);
  }
}
