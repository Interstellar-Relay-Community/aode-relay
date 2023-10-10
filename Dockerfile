ARG ALPINE_VER="3.18"

FROM --platform=$BUILDPLATFORM rust:1-alpine${ALPINE_VER} AS builder
ARG BUILDPLATFORM
ARG TARGETPLATFORM

RUN set -eux; \
    apk add --no-cache musl-dev;

WORKDIR /opt/aode-relay

ADD Cargo.lock Cargo.toml /opt/aode-relay/
RUN cargo fetch

ADD . /opt/aode-relay
RUN set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) ARCH='i686';; \
        linux/amd64) ARCH='x86_64';; \
        linux/arm32v6) ARCH='arm';; \
        linux/arm32v7) ARCH='armv7';; \
        linux/arm64) ARCH='aarch64';; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    rustup target add "${ARCH}-unknown-linux-musl"; \
    cargo build --frozen --release --target="${ARCH}-unknown-linux-musl";

################################################################################

FROM alpine:${ALPINE_VER}

RUN apk add --no-cache openssl ca-certificates curl tini

COPY --from=builder /opt/aode-relay/target/release/relay /usr/bin/aode-relay

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

CMD ["/usr/bin/aode-relay"]

EXPOSE 8080

HEALTHCHECK CMD curl -sSf "localhost:$PORT/healthz" > /dev/null || exit 1
