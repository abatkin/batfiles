#!/bin/sh
# Build and check the documentation with the tools pinned in dist/docs-tools.
#
# Usage: dist/docs.sh install
#        dist/docs.sh build <site-url>
#        dist/docs.sh serve
#        dist/docs.sh check <site-url>
#        dist/docs.sh sources [<file>...]
#        dist/docs.sh html <dir> <site-url>
#
# `install` fetches this host's pinned mdBook and lychee archives, verifies them
# against their digests, and installs the binaries into target/docs-tools/bin;
# tools already there at the pinned version are kept. `build` writes the book
# to target/docs for deployment under <site-url>, and `serve` previews it at /.
# `check` builds for <site-url>, then runs `sources` and `html` on the result.
# `sources` checks the links in the given Markdown files, by default the book
# and the repository's top-level guides. `html` checks a built site <dir> as it
# would be served at <site-url>, so a root-relative link outside that prefix
# fails. Both check fragments, map this repository's GitHub URLs on main to
# local files, and contact no other site. <site-url> starts and ends with `/`.
set -eu

die() {
    echo "docs: $*" >&2
    exit 1
}

root=$(cd "$(dirname "$0")/.." && pwd)
tools=$root/target/docs-tools/bin
pins=$root/dist/docs-tools

# The pinned version of tool $1.
pinned() {
    awk -v tool="$1" '$1 == tool { print $2; exit }' "$pins"
}

# The path of tool $1, which must be installed at its pinned version.
tool() {
    version=$(pinned "$1")
    case $("$tools/$1" --version 2>/dev/null) in
    "$1 $version" | "$1 v$version") echo "$tools/$1" ;;
    *) die "$1 $version is not installed; run task docs:install" ;;
    esac
}

host_target() {
    case $(uname -s)/$(uname -m) in
    Linux/x86_64) echo x86_64-unknown-linux-musl ;;
    Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-musl ;;
    Darwin/x86_64) echo x86_64-apple-darwin ;;
    Darwin/arm64) echo aarch64-apple-darwin ;;
    *) die "no pinned documentation tools for $(uname -s) $(uname -m); install the versions in dist/docs-tools into $tools" ;;
    esac
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1"
    else
        shasum -a 256 "$1"
    fi | cut -d ' ' -f 1
}

install_tool() {
    name=$1
    version=$(pinned "$name")
    case $("$tools/$name" --version 2>/dev/null) in
    "$name $version" | "$name v$version") return ;;
    esac
    target=$(host_target)
    digest=$(awk -v tool="$name" -v target="$target" \
        '$1 == tool && $3 == target { print $4; exit }' "$pins")
    [ -n "$digest" ] || die "dist/docs-tools pins no $name for $target"
    case $name in
    mdbook)
        url=https://github.com/rust-lang/mdBook/releases/download/v$version/mdbook-v$version-$target.tar.gz
        member=mdbook
        ;;
    lychee)
        url=https://github.com/lycheeverse/lychee/releases/download/lychee-v$version/lychee-$target.tar.gz
        member=lychee-$target/lychee
        ;;
    *) die "dist/docs-tools names an unknown tool $name" ;;
    esac
    curl -fsSL --proto '=https' --retry 3 -o "$work/$name.tar.gz" "$url" ||
        die "cannot fetch $url"
    actual=$(sha256 "$work/$name.tar.gz")
    [ "$actual" = "$digest" ] ||
        die "$url has SHA-256 $actual, but dist/docs-tools pins $digest"
    mkdir -p "$work/$name" "$tools"
    tar -xzf "$work/$name.tar.gz" -C "$work/$name" "$member"
    cp "$work/$name/$member" "$tools/.$name.tmp"
    chmod 0755 "$tools/.$name.tmp"
    mv "$tools/.$name.tmp" "$tools/$name"
    echo "docs: installed $name $version" >&2
}

site_url() {
    case $1 in
    /*/ | /) ;;
    *) die "the site URL '$1' must start and end with /" ;;
    esac
}

# mdBook's command $2 on the book, for deployment under URL path $1.
run_mdbook() {
    bin=$(tool mdbook)
    MDBOOK_OUTPUT__HTML__SITE_URL=$1 "$bin" "$2" "$root"
}

run_lychee() {
    bin=$(tool lychee)
    "$bin" --offline --include-fragments --no-ignore --no-progress --mode plain \
        --remap "^https://github\\.com/abatkin/batfiles/(blob|tree|edit)/main/ file://$root/" "$@"
}

sources() {
    if [ $# -eq 0 ]; then
        cd "$root"
        set -- README.md AGENTS.md dist/README.md 'docs/**/*.md'
    fi
    run_lychee "$@" >&2 || die "the Markdown sources have broken links"
}

# Check built site $1 as served at URL path $2: a scratch root holds the site
# at that path, so that root-relative links resolve as they will when served.
html() {
    site_url "$2"
    [ -f "$1/index.html" ] || die "no index.html in $1"
    dir=$(cd "$1" && pwd)
    if [ "$2" = / ]; then
        served=$dir
        served_root=$dir
    else
        served=$work/served${2%/}
        served_root=$work/served
        mkdir -p "$(dirname "$served")"
        ln -s "$dir" "$served"
    fi
    run_lychee --root-dir "$served_root" "$served/**/*.html" >&2 ||
        die "the site in $1 has broken links"
}

[ $# -ge 1 ] || die "usage: dist/docs.sh install | build <site-url> | serve | check <site-url> | sources [<file>...] | html <dir> <site-url>"
command=$1
shift
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
mkdir "$work/served"

case $command/$# in
install/0)
    install_tool mdbook
    install_tool lychee
    ;;
build/1)
    site_url "$1"
    run_mdbook "$1" build
    ;;
serve/0) run_mdbook / serve ;;
check/1)
    site_url "$1"
    run_mdbook "$1" build
    sources
    html "$root/target/docs" "$1"
    echo "docs: links are valid" >&2
    ;;
sources/*) sources "$@" ;;
html/2) html "$1" "$2" ;;
*) die "unknown command or wrong arguments: $command $*" ;;
esac
