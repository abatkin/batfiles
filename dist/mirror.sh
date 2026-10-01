#!/bin/sh
# Copy a published release for serving from another release base.
#
# Usage: dist/mirror.sh <version> <base> <out> [<from>]
#
# Downloads release <version> (or `latest`) from the release tree at <from>, by
# default the official one, verifies every binary against its SHA256SUMS, and
# writes the release to <out> with both installers restamped with <base>. <out>
# must be absent or empty. Upload it to <base>/download/v<version>/, and to
# <base>/latest/download/ for the latest release.
set -eu

die() {
    echo "dist:mirror: $*" >&2
    exit 1
}

if [ $# -lt 3 ] || [ $# -gt 4 ]; then
    die "usage: dist/mirror.sh <version> <base> <out> [<from>]"
fi
here=$(cd "$(dirname "$0")" && pwd)
version=${1#v}
base=$2
out=$3
from=${4:-https://github.com/abatkin/batfiles/releases}
from=${from%/}

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

if [ "$version" = latest ]; then
    fetch "$from/latest/download/VERSION" "$work/VERSION"
    version=$(cat "$work/VERSION")
fi
release=$from/download/v$version

mkdir "$work/release" "$work/bin"
fetch "$release/VERSION" "$work/release/VERSION"
[ "$(cat "$work/release/VERSION")" = "$version" ] ||
    die "$release/VERSION holds '$(cat "$work/release/VERSION")', not '$version'"
fetch "$release/SHA256SUMS" "$work/release/SHA256SUMS"
targets=
while read -r _ name; do
    case $name in
    batfiles-*) ;;
    *) die "$release/SHA256SUMS lists '$name', which is not a binary" ;;
    esac
    fetch "$release/$name" "$work/bin/$name"
    target=${name#batfiles-}
    targets="$targets ${target%.exe}"
done <"$work/release/SHA256SUMS"
[ -n "$targets" ] || die "$release/SHA256SUMS lists no binaries"
if command -v sha256sum >/dev/null 2>&1; then
    (cd "$work/bin" && sha256sum -c --quiet ../release/SHA256SUMS) >&2 ||
        die "a binary in $release does not match SHA256SUMS"
else
    (cd "$work/bin" && shasum -a 256 -c --quiet ../release/SHA256SUMS) >&2 ||
        die "a binary in $release does not match SHA256SUMS"
fi
fetch "$release/install.sh" "$work/release/install.sh"
fetch "$release/install.ps1" "$work/release/install.ps1"

sh "$here/assemble.sh" "$version" "$base" "$out" "$work/bin" "$targets" "$work/release" >/dev/null
cmp -s "$work/release/SHA256SUMS" "$out/SHA256SUMS" ||
    die "the mirrored SHA256SUMS differs from $release/SHA256SUMS"
echo "dist:mirror: $out, version $version, from $release, for $base"
