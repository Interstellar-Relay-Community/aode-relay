ARG ALPINE_VER="3.18"

FROM rust:1-alpine${ALPINE_VER} AS builder

RUN apk add --no-cache openssl libc-dev openssl-dev protobuf protobuf-dev

WORKDIR /opt/aode-relay

ADD Cargo.lock Cargo.toml /opt/aode-relay/
RUN cargo fetch

ADD . /opt/aode-relay
RUN cargo build --frozen --release

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
