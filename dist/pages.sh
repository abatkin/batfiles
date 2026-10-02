#!/bin/sh
# Build the Pages site: site/ and the latest stable release's installers.
#
# Usage: dist/pages.sh <out> [<from>] [<site>]
#
# Reads <from>/latest/download/VERSION once, fetches install.sh and install.ps1
# from <from>/download/v<version>/, and writes them to <out> beside a copy of
# <site>, by default the repository's site/. <from> defaults to the official
# base. <out> must be absent or empty, and is written only once the site is
# complete. Prints `version=<version>` on standard output.
set -eu

die() {
    echo "dist:pages: $*" >&2
    exit 1
}

if [ $# -lt 1 ] || [ $# -gt 3 ]; then
    die "usage: dist/pages.sh <out> [<from>] [<site>]"
fi
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=dist/version.sh
. "$here/version.sh"
out=${1%/}
from=${2:-https://github.com/abatkin/batfiles/releases}
from=${from%/}
site=${3:-$here/../site}

[ -d "$site" ] || die "no site directory $site"
for name in install.sh install.ps1; do
    if [ -e "$site/$name" ] || [ -L "$site/$name" ]; then
        die "$site/$name would be replaced by the release's installer"
    fi
done
if [ -e "$out" ]; then
    [ -d "$out" ] || die "$out exists and is not a directory"
    [ -z "$(ls -A "$out")" ] || die "$out is not empty"
fi

mkdir -p "$(dirname "$out")"
stage=$(mktemp -d "$(dirname "$out")/.pages.XXXXXX")
trap 'rm -rf "$stage"' EXIT

# Download <url> to <file>, returning nonzero on any HTTP error.
get() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        die "neither curl nor wget is available"
    fi
}

fetch() {
    get "$1" "$2" || die "cannot fetch $1"
}

get "$from/latest/download/VERSION" "$stage/VERSION" ||
    die "cannot fetch $from/latest/download/VERSION; a site needs a stable release"
version=$(cat "$stage/VERSION")
rm "$stage/VERSION"
is_version "$version" ||
    die "$from/latest/download/VERSION holds '$version', which is not a version"
case $version in
*-*) die "$from/latest/download/VERSION names the pre-release $version" ;;
esac

cp -R "$site/." "$stage/"
fetch "$from/download/v$version/install.sh" "$stage/install.sh"
fetch "$from/download/v$version/install.ps1" "$stage/install.ps1"
chmod -R u+rwX,go+rX,go-w "$stage"

[ ! -d "$out" ] || rmdir "$out"
mv "$stage" "$out"
trap - EXIT
echo "version=$version"
echo "dist:pages: $out, the installers of $version from $from" >&2
