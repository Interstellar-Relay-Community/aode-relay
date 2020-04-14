#!/usr/bin/env bash

BUILD_DATE=$(date)
VERSION=$1
MIGRATIONS=$2

function require() {
    if [ "$1" = "" ]; then
        echo "input '$2' required"
        print_help
        exit 1
    fi
}

function print_help() {
    echo "build.sh"
    echo ""
    echo "Usage:"
    echo "      build.sh [version] [migrations]"
    echo ""
    echo "Args:"
    echo "      version: The version of the current container"
    echo "      migrations: (optional) Whether to build the migrations container as well"
}

require "$VERSION" "version"

if docker run --rm -it arm64v8/ubuntu:19.10 /bin/bash -c 'echo "docker is configured correctly"'; then
    echo ""
else
    echo "docker is not configured to run on qemu-emulated architectures, fixing will require sudo"
    sudo docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
fi

set -xe

# from `cargo install cross`
cross build \
    --target aarch64-unknown-linux-musl \
    --release

mkdir -p artifacts
rm -rf artifacts/relay
cp ./target/aarch64-unknown-linux-musl/release/relay artifacts/relay

# from `sudo docker run --rm --privileged multiarch/qemu-user-static --reset -p yes`
docker build \
    --pull \
    --no-cache \
    --build-arg BUILD_DATE="${BUILD_DATE}" \
    --build-arg TAG="${TAG}" \
    -f Dockerfile.arm64v8 \
    -t "asonix/relay:${VERSION}-arm64v8" \
    -t "asonix/relay:latest-arm64v8" \
    -t "asonix/relay:latest" \
    ./artifacts

docker push "asonix/relay:${VERSION}-arm64v8"
docker push "asonix/relay:latest-arm64v8"
docker push "asonix/relay:latest"

if [ "${MIGRATIONS}" = "migrations" ]; then
    rm -rf artifacts/migrations
    cp -r ./migrations artifacts/migrations

    docker build \
        --pull \
        --no-cache \
        --build-arg BUILD_DATE="${BUILD_DATE}" \
        --build-arg TAG="${TAG}" \
        -f Dockerfile.migrations.arm64v8 \
        -t "asonix/relay-migrations:${VERSION}-arm64v8" \
        -t "asonix/relay-migrations:latest-arm64v8" \
        -t "asonix/relay-migrations:latest" \
        ./artifacts

    docker push "asonix/relay-migrations:${VERSION}-arm64v8"
    docker push "asonix/relay-migrations:latest-arm64v8"
    docker push "asonix/relay-migrations:latest"
fi
