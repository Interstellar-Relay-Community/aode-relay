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
    echo "	manifest.sh [repo] [tag]"
    echo ""
    echo "Args:"
    echo "	repo: The docker repository to update"
    echo "	tag: The git tag to be applied to the image manifest"
}

REPO=$1
TAG=$2

require "$REPO" "repo"
require "$TAG" "tag"

set -xe

sudo docker manifest create asonix/$REPO:$TAG \
    -a asonix/$REPO:arm64v8-$TAG \
    -a asonix/$REPO:arm32v7-$TAG \
    -a asonix/$REPO:amd64-$TAG

sudo docker manifest annotate asonix/$REPO:$TAG \
    asonix/$REPO:arm64v8-$TAG --os linux --arch arm64 --variant v8

sudo docker manifest annotate asonix/$REPO:$TAG \
    asonix/$REPO:arm32v7-$TAG --os linux --arch arm --variant v7

sudo docker manifest annotate asonix/$REPO:$TAG \
    asonix/$REPO:amd64-$TAG --os linux --arch amd64

sudo docker manifest push asonix/$REPO:$TAG --purge
