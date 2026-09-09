/* strutil.c - small line-parsing helpers */
#include "strutil.h"

#include <stdlib.h>
#include <string.h>

char *trim(char *s) {
  while (*s == ' ' || *s == '\t') s++;
  char *end = s + strlen(s);
  while (end > s && (end[-1] == ' ' || end[-1] == '\t' || end[-1] == '\r' || end[-1] == '\n')) end--;
  *end = '\0';
  return s;
}

char *next_token(char **rest) {
  char *start = *rest;
  if (*start == '\0') return NULL;

  char *sp = start;
  while (*sp && *sp != ' ') sp++;
  if (*sp) {
    *sp = '\0';
    sp++;
    while (*sp == ' ') sp++;
  }
  *rest = sp;
  return start;
}

bool parse_int(const char *s, int *out) {
  if (*s == '\0') return false;
  char *end;
  long v = strtol(s, &end, 10);
  if (*end != '\0') return false;
  *out = (int)v;
  return true;
}

bool parse_long(const char *s, long *out) {
  if (*s == '\0') return false;
  char *end;
  long v = strtol(s, &end, 10);
  if (*end != '\0') return false;
  *out = v;
  return true;
}

bool parse_double(const char *s, double *out) {
  if (*s == '\0') return false;
  char *end;
  double v = strtod(s, &end);
  if (*end != '\0') return false;
  *out = v;
  return true;
}
