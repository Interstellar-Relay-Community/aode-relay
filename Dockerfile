FROM rust:1-bullseye

ENV TINI_VERSION v0.19.0

ADD https://github.com/krallin/tini/releases/download/${TINI_VERSION}/tini /tini
RUN chmod +x /tini
ENTRYPOINT ["/tini", "--"]

RUN apt-get update && apt-get install -y libssl-dev protobuf-compiler

RUN mkdir -p /opt/aode-relay
WORKDIR /opt/aode-relay

ADD . /opt/aode-relay

RUN cargo build --release \
 && cp ./target/release/relay /usr/bin/aode-relay \
 && cd .. \
 && rm -rf aode-relay \
 && rm -rf ~/.cargo && rm -rf /usr/local/cargo \
 && mkdir aode-relay

# Some base env configuration
ENV ADDR 0.0.0.0
ENV PORT 8080
ENV DEBUG false
ENV VALIDATE_SIGNATURES true
ENV HTTPS true
ENV PRETTY_LOG false
ENV PUBLISH_BLOCKS true
ENV SLED_PATH /opt/aode-relay/sled/db-0.34
ENV RUST_LOG warn
# Since this container is intended to run behind reverse proxy
# we don't need HTTPS in here.
ENV HTTPS false

CMD ["/usr/bin/aode-relay"]
