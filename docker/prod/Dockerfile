ARG REPO_ARCH=amd64

# cross-build environment
FROM asonix/rust-builder:$REPO_ARCH-latest AS builder

ARG TAG=main
ARG BINARY=relay
ARG PROJECT=relay
ARG GIT_REPOSITORY=https://git.asonix.dog/asonix/$PROJECT

ENV \
 BINARY=${BINARY}

ADD \
 --chown=build:build \
 $GIT_REPOSITORY/archive/$TAG.tar.gz \
 /opt/build/repo.tar.gz

RUN \
 tar zxf repo.tar.gz

WORKDIR /opt/build/$PROJECT

RUN \
 build

# production environment
FROM asonix/rust-runner:$REPO_ARCH-latest

ARG BINARY=relay

ENV \
 BINARY=${BINARY}

COPY \
 --from=builder \
 /opt/build/binary \
 /usr/bin/${BINARY}

ENTRYPOINT ["/sbin/tini", "--"]
CMD /usr/bin/${BINARY}
