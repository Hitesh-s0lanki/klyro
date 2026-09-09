/* glob.c - Redis-style glob pattern matching */
#include "glob.h"

/* *pp points just after the opening '['; advances *pp past the closing
 * ']'. Returns whether `c` is a member of the (possibly negated) class. */
static bool match_class(const char **pp, char c) {
  const char *p = *pp;
  bool negate = false;
  if (*p == '^' || *p == '!') {
    negate = true;
    p++;
  }

  bool found = false;
  bool first = true; /* a ']' right after '[' (or '[^') is a literal member */
  while (*p && (*p != ']' || first)) {
    first = false;
    if (*p == '\\' && p[1]) {
      if (p[1] == c) found = true;
      p += 2;
      continue;
    }
    if (p[1] == '-' && p[2] && p[2] != ']') {
      if ((unsigned char)c >= (unsigned char)p[0] && (unsigned char)c <= (unsigned char)p[2]) {
        found = true;
      }
      p += 3;
      continue;
    }
    if (*p == c) found = true;
    p++;
  }
  if (*p == ']') p++;

  *pp = p;
  return negate ? !found : found;
}

bool glob_match(const char *pattern, const char *str) {
  while (*pattern) {
    switch (*pattern) {
      case '*': {
        pattern++;
        if (*pattern == '\0') return true; /* trailing '*' matches the rest */
        while (*str) {
          if (glob_match(pattern, str)) return true;
          str++;
        }
        return glob_match(pattern, str); /* '*' may also match zero chars */
      }

      case '?':
        if (*str == '\0') return false;
        pattern++;
        str++;
        break;

      case '[':
        if (*str == '\0') return false;
        pattern++;
        if (!match_class(&pattern, *str)) return false;
        str++;
        break;

      case '\\':
        if (pattern[1]) pattern++;
        /* fall through: match the (possibly escaped) character literally */
      default:
        if (*str != *pattern) return false;
        pattern++;
        str++;
        break;
    }
  }
  return *str == '\0';
}
