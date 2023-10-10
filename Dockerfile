# syntax=docker/dockerfile:1.4
FROM --platform=$BUILDPLATFORM rust:1 AS builder
ARG BUILDPLATFORM
ARG TARGETPLATFORM

RUN \
    --mount=type=cache,target=/var/cache,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    --mount=type=tmpfs,target=/var/log \
    set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) \
            rustArch='i686'; \
            dpkgArch='i386'; \
        ;; \
        linux/amd64) \
            rustArch='x86_64'; \
            dpkgArch='amd64'; \
        ;; \
        linux/arm64) \
            rustArch='aarch64'; \
            dpkgArch='arm64'; \
        ;; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    dpkg --add-architecture $dpkgArch; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        musl:$dpkgArch \
        musl-dev:$dpkgArch \
        musl-tools:$dpkgArch \
    ; \
    rustup target add "${rustArch}-unknown-linux-musl";

WORKDIR /opt/aode-relay

ADD Cargo.lock Cargo.toml /opt/aode-relay/
RUN cargo fetch;

ADD . /opt/aode-relay
RUN set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) rustArch='i686';; \
        linux/amd64) rustArch='x86_64';; \
        linux/arm64) rustArch='aarch64';; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    ln -s "target/${rustArch}-unknown-linux-musl/release/relay" "aode-relay"; \
    RUSTFLAGS="-C linker=${rustArch}-linux-musl-gcc" cargo build --frozen --release --target="${rustArch}-unknown-linux-musl";

################################################################################

FROM alpine:3.18

RUN apk add --no-cache openssl ca-certificates curl tini

COPY --link --from=builder /opt/aode-relay/aode-relay /usr/local/bin/aode-relay

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
