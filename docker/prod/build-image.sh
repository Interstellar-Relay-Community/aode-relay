#!/usr/bin/env bash

function require() {
    if [ "$1" = "" ]; then
        echo "input '$2' required"
        print_help
        exit 1
    fi
}

function print_help() {
    echo "deploy.sh"
    echo ""
    echo "Usage:"
    echo "	deploy.sh [repo] [tag] [arch]"
    echo ""
    echo "Args:"
    echo "	repo: The docker repository to publish the image"
    echo "	tag: The tag applied to the docker image"
    echo "	arch: The architecuture of the doker image"
}

REPO=$1
TAG=$2
ARCH=$3

require "$REPO" repo
require "$TAG" tag
require "$ARCH" arch

sudo docker build \
    --pull \
    --build-arg TAG=$TAG \
    --build-arg REPO_ARCH=$ARCH \
    -t $REPO:$ARCH-$TAG \
    -f Dockerfile \
    .
