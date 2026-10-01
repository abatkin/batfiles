#!/bin/sh
# Tag the checked-out commit for release, locally; pushing the tag releases it.
#
# Usage: dist/release.sh rc|stable
#
# `rc` tags the next release candidate of the Cargo.toml version,
# v<X.Y.Z>-rc.<n>, numbered one past every rc tag of that version here or on
# origin, from any commit. `stable` tags v<X.Y.Z>, from a commit on origin/main.
# Either refuses a working tree with uncommitted changes, and a version whose
# stable release is already tagged.
set -eu

die() {
    echo "release: $*" >&2
    exit 1
}

[ $# -eq 1 ] || die "usage: dist/release.sh rc|stable"
kind=$1
here=$(cd "$(dirname "$0")" && pwd)
root=$(git rev-parse --show-toplevel) || die "not in a Git repository"
cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)

[ -z "$(git status --porcelain)" ] || die "the working tree has uncommitted changes"

# Every tag, here and on origin.
remote=$(git ls-remote --tags --refs origin) || die "cannot list origin's tags"
tags=$( (
    git tag --list
    printf '%s\n' "$remote" | sed -n 's|.*refs/tags/||p'
) | sort -u)
if printf '%s\n' "$tags" | grep -Fqx "v$cargo"; then
    die "v$cargo is already tagged; set the next version in Cargo.toml"
fi

case $kind in
rc)
    pattern=$(printf '%s' "v$cargo-rc." | sed 's/\./\\./g')
    last=$(printf '%s\n' "$tags" | sed -n "s/^$pattern\([0-9][0-9]*\)$/\1/p" | sort -n | tail -n 1)
    tag=v$cargo-rc.$((${last:-0} + 1))
    ;;
stable)
    tag=v$cargo
    git fetch --quiet origin main || die "cannot fetch origin's main"
    ;;
*) die "usage: dist/release.sh rc|stable" ;;
esac

sh "$here/tag.sh" "$tag" >/dev/null
git tag -a "$tag" -m "batfiles ${tag#v}"
echo "release: tagged $(git rev-parse --short HEAD) as $tag; to release it:" >&2
echo "    git push origin $tag" >&2
