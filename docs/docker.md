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

## Publishing

[`.github/workflows/docker-publish.yml`](../.github/workflows/docker-publish.yml)
builds and pushes on every merge to `main`, and on demand from the
Actions tab.

It publishes to two registries from one build. GitHub's own `ghcr.io`
needs no secrets: the workflow logs in with the `GITHUB_TOKEN` that
Actions already provides, given `packages: write`. Note that the first
push there creates a *private* package - making it public is a one-time
change in the repository's package settings.

Docker Hub is the second, and it is opt-in, because it needs real
credentials that a fork will not have. Three repository settings turn it
on:

| Setting | Kind | What it is |
| --- | --- | --- |
| `DOCKERHUB_NAMESPACE` | variable | The user or organisation to publish under. The image becomes `<namespace>/klyro`. |
| `DOCKERHUB_USERNAME` | secret | The account the token belongs to. Not always the namespace - an organisation is pushed to by one of its members. |
| `DOCKERHUB_TOKEN` | secret | An access token from Docker Hub's security settings, with write scope. Not the account password. |

The variable is what gates it. Left unset, the Docker Hub login is
skipped and the run publishes to `ghcr.io` alone, so a fork without the
secrets still gets a working build rather than a credentials failure.

Unlike `ghcr.io`, Docker Hub does not create the repository's
description or its public/private setting from the push - the first push
creates a public repository under a personal namespace, and the rest of
the listing is filled in on Docker Hub itself.

The version comes from `[package] version` in `Cargo.toml`, so bumping
that line is what cuts a new tag. Every build gets four, on each
registry: the full version, major.minor, `latest`, and `sha-<commit>`.
Merges that don't bump the version overwrite the first three, which is
why the `sha-` tag exists - it is the only immutable handle on a
particular build.

Images are built for `linux/amd64` and `linux/arm64`. The arm64 half is
emulated with QEMU, which is slow for compilation in general but barely
noticeable here: the crate is small and its only dependency is `libc`.
Layers are cached in the GitHub Actions cache between runs.

### What it does not do

The workflow does not gate on `cargo test`. It would be a two-line
addition - a job that runs the suite and a `needs:` on the publish job -
and it is worth adding once `tests/admin.rs` is green again; at the time
of writing 22 of its tests fail against the in-flight `CONFIG`/`INFO`
work, so a gate would mean no image ever gets published. The Docker
build still fails the workflow if the crate does not compile.

### Why it smoke-tests before pushing

`cargo test` covers the server. Nothing in the crate covers the parts
that only exist in the image - the entrypoint's argument building, the
healthcheck, the volume, and whether `SIGTERM` really saves the dump.
Those are shell and Docker semantics, and they broke twice while this
image was being written.

So the workflow builds a single-architecture image first, keeps it
local, starts it, and checks that it goes healthy, answers `PING`,
returns what it stored, and still has the data after a stop and start.
Only then does it build the multi-architecture image and push. The
second build reuses the first one's layers, so the guard is close to
free.

The test asserts on a value it wrote itself rather than on the exact
shape of a reply, because the wire format is still moving. A test that
matched `VALUE ok` would have started failing the moment replies became
`$2\r\nok`.

## A note on line endings

The healthcheck sends `PING\r\n`, not `PING\n`. The server answers a
CRLF-terminated line and currently ignores one that ends in a bare LF,
which is exactly what a healthcheck built around `echo` sends. The
first version of the healthcheck did exactly that and passed anyway,
until a change to the server made it stop - so anything scripted against
this server should terminate its lines properly, whether or not a bare
LF happens to work today.
