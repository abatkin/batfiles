#!/bin/sh
# Install a published release on this machine with its own hosted installer,
# and check that what it installs reports the release.
#
# Usage: dist/smoke.sh <url> <version> <latest>
#
# Pipes <url>/download/v<version>/install.sh into sh with that version
# requested. With <latest> `yes`, also pipes <url>/latest/download/install.sh
# with no version requested, which must install the same release. Each install
# goes to a scratch path, never touching a batfiles already on the machine.
set -eu

die() {
    echo "dist:smoke: $*" >&2
    exit 1
}

[ $# -eq 3 ] || die "usage: dist/smoke.sh <url> <version> <yes|no>"
url=${1%/}
version=$2
latest=$3
case $latest in yes | no) ;; *) die "<latest> is '$latest', not yes or no" ;; esac

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Pipe the installer at $1 into sh, installing to $work/$2, with BATFILES_VERSION
# set to $3 (empty for the latest release), and check `batfiles version`.
try() {
    curl -fsSL "$1" >"$work/$2.sh" || die "cannot fetch $1"
    reported=$(BATFILES_BASE=$url BATFILES_VERSION=$3 BATFILES_BIN=$work/$2/batfiles \
        sh -s -- version <"$work/$2.sh") || die "installing with $1 failed"
    [ "$reported" = "batfiles $version" ] ||
        die "installing with $1 gave '$reported', not 'batfiles $version'"
    echo "dist:smoke: $1 installed batfiles $version on $(uname -s) $(uname -m)"
}

try "$url/download/v$version/install.sh" pinned "$version"
if [ "$latest" = yes ]; then
    try "$url/latest/download/install.sh" latest ""
fi
