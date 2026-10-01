#!/bin/sh
# batfiles-stub 1
BATFILES_BASE=${BATFILES_BASE:-'@BATFILES_BASE@'}
BATFILES_VERSION=${BATFILES_VERSION:-}

# Installs this checkout with batfiles, which `batfiles init` wrote this file
# for: it uses a batfiles already on this machine, or fetches one with the
# hosted installer from BATFILES_BASE, then runs
# `batfiles sync --bootstrap --batfiles-dir <this directory>` with any arguments
# given, such as --dry-run. BATFILES_VERSION, when set, is the oldest release
# this repository accepts. BATFILES_BIN names the one batfiles to use or
# install. To set up a machine with no checkout, use the hosted installer's
# one-liner with `clone` instead.

say() {
    printf 'install.sh: %s\n' "$*" >&2
}

fail() {
    say "$*"
    exit 1
}

# The release version grammar, as the hosted installer has it.
version_pattern='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?'

# Whether $1 is a version.
is_version() {
    printf '%s\n' "$1" | grep -Eqx "$version_pattern"
}

# The version the batfiles at $1 reports, or failure if it does not run or
# reports something else.
version_of() {
    reported=$("$1" version 2>/dev/null) || return 1
    case $reported in
    "batfiles "*) reported=${reported#batfiles } ;;
    *) return 1 ;;
    esac
    is_version "$reported" || return 1
    printf '%s\n' "$reported"
}

# Set $on_path and $bin_path, the batfiles to consider, in order.
# BATFILES_BIN is the only one when it is set; otherwise they are the batfiles
# on PATH and $HOME/.local/bin/batfiles, and the last is where a download goes.
candidates() {
    on_path=
    if [ -n "${BATFILES_BIN:-}" ]; then
        bin_path=$BATFILES_BIN
        case $bin_path in /*) ;; *) bin_path=$(pwd)/$bin_path ;; esac
    else
        [ -n "${HOME:-}" ] || fail "HOME is not set; set BATFILES_BIN instead"
        bin_path=$HOME/.local/bin/batfiles
        on_path=$(command -v batfiles 2>/dev/null) || on_path=
        # A relative PATH entry gives a relative path, anchored here since this
        # script never changes directory; a function, alias, or builtin gives a
        # bare name, which is not a batfiles to run.
        case $on_path in
        /*) ;;
        */*) on_path=$(pwd)/$on_path ;;
        *) on_path= ;;
        esac
        [ "$on_path" != "$bin_path" ] || on_path=
    fi
}

# Print the document at $1.
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O - "$1"
    else
        say "neither curl nor wget is available to download batfiles"
        return 1
    fi
}

main() {
    # This file's directory, which must be a checkout: piped into sh, $0 is the
    # shell rather than this file.
    case $0 in
    */*) dir=${0%/*} ;;
    *) dir=. ;;
    esac
    if [ ! -f "$0" ] || [ ! -f "$dir/batfiles.toml" ]; then
        say "this runs from a checkout of a batfiles repository; to set up a machine without one, run:"
        fail "curl -fsSL $BATFILES_BASE/latest/download/install.sh | sh -s -- clone <repository-url>"
    fi
    dir=$(cd "$dir" && pwd) || fail "cannot enter $dir"

    version=${BATFILES_VERSION#v}
    [ "$version" != latest ] || version=

    candidates
    found=
    for candidate in "$on_path" "$bin_path"; do
        [ -n "$candidate" ] && [ -f "$candidate" ] && [ -x "$candidate" ] || continue
        if version_of "$candidate" >/dev/null; then
            found=$candidate
            break
        fi
    done

    # With no version requested, any batfiles will do, and nothing is fetched.
    if [ -n "$found" ] && [ -z "$version" ]; then
        exec "$found" sync --bootstrap --batfiles-dir "$dir" "$@"
    fi

    # Otherwise the hosted installer decides, matching the requested release.
    if [ -n "$version" ]; then
        from=$BATFILES_BASE/download/v$version
    else
        from=$BATFILES_BASE/latest/download
    fi
    if installer=$(fetch "$from/install.sh"); then
        export BATFILES_BASE BATFILES_VERSION
        printf '%s\n' "$installer" | sh -s -- sync --bootstrap --batfiles-dir "$dir" "$@"
        exit
    fi
    if [ -n "$found" ]; then
        say "warning: cannot fetch $from/install.sh to check $found against BATFILES_VERSION=$BATFILES_VERSION; using it unchecked"
        exec "$found" sync --bootstrap --batfiles-dir "$dir" "$@"
    fi
    fail "cannot fetch $from/install.sh, and this machine has no batfiles to use instead"
}

main "$@"
