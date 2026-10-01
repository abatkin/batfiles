#!/bin/sh
# Publish an assembled release as a GitHub release, with the `gh` CLI.
#
# Usage: dist/publish.sh <version> <dir>
#
# Creates a draft release for the existing tag v<version>, uploads every file in
# <dir>, then publishes it, so `latest/download/` never serves a partial asset
# set. A pre-release version is published as a pre-release and never becomes
# the latest release. Refuses a tag that already has a release. The repository
# is the one `gh` infers, or $GH_REPO.
set -eu

die() {
    echo "dist:publish: $*" >&2
    exit 1
}

[ $# -eq 2 ] || die "usage: dist/publish.sh <version> <dir>"
version=$1
dir=${2%/}
tag=v$version
if [ ! -f "$dir/VERSION" ] || [ "$(cat "$dir/VERSION")" != "$version" ]; then
    die "$dir is not an assembled release of $version"
fi

case $version in
*-*) kind="--prerelease" latest="--latest=false" ;;
*) kind='' latest="--latest" ;;
esac

# shellcheck disable=SC2086 # $kind is empty or one flag.
gh release create "$tag" --draft --verify-tag $kind \
    --title "batfiles $version" --generate-notes "$dir"/*
gh release edit "$tag" --draft=false "$latest"
echo "dist:publish: $tag published"
