#!/usr/bin/env bash

BUILD_DATE=$(date)
VERSION=$1

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
    echo "      build.sh [version]"
    echo ""
    echo "Args:"
    echo "      version: The version of the current container"
}

require "$VERSION" "version"

set -xe

# from `cargo install cross`
cross build \
    --target aarch64-unknown-linux-musl \
    --release

mkdir -p artifacts
cp ./target/aarch64-unknown-linux-musl/release/relay artifacts/relay

# from `sudo docker run --rm --privileged multiarch/qemu-user-static --reset -p yes`
docker build \
    --pull \
    --no-cache \
    --build-arg BUILD_DATE="${BUILD_DATE}" \
    --build-arg TAG="${TAG}" \
    -f "Dockerfile.arm64v8" \
    -t "asonix/relay:${VERSION}-arm64v8" \
    -t "asonix/relay:latest-arm64v8" \
    -t "asonix/relay:latest" \
    ./artifacts

docker push "asonix/relay:${VERSION}-arm64v8"
docker push "asonix/relay:latest-arm64v8"
docker push "asonix/relay:latest"
