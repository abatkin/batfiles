#!/bin/sh
# The batfiles hosted installer.
#
#   curl -fsSL <base>/latest/download/install.sh | sh
#   curl -fsSL <base>/latest/download/install.sh | sh -s -- clone <url>
#
# Uses a batfiles already on this machine, or downloads and verifies one, then
# runs it with any arguments given. Inputs, all optional: BATFILES_BASE (the
# release base, by default the one stamped below), BATFILES_VERSION (a release
# to fetch, and the oldest one accepted), and BATFILES_BIN (the one batfiles to
# use or install, instead of searching PATH and then $HOME/.local/bin).
# docs/distribution.md specifies the rest.
#
# `dist/assemble.sh` stamps the release base into the line below. An unstamped
# copy runs only with BATFILES_BASE set.
batfiles_stamped_base=unstamped

say() {
    printf 'install.sh: %s\n' "$*" >&2
}

fail() {
    say "$*"
    exit 1
}

# SemVer without build metadata, as docs/distribution.md#versions specifies.
# dist/version.sh holds the same line; tests/dist checks they agree.
version_pattern='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?'

# Whether $1 is a version.
is_version() {
    printf '%s\n' "$1" | grep -Eqx "$version_pattern"
}

# Whether version $1 is at least version $2, both valid, by SemVer precedence:
# a pre-release ranks below its release, and its identifiers compare as
# numbers when both are digits and as ASCII strings otherwise. Numbers compare
# exactly, by length and then digit by digit, which needs no leading zeros;
# string comparison is forced with "" and the C locale, since awk would
# otherwise read an identifier such as 1e2 as a number.
version_at_least() {
    LC_ALL=C awk -v have="$1" -v want="$2" '
        function core(v,  i) { i = index(v, "-"); return i ? substr(v, 1, i - 1) : v }
        function pre(v,  i) { i = index(v, "-"); return i ? substr(v, i + 1) : "" }
        function numeric(s) { return s ~ /^[0-9]+$/ }
        function strings(a, b) {
            a = a ""; b = b ""
            return (a == b) ? 0 : ((a < b) ? -1 : 1)
        }
        function numbers(a, b) {
            if (length(a) != length(b)) return (length(a) < length(b)) ? -1 : 1
            return strings(a, b)
        }
        function compare(a, b,  ca, cb, pa, pb, x, y, n, m, k, c) {
            split(core(a), ca, "."); split(core(b), cb, ".")
            for (k = 1; k <= 3; k++)
                if ((c = numbers(ca[k], cb[k])) != 0) return c
            pa = pre(a); pb = pre(b)
            if (pa == "" && pb == "") return 0
            if (pa == "") return 1
            if (pb == "") return -1
            n = split(pa, x, "."); m = split(pb, y, ".")
            for (k = 1; k <= n && k <= m; k++) {
                if (numeric(x[k]) && numeric(y[k])) c = numbers(x[k], y[k])
                else if (numeric(x[k])) c = -1
                else if (numeric(y[k])) c = 1
                else c = strings(x[k], y[k])
                if (c != 0) return c
            }
            return (n < m) ? -1 : ((n > m) ? 1 : 0)
        }
        BEGIN { exit (compare(have, want) >= 0) ? 0 : 1 }
    '
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

# The release target for this machine.
detect_target() {
    os=$(uname -s)
    arch=$(uname -m)
    case $os in
    Linux) suffix=unknown-linux-musl ;;
    Darwin) suffix=apple-darwin ;;
    *) fail "there is no batfiles release for $os on $arch" ;;
    esac
    case $arch in
    x86_64 | amd64) cpu=x86_64 ;;
    aarch64 | arm64) cpu=aarch64 ;;
    *) fail "there is no batfiles release for $os on $arch" ;;
    esac
    # An x86_64 shell under Rosetta on Apple silicon.
    if [ "$os" = Darwin ] && [ "$cpu" = x86_64 ] &&
        [ "$(sysctl -n hw.optional.arm64 2>/dev/null)" = 1 ]; then
        cpu=aarch64
    fi
    printf '%s\n' "$cpu-$suffix"
}

# Download $1 to $2, returning failure when the download fails.
try_download() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        fail "neither curl nor wget is available to download batfiles"
    fi
}

# Download $1 to $2, or fail.
download() {
    try_download "$1" "$2" || fail "cannot download $1"
}

# The SHA-256 digest of $1.
digest() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print $1 }'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{ print $1 }'
    else
        return 1
    fi
}

