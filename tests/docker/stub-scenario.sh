#!/usr/bin/env bash
# The leaf stub's acceptance, on a machine of its own: a checkout that arrived
# by plain `git clone` installs itself with `./install.sh`, which `batfiles init`
# wrote into the repository. The release tree is the one the image assembled
# under /srv/releases; BATFILES_BASE points the committed stub at it, as a
# self-hoster's environment would.
set -euo pipefail

repo=$HOME/dotfiles
batfiles=$HOME/.local/bin/batfiles
releases=file:///srv/releases
checks=0

fail() {
    echo "FAIL: $*" >&2
    exit 1
}

ok() {
    checks=$((checks + 1))
}

step() {
    echo "== $*"
}

absent() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        fail "$1 exists: $2"
    fi
    ok
}

contains() {
    grep -qF -- "$2" "$1" || fail "$1 does not contain '$2': $3"
    ok
}

lacks() {
    if grep -qF -- "$2" "$1"; then
        fail "$1 contains '$2': $3"
    fi
    ok
}

# Run the stub as a user would, from the home directory, keeping its output in
# the log named first, and report whether it succeeded.
stub() {
    local log=$1 status=0
    shift
    "$repo/install.sh" "$@" >"$log" 2>&1 || status=$?
    cat "$log"
    return "$status"
}

step "a checkout arrives by git clone, on a machine with no batfiles"

git clone --quiet /srv/personal.git "$repo"
[ -x "$repo/install.sh" ] || fail "the checkout has no executable install.sh"
ok
if command -v batfiles >/dev/null; then
    fail "batfiles is on PATH: this machine is supposed to be pristine"
fi
ok
absent "$HOME/.local" "this machine is supposed to be pristine"

step "piped into sh, the stub has no checkout and says how to clone instead"

log=$(mktemp)
if dash <"$repo/install.sh" >"$log" 2>&1; then
    cat "$log"
    fail "the piped stub succeeded without a checkout"
fi
cat "$log"
contains "$log" "sh -s -- clone" "the refusal did not point at the clone one-liner"
absent "$batfiles" "the piped stub installed something"

step "with no batfiles, the stub fetches one, bootstraps, and synchronizes"

log=$(mktemp)
BATFILES_BASE=$releases stub "$log" || fail "the stub did not succeed"
contains "$log" "installed batfiles" "the stub did not install batfiles"
[ -x "$batfiles" ] || fail "no batfiles at $batfiles"
ok
version=$("$batfiles" version)
version=${version#batfiles }

[ "$(readlink "$HOME/.zshrc")" = "$repo/shell/zshrc" ] ||
    fail "the synchronization did not link the shell"
ok

# The bootstrap policy, which `sync --bootstrap` adopts as `clone` would.
disabled=$HOME/.config/batfiles/disabled.toml
[ "$(cat "$disabled")" = 'actions = ["batgrep"]
groups = ["editor"]' ] || fail "the bootstrap wrote: $(cat "$disabled" 2>/dev/null)"
ok
# The backticks are batfiles' own quoting, not command substitution.
# shellcheck disable=SC2016
contains "$log" 'default-disabled: disabled action `batgrep`' "the bootstrap was not reported"
absent "$HOME/.local/bin/batgrep" "a default-disabled action was installed"
absent "$HOME/.config/nvim" "a default-disabled group was installed"

step "with batfiles installed and the base unreachable, the stub works offline"

log=$(mktemp)
BATFILES_BASE=file:///nonexistent stub "$log" --dry-run ||
    fail "the stub needed the network to use an installed batfiles"
lacks "$log" "downloading" "the stub downloaded with a batfiles already installed"

step "pinned, the stub passes over an older batfiles on PATH, and leaves it alone"

mkdir "$HOME/old-bin"
printf '#!/bin/sh\necho "batfiles 0.0.1"\n' >"$HOME/old-bin/batfiles"
chmod +x "$HOME/old-bin/batfiles"
before=$(cat "$HOME/old-bin/batfiles")

log=$(mktemp)
PATH=$HOME/old-bin:$PATH BATFILES_BASE=$releases BATFILES_VERSION=$version stub "$log" ||
    fail "the pinned stub did not succeed"
contains "$log" "warning: passing over $HOME/old-bin/batfiles" "the older batfiles was not passed over"
contains "$log" "using batfiles $version at $batfiles" "the pinned stub did not use the batfiles that meets the pin"
[ "$(cat "$HOME/old-bin/batfiles")" = "$before" ] || fail "the older batfiles was modified"
ok

echo "stub acceptance: $checks checks passed"
