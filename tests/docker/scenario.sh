#!/usr/bin/env bash
# The pristine-machine acceptance: one command turns a URL into a set-up
# machine, and a second run over that machine changes nothing it should not.
#
# Nothing here sets a BATFILES_* or XDG_* variable, which is the whole point.
# Every test under `tests/cli/` pins all four roots at a temporary directory, so
# what a real machine does -- clone into $HOME/dotfiles, write
# $HOME/.config/batfiles/disabled.toml, create the ~/.config and ~/.cache
# parents nothing has made yet -- is decided only here.
set -euo pipefail

repo=$HOME/dotfiles
remote=$repo/remotes/corporate
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

# A path nothing has created. Tested against the link rather than its target, so
# that a dangling symlink counts as present.
absent() {
    if [ -e "$1" ] || [ -L "$1" ]; then
        fail "$1 exists: $2"
    fi
    ok
}

is_dir() {
    [ -d "$1" ] || fail "$1 is not a directory: $2"
    ok
}

# A seed or a fetched file: content at the destination rather than a link back
# into the repository, because what lands there is meant to be edited.
is_file() {
    [ -f "$1" ] || fail "$1 is not a file: $2"
    if [ -L "$1" ]; then
        fail "$1 is a symlink rather than content: $2"
    fi
    ok
}

links_to() {
    local actual
    [ -L "$1" ] || fail "$1 is not a symlink: $3"
    actual=$(readlink "$1")
    [ "$actual" = "$2" ] || fail "$1 links to $actual, not $2"
    [ -e "$1" ] || fail "$1 links to nothing that exists"
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

reported() {
    contains "$1" "$2" "the run did not report it"
}

# Run batfiles, keeping the output for the assertions that read it and showing
# it either way: a failure here is read from the log rather than reproduced.
run() {
    local log=$1
    shift
    if ! batfiles "$@" >"$log" 2>&1; then
        cat "$log" >&2
        fail "batfiles $* did not succeed"
    fi
    cat "$log"
}

step "the machine has nothing on it yet"

for path in "$repo" "$HOME/.config" "$HOME/.cache" "$HOME/.local" \
    "$HOME/.gitconfig" "$HOME/.zshrc"; do
    absent "$path" "this machine is supposed to be pristine"
done

step "clone: one command brings the repository down and sets the machine up"

log=$(mktemp)
run "$log" clone /srv/personal.git

reported "$log" "cloned $repo from /srv/personal.git"

is_dir "$repo/.git" "the clone left no git directory"
is_file "$repo/batfiles.toml" "the clone left no manifest"

step "the leaf's own actions ran against a home that had none of their parents"

links_to "$HOME/.zshrc" "$repo/shell/zshrc" "the shell is not linked"
links_to "$HOME/.zshenv" "$repo/shell/zshenv" "the shell is not linked"
links_to "$HOME/.config/zsh/aliases.zsh" "$repo/shell/aliases.zsh" \
    "a link two missing parents deep"
links_to "$HOME/.gitconfig" "$repo/git/gitconfig" "git is not configured"
links_to "$HOME/.config/git/ignore" "$repo/git/gitignore" "git is not configured"

# `files/` a child at a time, dotted on the way out.
for name in ackrc curlrc inputrc; do
    links_to "$HOME/.$name" "$repo/files/$name" "the rc files are not linked"
done

is_dir "$HOME/.cache/zsh" "the history directory was not created"
absent "$HOME/.cache/work-tools" "the work variable is false, so this was gated off"

step "the seeds landed as content, and declaration order decided the collision"

is_file "$HOME/.config/git/local" "the machine-local identity is a seed"
contains "$HOME/.config/git/local" "Machine-local git identity" \
    "the seed holds the template"
is_file "$HOME/.config/zsh/profile.zsh" "the profile is a seed"
contains "$HOME/.config/zsh/profile.zsh" "PROMPT_STYLE=verbose" \
    "the first of the two seeds naming this destination is the one that landed"
for name in env prompt; do
    is_file "$HOME/.config/zsh/local/$name.zsh" "the local directory is seeded"
done

step "the inclusion installed what a repository this machine never named declares"

is_file "$remote/batfiles.toml" "the corporate remote was not materialized"
links_to "$HOME/.zshrc.corporate" "$remote/files/zshrc" \
    "the included shell settings are not linked"
links_to "$HOME/.p10k.zsh" "$remote/files/p10k.zsh" \
    "the included prompt is not linked"
is_file "$HOME/.config/corporate/gitconfig" "the included seeds are copies"
contains "$HOME/.config/corporate/gitconfig" "you@corp.example" \
    "the seed holds the corporate identity"
is_file "$HOME/.config/corporate/npmrc" "the included seeds are copies"

step "the leaf's dynamic variable ran, and was cached under the XDG default"

cache=$HOME/.cache/batfiles/dynamic-vars.toml
is_file "$cache" "the capture wrote no cache under the XDG default"
contains "$cache" "[has_git]" "the leaf's variable was not cached"
contains "$cache" 'value = "true"' "git ran, so the status capture is true"

step "the bootstrap policy was adopted before the first action, not after it"

disabled=$HOME/.config/batfiles/disabled.toml
is_file "$disabled" "the adoption wrote no document under the XDG default"
[ "$(cat "$disabled")" = 'actions = ["batgrep"]
groups = ["editor"]' ] || fail "the document says: $(cat "$disabled")"
ok

reported "$log" 'default-disabled: disabled action `batgrep`'
reported "$log" 'default-disabled: disabled group `editor`'

# What the document switched off was never installed, which is the half only a
# run on a machine with no state of its own can show.
absent "$HOME/.local" "a disabled action creates none of its parents either"
absent "$HOME/.config/nvim" "the editor group was disabled"

# What it did not switch off was installed: the gitconfig candidate is one this
# machine was never offered, and the editor group reaches no record the
# inclusion contributed, since those are addressed under the inclusion.
lacks "$disabled" "gitconfig" "the candidate's condition closed, so it was passed over"

step "sync: a second run over the machine the clone set up"

printf '\n; edited on this machine\n' >>"$HOME/.config/git/local"

# Nothing names the repository: no BATFILES_DIR, and a working directory holding
# no manifest, so the default under the selected home is what answers. The
# verbosity is for the seed that reports keeping what it found.
sync_log=$(mktemp)
run "$sync_log" sync -v

contains "$sync_log" "kept $HOME/.config/git/local" \
    "the second run replaced a seed instead of keeping it"
contains "$HOME/.config/git/local" "edited on this machine" \
    "the second run overwrote an edit made on this machine"

# The machine's own state outlives the bootstrap that wrote it.
absent "$HOME/.local" "the disabled action ran on the second pass"
absent "$HOME/.config/nvim" "the disabled group ran on the second pass"
links_to "$HOME/.zshrc" "$repo/shell/zshrc" "the second run disturbed a link"

echo "pristine-machine acceptance: $checks checks passed"
