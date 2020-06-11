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
    echo "	manifest.sh [tag]"
    echo ""
    echo "Args:"
    echo "	repo: The docker repository to push the manifest to"
    echo "	tag: The git tag to be applied to the image manifest"
}

repo=$1
tag=$2

require "$repo" "repo"
require "$tag" "tag"

set -xe

docker manifest create $repo:$tag \
    -a $repo:arm64v8-$tag \
    -a $repo:arm32v7-$tag \
    -a $repo:amd64-$tag

docker manifest annotate $repo:$tag \
    $repo:arm64v8-$tag --os linux --arch arm64 --variant v8

docker manifest annotate $repo:$tag \
    $repo:arm32v7-$tag --os linux --arch arm --variant v7

docker manifest annotate $repo:$tag \
    $repo:amd64-$tag --os linux --arch amd64

docker manifest push $repo:$tag --purge
