#!/bin/sh
# Check a release tag for the commit checked out in the current repository.
#
# Usage: dist/tag.sh <tag> [<main>]
#
# The Cargo.toml version, X.Y.Z, is the release being worked toward. The tag
# v<X.Y.Z> is a stable release and needs the checked-out commit to be on <main>,
# by default origin/main. v<X.Y.Z>-<pre-release> is a pre-release, from any
# commit. On success, prints `version=<version>` and
# `prerelease=<true|false>` on standard output.
set -eu

die() {
    echo "dist:tag: $*" >&2
    exit 1
}

if [ $# -lt 1 ] || [ $# -gt 2 ]; then
    die "usage: dist/tag.sh <tag> [<main>]"
fi
tag=$1
main=${2:-origin/main}
# shellcheck source=dist/version.sh
. "$(cd "$(dirname "$0")" && pwd)/version.sh"
root=$(git rev-parse --show-toplevel) || die "not in a Git repository"
cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)

if ! is_version "$cargo" || [ "${cargo#*-}" != "$cargo" ]; then
    die "the Cargo.toml version '$cargo' is not X.Y.Z; a pre-release takes its suffix from the tag"
fi

case $tag in
"v$cargo")
    version=$cargo
    prerelease=false
    git rev-parse --verify --quiet "$main^{commit}" >/dev/null ||
        die "there is no $main to check a stable release against"
    git merge-base --is-ancestor HEAD "$main" ||
        die "$tag is a stable release, and $(git rev-parse --short HEAD) is not on $main"
    ;;
"v$cargo"-*)
    version=${tag#v}
    prerelease=true
    is_version "$version" ||
        die "'$tag' has a malformed pre-release suffix; see docs/distribution.md#versions"
    ;;
*)
    die "tag '$tag' is neither v$cargo nor v$cargo-<pre-release>, as the Cargo.toml version allows"
    ;;
esac
printf 'version=%s\nprerelease=%s\n' "$version" "$prerelease"
