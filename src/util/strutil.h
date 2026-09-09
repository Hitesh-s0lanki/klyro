#ifndef KLYRO_STRUTIL_H_
#define KLYRO_STRUTIL_H_

#include <stdbool.h>

/* Small line-parsing helpers shared by the command dispatcher and the
 * persistence loader/saver, which both parse simple whitespace-delimited
 * text lines. */

/* Trims leading spaces/tabs and trailing spaces/tabs/\r from s in place,
 * returning a pointer to the trimmed start. */
char *trim(char *s);

/* Extracts the next whitespace-delimited token from *rest, null-terminating
 * it in place and advancing *rest past it plus any following spaces.
 * Returns NULL (leaving *rest untouched) if *rest is already empty. */
char *next_token(char **rest);

bool parse_int(const char *s, int *out);
bool parse_long(const char *s, long *out);
bool parse_double(const char *s, double *out);

#endif // !KLYRO_STRUTIL_H_
