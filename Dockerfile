# syntax=docker/dockerfile:1.4
ARG ALPINE_VERSION=3.19
ARG RUST_VERSION=1.75

################################################################################

FROM rust:$RUST_VERSION-alpine$ALPINE_VERSION AS builder
ARG TARGETPLATFORM

RUN \
    --mount=type=cache,id=$TARGETPLATFORM-alpine,target=/var/cache/apk,sharing=locked \
    set -eux; \
    apk add -U build-base;

WORKDIR /opt/aode-relay

ADD Cargo.lock Cargo.toml /opt/aode-relay/
RUN cargo fetch;

ADD . /opt/aode-relay

RUN set -eux; \
    export RUSTFLAGS="-C target-cpu=generic"; \
    cargo build --frozen --release;

################################################################################

FROM alpine
ARG TARGETPLATFORM

RUN \
    --mount=type=cache,id=$TARGETPLATFORM-alpine,target=/var/cache/apk,sharing=locked \
    set -eux; \
    apk add -U ca-certificates curl tini;

COPY --link --from=builder /opt/aode-relay/target/release/relay /usr/local/bin/aode-relay

# Smoke test
RUN /usr/local/bin/aode-relay --help

# Some base env configuration
ENV ADDR 0.0.0.0
ENV PORT 8080
ENV DEBUG false
ENV VALIDATE_SIGNATURES true
ENV HTTPS false
ENV PRETTY_LOG false
ENV PUBLISH_BLOCKS true
ENV SLED_PATH "/var/lib/aode-relay/sled/db-0.34"
ENV RUST_LOG warn

VOLUME "/var/lib/aode-relay"

ENTRYPOINT ["/sbin/tini", "--"]

CMD ["/usr/local/bin/aode-relay"]

EXPOSE 8080

HEALTHCHECK CMD curl -sSf "localhost:$PORT/healthz" > /dev/null || exit 1
