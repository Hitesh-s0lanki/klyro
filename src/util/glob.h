#ifndef KLYRO_GLOB_H_
#define KLYRO_GLOB_H_

#include <stdbool.h>

/* Redis-style glob matching, used by KEYS/SCAN's optional pattern:
 * '*' matches any run of characters (including none), '?' matches
 * exactly one character, '[...]' matches one character from a set
 * (a leading '^' or '!' negates it; 'a-z' ranges are supported, and a
 * literal ']' is allowed as the class's first character), and '\'
 * escapes the next pattern character to match it literally. */
bool glob_match(const char *pattern, const char *str);

#endif // !KLYRO_GLOB_H_
