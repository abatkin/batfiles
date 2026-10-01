#!/bin/sh
# Build one release binary and stage it as a release asset.
#
# Usage: dist/binary.sh <target> [<base>] [xwin] [<version>]
#
# Builds the `dist` profile for <target> and copies the result to
# $CARGO_TARGET_DIR/assets/batfiles-<target>[.exe], where `dist/assemble.sh`
# looks for it. <base> is compiled in as the default release base; without it
# the binary falls back to the official base. `xwin` builds a Windows target
# with `cargo xwin` instead of a native toolchain. <version>, the Cargo.toml
# version or a pre-release of it, is the version the binary reports; it defaults
# to the Cargo.toml version.
#
# Refuses a Linux or Windows binary that links a C runtime dynamically, and runs
# `version` on a binary this host can execute, which must report <version>.
set -eu

die() {
    echo "dist:binary: $*" >&2
    exit 1
}

if [ $# -lt 1 ] || [ $# -gt 4 ]; then
    die "usage: dist/binary.sh <target> [<base>] [xwin] [<version>]"
fi
target=$1
base=${2:-}
cross=${3:-}
release=${4:-}

root=$(cd "$(dirname "$0")/.." && pwd)
# shellcheck source=dist/version.sh
. "$root/dist/version.sh"
target_dir=${CARGO_TARGET_DIR:-$root/target}
host=$(rustc -vV | sed -n 's/^host: //p')
cargo=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
version=${release:-$cargo}
case $version in
"$cargo" | "$cargo"-*) ;;
*) die "version $version is neither the Cargo.toml version $cargo nor a pre-release of it" ;;
esac
is_version "$version" ||
    die "version '$version' is not X.Y.Z or X.Y.Z-<pre-release>; see docs/distribution.md#versions"

case $target in
*-windows-*) exe=.exe ;;
*) exe= ;;
esac
case $cross in
'') build="cargo build" ;;
xwin)
    case $target in
    *-windows-msvc)
        build="cargo xwin build"
        export XWIN_ACCEPT_LICENSE=1
        ;;
    *) die "xwin builds only a *-windows-msvc target, not $target" ;;
    esac
    ;;
*) die "unknown cross-compiler '$cross'; the only one is xwin" ;;
esac

# The target triple as Cargo spells it in an environment variable's name.
target_var=$(printf '%s' "$target" | tr 'a-z-' 'A-Z_')

case $target in
*-windows-msvc)
    # Without this, the binary needs the Visual C++ runtime DLLs, which a fresh
    # Windows machine may not have.
    export "CARGO_TARGET_${target_var}_RUSTFLAGS=-C target-feature=+crt-static"
    ;;
*-linux-musl)
    # The ring crate compiles C; a native musl build needs musl's compiler
    # wrapper rather than the host's glibc one.
    cc_var=CC_$(printf '%s' "$target" | tr '-' '_')
    eval "cc=\${$cc_var:-}"
    if [ -z "$cc" ] && [ "${target%%-*}" = "${host%%-*}" ] &&
        command -v musl-gcc >/dev/null 2>&1; then
        export "$cc_var=musl-gcc"
    fi
    ;;
esac

if [ -n "$base" ]; then
    export BATFILES_DEFAULT_BASE="$base"
fi
export BATFILES_RELEASE_VERSION="$version"

rustup target add "$target"
(cd "$root" && $build --locked --profile dist --target "$target")

built=$target_dir/$target/dist/batfiles$exe
asset=$target_dir/assets/batfiles-$target$exe
mkdir -p "$target_dir/assets"
cp "$built" "$asset"
chmod 0755 "$asset"

# Whether the binary depends on a C runtime the target machine must supply.
sysroot=$(rustc --print sysroot)
if command -v cygpath >/dev/null 2>&1; then
    sysroot=$(cygpath -u "$sysroot")
fi
readobj=$sysroot/lib/rustlib/$host/bin/llvm-readobj
case $target in
*-linux-*)
    dynamic=$("$readobj" --dynamic-table "$asset")
    if printf '%s\n' "$dynamic" | grep -q NEEDED; then
        die "$asset links shared libraries; a Linux release binary must be static"
    fi
    ;;
*-windows-*)
    imports=$("$readobj" --coff-imports "$asset")
    if printf '%s\n' "$imports" | grep -Eqi 'Name: *(vcruntime|msvcp|api-ms-win-crt)'; then
        die "$asset imports the Visual C++ runtime; it must link it statically"
    fi
    ;;
esac

# The same architecture and operating system: this host can run it.
os_of() {
    case $1 in
    *-linux-*) echo linux ;;
    *-darwin) echo darwin ;;
    *-windows-*) echo windows ;;
    *) echo "$1" ;;
    esac
}
if [ "${target%%-*}" = "${host%%-*}" ] && [ "$(os_of "$target")" = "$(os_of "$host")" ]; then
    reported=$("$asset" version)
    [ "$reported" = "batfiles $version" ] ||
        die "$asset reports '$reported', not 'batfiles $version'"
fi

echo "dist:binary: $asset"
