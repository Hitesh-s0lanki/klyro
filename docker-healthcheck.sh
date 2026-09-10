#!/bin/sh
# Probes Klyro over its own line protocol: PING should answer PONG.
#
# The port is read off Klyro's listening socket rather than the
# environment, so the check stays correct however the server was started
# - env vars, a config file, or arguments passed straight to
# `docker run`. Matching on the program name matters: on a user-defined
# network, Docker's embedded DNS resolver is listening in here too.
set -e

port=$(netstat -lntp 2>/dev/null |
    awk '/LISTEN/ && $NF ~ /\/klyro$/ { n = split($4, a, ":"); print a[n]; exit }')

# The request has to end in CRLF - the server does not answer a bare
# LF - and QUIT makes it close the socket, which is what busybox's nc
# waits for. The sleep only matters if a reply is ever slow enough that
# nc would otherwise reach stdin's EOF first and exit empty-handed.
{ printf 'PING\r\nQUIT\r\n'; sleep 1; } |
    nc -w 2 127.0.0.1 "${port:-${KLYRO_PORT:-7171}}" |
    grep -q PONG