# Download, verify, and install the requested release as $bin_path, setting
# $installed_version. Refuses to replace anything there that is not batfiles.
install_release() {
    if [ -d "$bin_path" ]; then
        fail "$bin_path is a directory; set BATFILES_BIN to the path of the batfiles to install"
    fi
    if [ -e "$bin_path" ] || [ -L "$bin_path" ]; then
        version_of "$bin_path" >/dev/null ||
            fail "$bin_path is not a batfiles, so it is left as it is; move it aside, or set BATFILES_BIN to another path"
    fi
    target=$(detect_target) || exit 1
    bin_dir=${bin_path%/*}
    bin_dir=${bin_dir:-/}
    mkdir -p "$bin_dir" || fail "cannot create $bin_dir"
    staged=$(mktemp "$bin_dir/.batfiles.XXXXXX") || fail "cannot write in $bin_dir"
    sums=$(mktemp "$bin_dir/.SHA256SUMS.XXXXXX") || fail "cannot write in $bin_dir"
    trap 'rm -f "$staged" "$sums"' EXIT
    trap 'exit 1' HUP INT TERM

    # The latest release is read once, from its VERSION, and everything else
    # comes from that release's own directory, so a release published midway
    # cannot mix two.
    release=$want
    if [ -z "$release" ]; then
        if ! try_download "$base/latest/download/VERSION" "$sums"; then
            say "cannot download $base/latest/download/VERSION"
            fail "if this release base has no stable release yet, set BATFILES_VERSION to a pre-release"
        fi
        read -r release <"$sums" || release=
        is_version "$release" ||
            fail "$base/latest/download/VERSION holds '$release', not a version"
    fi
    from=$base/download/v$release

    say "downloading batfiles-$target $release from $from"
    download "$from/batfiles-$target" "$staged"
    download "$from/SHA256SUMS" "$sums"
    expected=$(awk -v name="batfiles-$target" '$2 == name { print $1 }' "$sums")
    [ -n "$expected" ] || fail "$from/SHA256SUMS lists no batfiles-$target"
    actual=$(digest "$staged") ||
        fail "neither sha256sum nor shasum is available to verify the download; nothing was installed"
    [ "$actual" = "$expected" ] ||
        fail "batfiles-$target does not match $from/SHA256SUMS; nothing was installed"

    chmod 0755 "$staged" || fail "cannot make $staged executable"
    installed_version=$(version_of "$staged") ||
        fail "the downloaded batfiles-$target does not run on this machine; nothing was installed"
    [ "$installed_version" = "$release" ] ||
        fail "the download reports version $installed_version, not $release; nothing was installed"
    mv -f "$staged" "$bin_path" || fail "cannot install $bin_path"
    rm -f "$sums"
    trap - EXIT HUP INT TERM
    say "installed batfiles $installed_version as $bin_path"
    case ":${PATH:-}:" in
    *":$bin_dir:"*) ;;
    *) say "$bin_dir is not on PATH; add it there to run batfiles by name" ;;
    esac
}

main() {
    base=${BATFILES_BASE:-$batfiles_stamped_base}
    [ "$base" != unstamped ] ||
        fail "this copy was never stamped with a release base; set BATFILES_BASE, or use the one published with a release"
    base=${base%/}

    want=${BATFILES_VERSION:-}
    case $want in
    latest) want= ;;
    v*) want=${want#v} ;;
    esac
    if [ -n "$want" ] && ! is_version "$want"; then
        fail "BATFILES_VERSION is '$BATFILES_VERSION', not a version such as 1.2.3 or 1.2.3-rc.1"
    fi

    # The first candidate that runs and is at least the requested version wins.
    candidates
    bin=
    for candidate in "$on_path" "$bin_path"; do
        [ -n "$candidate" ] && [ -f "$candidate" ] && [ -x "$candidate" ] || continue
        if ! have=$(version_of "$candidate"); then
            # What is at $bin_path is reported if a download would replace it.
            if [ "$candidate" = "$on_path" ]; then
                say "passing over $candidate, which does not report a batfiles version"
            fi
            continue
        fi
        if [ -z "$want" ] || version_at_least "$have" "$want"; then
            bin=$candidate
            say "using batfiles $have at $bin"
            break
        fi
        if [ "$candidate" = "$on_path" ]; then
            say "warning: passing over $candidate, batfiles $have, which is older than $want; it is left as it is"
        fi
    done

    if [ -z "$bin" ]; then
        install_release
        bin=$bin_path
    elif [ $# -eq 0 ] && [ -z "$want" ]; then
        say "to upgrade it, run 'batfiles update'"
    fi

    [ $# -gt 0 ] || exit 0
    # Piped into sh, standard input is the script; give batfiles the terminal.
    if [ ! -t 0 ] && (: </dev/tty) 2>/dev/null; then
        exec "$bin" "$@" </dev/tty
    fi
    exec "$bin" "$@"
}

main "$@"
