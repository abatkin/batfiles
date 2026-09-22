#!/usr/bin/env bash
# Host side of the pristine-machine test: build the image and run the scenario
# in it. Invoked by `task test:docker`, which builds the binary this prefers.
#
# A machine with no working container runtime is not a failure: the test says it
# did not run and exits 0, so `task ci` stays usable where CI's container
# runtime is not. CI has one, so the scenario really runs there.
set -euo pipefail

here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
binary=${CARGO_TARGET_DIR:-$root/target}/debug/batfiles
image=batfiles-pristine
base=${BATFILES_DOCKER_BASE:-docker.io/library/fedora:latest}

# Whether the host's own binary is one the image could execute, read from the
# four bytes that say so rather than from a tool that may not be installed.
is_linux_binary() {
    [ -r "$1" ] && [ "$(od -An -N4 -tx1 "$1" | tr -d ' \n')" = "7f454c46" ]
}

# The CLI has to be there and its daemon has to answer; a client with nothing
# behind it is the same as no client at all.
runtime=
for candidate in docker podman; do
    if command -v "$candidate" >/dev/null 2>&1 &&
        "$candidate" info >/dev/null 2>&1; then
        runtime=$candidate
        break
    fi
done
if [ -z "$runtime" ]; then
    echo "test:docker: no working docker or podman; the pristine-machine test did not run" >&2
    exit 0
fi

# Where the image's binary comes from. `task build` produces a Mach-O executable
# on macOS and a PE one on Windows, neither of which a Linux container can run,
# so those hosts build batfiles inside the image instead of failing on its
# format. BATFILES_DOCKER_BINARY forces either choice.
binary_source=${BATFILES_DOCKER_BINARY:-}
if [ -z "$binary_source" ]; then
    if is_linux_binary "$binary"; then
        binary_source=host
    else
        binary_source=built
    fi
fi

if [ "$binary_source" = host ] && [ ! -x "$binary" ]; then
    echo "test:docker: $binary is missing; run 'task build' first" >&2
    exit 1
fi

# The context is staged rather than taken from the repository, so that the whole
# of target/ is not a build context. It holds the image's own files, the two
# fixture trees the origins are built from, and both possible binaries: the
# host's, and the sources the image would compile instead.
context=$(mktemp -d)
trap 'rm -rf "$context"' EXIT

install -m 0644 "$here/Dockerfile" "$here/overlay.toml" "$context/"
install -m 0755 "$here/origins.sh" "$here/scenario.sh" "$context/"
mkdir "$context/fixtures"
cp -R "$root/tests/fixtures/leaf" "$root/tests/fixtures/corporate" "$context/fixtures/"
mkdir "$context/source"
cp -R "$root/src" "$context/source/src"
install -m 0644 "$root/Cargo.toml" "$root/Cargo.lock" "$root/rust-toolchain.toml" \
    "$context/source/"
if [ -e "$binary" ]; then
    install -m 0755 "$binary" "$context/batfiles"
else
    : >"$context/batfiles"
fi

case $binary_source in
host) echo "test:docker: $runtime, from $base, with a binary built on this machine" ;;
built) echo "test:docker: $runtime, from $base, building batfiles in the image" ;;
esac

"$runtime" build \
    --build-arg "BASE=$base" \
    --build-arg "BINARY=$binary_source" \
    --tag "$image" "$context"

if ! "$runtime" run --rm "$image"; then
    # A binary from this machine also has to load on the image's older libraries,
    # which is the one failure whose cause is not in the scenario's own output.
    if [ "$binary_source" = host ]; then
        echo "test:docker: the scenario failed under $base." >&2
        echo "test:docker: if the binary would not load, the image is older than this" >&2
        echo "test:docker: machine - set BATFILES_DOCKER_BASE to an image at least as new." >&2
    fi
    exit 1
fi
