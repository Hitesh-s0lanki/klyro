#!/bin/sh
# Builds Klyro's argument list from the environment, so the container is
# configurable with -e alone. Any arguments given to `docker run` are
# passed through untouched instead, as an escape hatch.
#
#   KLYRO_CONFIG  path to a mounted config file (optional)
#   KLYRO_PORT    listener port
#   KLYRO_DUMP    dump path (optional; defaults to klyro.dump in /data)
#
# Klyro lets command-line arguments win over a config file, so KLYRO_PORT
# overrides any `port` line in KLYRO_CONFIG.
set -e

if [ "$#" -gt 0 ]; then
    exec klyro "$@"
fi

set --
if [ -n "${KLYRO_CONFIG:-}" ]; then
    set -- --config "$KLYRO_CONFIG"
fi
set -- "$@" "${KLYRO_PORT:-7171}"
if [ -n "${KLYRO_DUMP:-}" ]; then
    set -- "$@" "$KLYRO_DUMP"
fi

exec klyro "$@"
