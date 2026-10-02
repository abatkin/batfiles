#!/bin/sh
# Install a published release on this machine with its own hosted installer,
# and check that what it installs reports the release.
#
# Usage: dist/smoke.sh <url> <version> <latest>
#
# Pipes <url>/download/v<version>/install.sh into sh with that version
# requested. With <latest> `yes`, also pipes <url>/latest/download/install.sh
# with no version requested, which must install the same release, and whose
# `batfiles update --check` must report it available. Each install goes to a
# scratch path, never touching a batfiles already on the machine. On Windows,
# under Git Bash, the same checks run install.ps1 with pwsh instead, by its two
# one-liners: the scriptblock form running `version`, and `irm | iex`.
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

# Run the Windows installer at $1 with pwsh, installing to $work/$2, with
# BATFILES_VERSION set to $3, and check `batfiles version`: through the
# scriptblock one-liner when a version is requested, and otherwise through
# `irm | iex` followed by the installed batfiles' own `version`.
try_windows() {
    bin=$(cygpath -w "$work/$2/batfiles.exe")
    if [ -n "$3" ]; then
        command="& ([scriptblock]::Create((irm '$1'))) version; exit \$LASTEXITCODE"
    else
        command="irm '$1' | iex; & '$bin' version; exit \$LASTEXITCODE"
    fi
    reported=$(BATFILES_BASE=$url BATFILES_VERSION=$3 BATFILES_BIN=$bin \
        pwsh -NoLogo -NoProfile -NonInteractive -Command "$command") ||
        die "installing with $1 failed"
    reported=$(printf '%s' "$reported" | tr -d '\r')
    [ "$reported" = "batfiles $version" ] ||
        die "installing with $1 gave '$reported', not 'batfiles $version'"
    echo "dist:smoke: $1 installed batfiles $version on Windows $(uname -m)"
}

case $(uname -s) in
MINGW* | MSYS* | CYGWIN*) installer=install.ps1 attempt=try_windows exe=.exe ;;
*) installer=install.sh attempt=try exe= ;;
esac

$attempt "$url/download/v$version/$installer" pinned "$version"
if [ "$latest" = yes ]; then
    $attempt "$url/latest/download/$installer" latest ""
    check=$(BATFILES_BASE=$url "$work/latest/batfiles$exe" update --check) ||
        die "batfiles update --check failed"
    check=$(printf '%s' "$check" | tr -d '\r')
    printf '%s\n' "$check" | grep -qx "available $version" ||
        die "batfiles update --check reported '$check', not 'available $version'"
    echo "dist:smoke: batfiles update --check finds $version"
fi
