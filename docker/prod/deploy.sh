#!/usr/bin/env bash

TAG=$1
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
    echo "      build.sh [tag] [migrations]"
    echo ""
    echo "Args:"
    echo "      tag: The git tag to create and publish"
    echo "      migrations: (optional) Whether to build the migrations container as well"
}

function build_image() {
    repo=$1
    tag=$2
    arch=$3

    docker build \
        --pull \
        --build-arg TAG="${tag}" \
        -f "Dockerfile.${arch}" \
        -t "${repo}:${tag}-${arch}" \
        -t "${repo}:latest-${arch}" \
        .

    docker push "${repo}:${tag}-${arch}"
    docker push "${repo}:latest-${arch}"
}

require "$TAG" "tag"

if ! docker run --rm -it arm64v8/ubuntu:19.10 /bin/bash -c 'echo "docker is configured correctly"'; then
    echo "docker is not configured to run on qemu-emulated architectures, fixing will require sudo"
    sudo docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
fi

set -xe

git checkout master
git commit -m "Version $TAG"
git tag $TAG

git push origin $TAG
git push

build_image "asonix/relay" "$TAG" "arm64v8"
build_image "asonix/relay" "$TAG" "arm32v7"
build_image "asonix/relay" "$TAG" "amd64"

./manifest.sh "asonix/relay" "$TAG"
./manifest.sh "asonix/relay" "latest"

if [ "${MIGRATIONS}" = "migrations" ]; then
    build_image "asonix/relay-migrations" "$TAG" arm64v8
    build_image "asonix/relay-migrations" "$TAG" arm32v7
    build_image "asonix/relay-migrations" "$TAG" amd64

    ./manifest.sh "asonix/relay-migrations" "$TAG"
    ./manifest.sh "asonix/relay-migrations" "latest"
fi
