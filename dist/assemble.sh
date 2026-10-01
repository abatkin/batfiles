#!/bin/sh
# Assemble one release's complete asset set from binaries already built.
#
# Usage: dist/assemble.sh <version> <base> <out> [<in>] [<targets>] [<installers>]
#
# Copies every batfiles-<target>[.exe] in <in> to <out>, writes VERSION and
# SHA256SUMS, and stamps <base> into the hosted installers. An empty or omitted
# <in> is where `dist/binary.sh` stages binaries. <out> must be absent or empty,
# and is written only once the set is complete. <targets>, a space-separated
# list, requires the binaries in <in> to be exactly those targets. The installers
# are stamped from install.sh and install.ps1 in <installers>, by default
# `dist/`; a stamped copy is restamped.
set -eu

die() {
    echo "dist:assemble: $*" >&2
    exit 1
}

if [ $# -lt 3 ] || [ $# -gt 6 ]; then
    die "usage: dist/assemble.sh <version> <base> <out> [<in>] [<targets>] [<installers>]"
fi
here=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=dist/version.sh
. "$here/version.sh"
version=$1
base=${2%/}
out=${3%/}
in=${4:-${CARGO_TARGET_DIR:-$here/../target}/assets}
targets=${5:-}
installers=${6:-$here}

is_version "$version" ||
    die "version '$version' is not X.Y.Z or X.Y.Z-<pre-release>; see docs/distribution.md#versions"
# Every character the stamped installers quote safely.
printf '%s\n' "$base" |
    grep -Eqx "[A-Za-z][A-Za-z0-9+.-]*://[A-Za-z0-9._~:/@%+=,;!*()-]+" ||
    die "base '$base' is not a URL, or holds a character an installer cannot quote"
[ -d "$in" ] || die "no binaries directory $in"
if [ -e "$out" ]; then
    [ -d "$out" ] || die "$out exists and is not a directory"
    [ -z "$(ls -A "$out")" ] || die "$out is not empty"
fi

found=
for binary in "$in"/batfiles-*; do
    [ -e "$binary" ] || continue
    name=${binary##*/}
    target=${name#batfiles-}
    target=${target%.exe}
    case $target in
    *-windows-*) [ "$name" = "batfiles-$target.exe" ] || die "$name is a Windows binary without .exe" ;;
    *) [ "$name" = "batfiles-$target" ] || die "$name is not a Windows binary but ends in .exe" ;;
    esac
    found="$found $target"
done
[ -n "$found" ] || die "no batfiles-<target> binaries in $in"
if [ -n "$targets" ]; then
    for target in $targets; do
        case " $found " in *" $target "*) ;; *) die "no binary for $target in $in" ;; esac
    done
    for target in $found; do
        case " $targets " in *" $target "*) ;; *) die "$in holds batfiles-$target, which is not a release target" ;; esac
    done
fi

mkdir -p "$(dirname "$out")"
stage=$(mktemp -d "$(dirname "$out")/.assemble.XXXXXX")
trap 'rm -rf "$stage"' EXIT

for binary in "$in"/batfiles-*; do
    cp "$binary" "$stage/"
    chmod 0755 "$stage/${binary##*/}"
done

printf '%s\n' "$version" >"$stage/VERSION"

if command -v sha256sum >/dev/null 2>&1; then
    (cd "$stage" && sha256sum batfiles-*) >"$stage/SHA256SUMS.tmp"
elif command -v shasum >/dev/null 2>&1; then
    (cd "$stage" && shasum -a 256 batfiles-*) >"$stage/SHA256SUMS.tmp"
else
    die "neither sha256sum nor shasum is available"
fi
mv "$stage/SHA256SUMS.tmp" "$stage/SHA256SUMS"

# Copy <source> to <dest>, replacing the one line that begins with <prefix>
# by <line>.
stamp() {
    awk -v prefix="$3" -v line="$4" '
        index($0, prefix) == 1 { n++; print line; next }
        { print }
        END { if (n != 1) exit 3 }
    ' "$1" >"$2" || die "$1 does not have exactly one line beginning '$3'"
}
stamp "$installers/install.sh" "$stage/install.sh" \
    "batfiles_stamped_base=" "batfiles_stamped_base='$base'"
stamp "$installers/install.ps1" "$stage/install.ps1" \
    "\$BatfilesStampedBase = " "\$BatfilesStampedBase = '$base'"
chmod 0644 "$stage/VERSION" "$stage/SHA256SUMS" "$stage/install.sh" "$stage/install.ps1"
chmod 0755 "$stage"

[ ! -d "$out" ] || rmdir "$out"
mv "$stage" "$out"
trap - EXIT
echo "dist:assemble: $out, version $version, targets:$found"
