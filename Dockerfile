# syntax=docker/dockerfile:1.4
FROM alpine:3.19 AS alpine

RUN \
    --mount=type=cache,id=$TARGETPLATFORM-alpine,target=/var/cache/apk,sharing=locked \
    set -eux; \
    apk add -U libgcc;

################################################################################

FROM alpine AS alpine-dev

RUN \
    --mount=type=cache,id=$TARGETPLATFORM-alpine,target=/var/cache/apk,sharing=locked \
    set -eux; \
    apk add -U musl-dev;

################################################################################

FROM --platform=$BUILDPLATFORM rust:1 AS builder
ARG TARGETPLATFORM

RUN \
    --mount=type=cache,id=$BUILDPLATFORM-debian,target=/var/cache,sharing=locked \
    --mount=type=cache,id=$BUILDPLATFORM-debian,target=/var/lib/apt,sharing=locked \
    set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) \
            dpkgArch='i386'; \
        ;; \
        linux/amd64) \
            dpkgArch='amd64'; \
        ;; \
        linux/arm64) \
            dpkgArch='arm64'; \
        ;; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    dpkg --add-architecture $dpkgArch; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        musl-dev:$dpkgArch \
        musl-tools:$dpkgArch \
    ;

WORKDIR /opt/aode-relay

RUN set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) rustArch='i686';; \
        linux/amd64) rustArch='x86_64';; \
        linux/arm64) rustArch='aarch64';; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    rustup target add "${rustArch}-unknown-linux-musl";

ADD Cargo.lock Cargo.toml /opt/aode-relay/
RUN cargo fetch;

ADD . /opt/aode-relay
COPY --link --from=alpine-dev / /opt/alpine/

RUN set -eux; \
    case "${TARGETPLATFORM}" in \
        linux/i386) rustArch='i686';; \
        linux/amd64) rustArch='x86_64';; \
        linux/arm64) rustArch='aarch64';; \
        *) echo "unsupported architecture"; exit 1 ;; \
    esac; \
    ln -s "target/${rustArch}-unknown-linux-musl/release/relay" "aode-relay"; \
    # Workaround to use gnu-gcc instead of musl-gcc: https://github.com/rust-lang/rust/issues/95926
    export RUSTFLAGS="-C target-cpu=generic -C linker=${rustArch}-linux-musl-gcc -C relocation-model=static -C target-feature=-crt-static -C link-self-contained=no -L /opt/alpine/lib -L /opt/alpine/usr/lib"; \
    cargo build --frozen --release --target="${rustArch}-unknown-linux-musl";

################################################################################

FROM alpine

RUN \
    --mount=type=cache,id=$TARGETPLATFORM-alpine,target=/var/cache/apk,sharing=locked \
    set -eux; \
    apk add -U ca-certificates curl tini;

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
