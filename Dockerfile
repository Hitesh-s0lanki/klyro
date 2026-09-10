# Build a statically-linked musl binary, then ship it on a minimal
# Alpine runtime. Alpine (rather than scratch) keeps busybox around, so
# the healthcheck below has an `nc` to speak the protocol with and a
# `netstat` to find the port.

FROM rust:1-alpine AS build

# The libc crate links against musl, which needs the C toolchain.
RUN apk add --no-cache musl-dev

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release && strip target/release/klyro


FROM alpine:3

LABEL org.opencontainers.image.title="Klyro" \
      org.opencontainers.image.description="The high-performance in-memory data server" \
      org.opencontainers.image.source="https://github.com/Hitesh-s0lanki/klyro" \
      org.opencontainers.image.licenses="MIT"

COPY --from=build /src/target/release/klyro /usr/local/bin/klyro
COPY docker-entrypoint.sh docker-healthcheck.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/bin/docker-healthcheck.sh

# Klyro never needs root, and the dump file is the only thing it writes.
RUN addgroup -S -g 10001 klyro \
    && adduser -S -u 10001 -G klyro -H -s /sbin/nologin klyro \
    && mkdir -p /data \
    && chown klyro:klyro /data

# The port the entrypoint passes unless it is overridden. The dump path
# is deliberately left unset: WORKDIR is /data and Klyro's default
# dbfilename is relative, so the dump lands in the volume on its own -
# and a mounted config file's `dbfilename` still gets to win.
ENV KLYRO_PORT=7171

USER klyro
WORKDIR /data
VOLUME ["/data"]
EXPOSE 7171

# `docker stop` sends SIGTERM, which Klyro handles by saving the dump
# before exiting - so give it room to finish writing.
STOPSIGNAL SIGTERM

HEALTHCHECK --interval=10s --timeout=5s --start-period=2s --retries=3 \
    CMD ["docker-healthcheck.sh"]

ENTRYPOINT ["docker-entrypoint.sh"]
