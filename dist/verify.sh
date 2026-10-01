#!/bin/sh
# Check a published release through the release tree that serves it.
#
# Usage: dist/verify.sh <url> <version> <latest> [<stamped-base>]
#
# Fetches <url>/download/v<version>/: VERSION must hold <version>, every binary
# SHA256SUMS lists must match it, and both installers must be stamped with
# <stamped-base>, which defaults to <url>. With <latest> `yes`,
# <url>/latest/download/ must serve the same VERSION and SHA256SUMS.
set -eu

die() {
    echo "dist:verify: $*" >&2
    exit 1
}

[ $# -ge 3 ] && [ $# -le 4 ] ||
    die "usage: dist/verify.sh <url> <version> <yes|no> [<stamped-base>]"
url=${1%/}
version=$2
latest=$3
stamped=${4:-$url}
stamped=${stamped%/}
case $latest in yes | no) ;; *) die "<latest> is '$latest', not yes or no" ;; esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Download <url> to <file>, failing on any HTTP error.
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1" || die "cannot fetch $1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1" || die "cannot fetch $1"
    else
        die "neither curl nor wget is available"
    fi
}

release=$url/download/v$version
mkdir "$work/release"
fetch "$release/VERSION" "$work/release/VERSION"
[ "$(cat "$work/release/VERSION")" = "$version" ] ||
    die "$release/VERSION holds '$(cat "$work/release/VERSION")', not '$version'"

fetch "$release/SHA256SUMS" "$work/release/SHA256SUMS"
names=$(awk '{ print $2 }' "$work/release/SHA256SUMS")
[ -n "$names" ] || die "$release/SHA256SUMS lists no binaries"
for name in $names; do
    case $name in
    batfiles-*) fetch "$release/$name" "$work/release/$name" ;;
    *) die "$release/SHA256SUMS lists '$name', which is not a binary" ;;
    esac
done
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$work/release" && sha256sum -c --quiet SHA256SUMS) >&2 ||
        die "a binary in $release does not match SHA256SUMS"
else
    (cd "$work/release" && shasum -a 256 -c --quiet SHA256SUMS) >&2 ||
        die "a binary in $release does not match SHA256SUMS"
fi

fetch "$release/install.sh" "$work/release/install.sh"
grep -Fqx "batfiles_stamped_base='$stamped'" "$work/release/install.sh" ||
    die "$release/install.sh is not stamped with $stamped"
fetch "$release/install.ps1" "$work/release/install.ps1"
grep -Fqx "\$BatfilesStampedBase = '$stamped'" "$work/release/install.ps1" ||
    die "$release/install.ps1 is not stamped with $stamped"

if [ "$latest" = yes ]; then
    mkdir "$work/latest"
    fetch "$url/latest/download/VERSION" "$work/latest/VERSION"
    fetch "$url/latest/download/SHA256SUMS" "$work/latest/SHA256SUMS"
    cmp -s "$work/release/VERSION" "$work/latest/VERSION" ||
        die "$url/latest/download/ serves version $(cat "$work/latest/VERSION"), not $version"
    cmp -s "$work/release/SHA256SUMS" "$work/latest/SHA256SUMS" ||
        die "$url/latest/download/SHA256SUMS differs from the one for v$version"
fi

echo "dist:verify: $release: $(echo "$names" | wc -w | tr -d ' ') binaries verified"
