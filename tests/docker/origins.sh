#!/usr/bin/env bash
# Build the two bare repositories the scenario clones, under /srv, so that the
# pristine home never holds a fixture. Run when the container starts rather than
# when the image is built, for the reason the Dockerfile gives.
#
# `personal` is the committed `leaf` fixture with `overlay.toml` appended to its
# manifest; `corporate` is the committed `corporate` fixture unchanged. The git
# invocations mirror `tests/cli/support/git.rs`: only the repository's own
# config and the identity given here, so the commits are the same on every
# machine.
set -euo pipefail

fixtures=/usr/local/share/batfiles-fixtures
overlay=/usr/local/share/overlay.toml
origins=/srv

export GIT_CONFIG_NOSYSTEM=1
export GIT_CONFIG_GLOBAL=/dev/null

git_fixture() {
    local dir=$1
    shift
    git -C "$dir" \
        -c user.name='batfiles tests' \
        -c user.email='tests@example.invalid' \
        -c commit.gpgsign=false \
        -c init.templateDir= \
        "$@"
}

# One bare repository holding one tree, on `main`, in a single commit.
publish() {
    local name=$1 tree=$2 work
    git_fixture "$origins" init --quiet --bare -b main "$name.git"
    work=$(mktemp -d)
    cp -R "$tree/." "$work/"
    git_fixture "$work" init --quiet -b main
    git_fixture "$work" add -A
    git_fixture "$work" commit --quiet -m "the $name repository"
    git_fixture "$work" push --quiet "$origins/$name.git" main
    rm -rf "$work"
}

publish corporate "$fixtures/corporate"

staged=$(mktemp -d)
cp -R "$fixtures/leaf/." "$staged/"
cat "$overlay" >>"$staged/batfiles.toml"
publish personal "$staged"
rm -rf "$staged"
