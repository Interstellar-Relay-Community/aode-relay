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
    echo "	deploy.sh [tag] [branch] [push]"
    echo ""
    echo "Args:"
    echo "	tag: The git tag to be applied to the repository and docker build"
    echo "	branch: The git branch to use for tagging and publishing"
    echo "	push: Whether or not to push the image"
    echo ""
    echo "Examples:"
    echo "	./deploy.sh v0.3.0-alpha.13 main true"
    echo "	./deploy.sh v0.3.0-alpha.13-shell-out asonix/shell-out false"
}

function build_image() {
    tag=$1
    arch=$2
    push=$3

    ./build-image.sh asonix/relay $tag $arch

    sudo docker tag asonix/relay:$arch-$tag asonix/relay:$arch-latest

    if [ "$push" == "true" ]; then
        sudo docker push asonix/relay:$arch-$tag
        sudo docker push asonix/relay:$arch-latest
    fi
}

# Creating the new tag
new_tag="$1"
branch="$2"
push=$3

require "$new_tag" "tag"
require "$branch" "branch"
require "$push" "push"

if ! sudo docker run --rm -it arm64v8/alpine:3.11 /bin/sh -c 'echo "docker is configured correctly"'
then
    echo "docker is not configured to run on qemu-emulated architectures, fixing will require sudo"
    sudo docker run --rm --privileged multiarch/qemu-user-static --reset -p yes
fi

set -xe

git checkout $branch

# Changing the docker-compose prod
sed -i "s/asonix\/relay:.*/asonix\/relay:$new_tag/" docker-compose.yml
git add ../prod/docker-compose.yml
# The commit
git commit -m"Version $new_tag"
git tag $new_tag

# Push
git push origin $new_tag
git push

# Build for arm64v8, arm32v7 and amd64
build_image $new_tag arm64v8 $push
build_image $new_tag arm32v7 $push
build_image $new_tag amd64 $push

# Build for other archs
# TODO

if [ "$push" == "true" ]; then
    ./manifest.sh relay $new_tag
    ./manifest.sh relay latest

    # pushd ../../
    # cargo publish
    # popd
fi
