# The Docker image

Usage lives in the README's "Run with Docker" section. This file covers
the decisions behind the image, which are mostly about matching Klyro's
own behaviour rather than about Docker.

## Two stages, static binary

The build stage is `rust:1-alpine`, whose default target is
`*-linux-musl`. Rust links musl statically by default, so the binary
that comes out has no runtime library dependencies at all. Nothing from
the build stage is carried forward except that one file, which is why
the image is ~15 MB rather than the ~1.5 GB of the Rust toolchain.

No `--target` is passed anywhere, so the same Dockerfile builds a native
image on both x86_64 and arm64 hosts.

Only `Cargo.toml`, `Cargo.lock` and `src/` are copied in. `tests/`,
`clients/` and `docs/` are excluded by `.dockerignore`, so editing them
doesn't invalidate the build cache.

## Alpine, not scratch

A static binary could run on `scratch`, which would save another ~8 MB.
Alpine is worth those megabytes for two reasons:

- The healthcheck needs `nc` to speak the protocol and `netstat` to find
  the port. Both come from busybox.
- `docker exec ... sh` works, which matters the first time something is
  wrong in production.

## Shutdown has to be graceful

Klyro keeps everything in memory and writes the dump on `SIGTERM` (and
on `SIGINT`, `SHUTDOWN`, `SAVE`, and every 60s if anything changed).
`docker stop` sends `SIGTERM` and then `SIGKILL` ten seconds later, and
a `SIGKILL` here means losing every write since the last save.

So `STOPSIGNAL SIGTERM` is explicit in the image, and Compose sets
`stop_grace_period: 30s`. Anyone running a keyspace big enough that a
save takes longer than that should raise it further.

## The healthcheck finds its own port

The obvious healthcheck - `nc` to `$KLYRO_PORT` - is wrong as soon as
the port comes from somewhere other than that variable, which it can:
a `port` line in a mounted config file, or arguments passed straight to
`docker run`. The container then runs fine while reporting unhealthy.

`docker-healthcheck.sh` reads the port off the listening socket instead,
so it is right in all three cases. It matches on the program name rather
than taking the first listening socket, because on a user-defined
network - which is to say, under Compose - Docker's embedded DNS
resolver is also listening inside the container, on 127.0.0.11 at a
random port. Taking the first socket finds the resolver about half the
time; that was a real failure, caught by testing against Compose rather
than plain `docker run`.

## Configuration precedence

`docker-entrypoint.sh` turns `KLYRO_CONFIG`, `KLYRO_PORT` and
`KLYRO_DUMP` into Klyro's own arguments, so the container is
configurable with `-e` alone. Arguments given to `docker run` after the
image name bypass it entirely and reach the binary untouched.

Klyro lets command-line arguments override a config file, and the
entrypoint always passes a port. So `KLYRO_PORT` wins over a `port` line
in `KLYRO_CONFIG` - worth knowing if a mounted config file's port
appears to be ignored.

`KLYRO_DUMP` deliberately has no default in the image. `WORKDIR` is
`/data` and Klyro's default `dbfilename` is relative, so the dump lands
in the volume without the entrypoint naming a path - which leaves
`dbfilename` in a mounted config file free to take effect.

## Non-root

The server needs no privileges: it binds 7171, not a privileged port,
and the dump file is the only thing it writes. It runs as uid 10001,
which owns `/data`.

A named volume inherits that ownership, so the common path needs no
setup. A bind mount does not - `-v $PWD/data:/data` needs the host
directory to be writable by uid 10001, or the first save fails.
