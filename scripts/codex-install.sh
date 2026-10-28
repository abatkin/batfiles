#!/bin/sh
# Prepare an Ubuntu x86_64 Codex Cloud VM; run from any working directory.
# Requires root or passwordless sudo and network access. Installs system tools.
set -eu

if [ "$(uname -s)/$(uname -m)" != Linux/x86_64 ]; then
    echo "codex-install: requires Ubuntu on x86_64" >&2
    exit 1
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

as_root() {
    if [ "$(id -u)" = 0 ]; then
        "$@"
    else
        sudo -n "$@"
    fi
}

as_root apt-get update
as_root env DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    build-essential ca-certificates clang cmake curl git libssl-dev lld \
    pkg-config unzip wget xz-utils

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

if ! command -v rustup >/dev/null 2>&1; then
    curl -fsSL --proto '=https' --retry 3 https://sh.rustup.rs -o "$work/rustup.sh"
    sh "$work/rustup.sh" -y --profile minimal --default-toolchain none --no-modify-path
fi
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:/usr/local/bin:$PATH"
# Make the rustup proxies available to later shells, too.
for tool in rustup cargo rustc rustfmt cargo-fmt cargo-clippy clippy-driver; do
    tool_path=$(command -v "$tool")
    if [ "$tool_path" != "/usr/local/bin/$tool" ]; then
        as_root ln -sf "$tool_path" "/usr/local/bin/$tool"
    fi
done
rustup toolchain install

if ! task --version 2>/dev/null | grep -q '3\.53\.1$'; then
    curl -fsSL --proto '=https' --retry 3 -o "$work/task.tar.gz" \
        https://github.com/go-task/task/releases/download/v3.53.1/task_linux_amd64.tar.gz
    echo "a54a408f6861ff921f6e87774180db31bacd8c1e7c944ca696db9fea49a82fc7  $work/task.tar.gz" | sha256sum -c
    tar -xzf "$work/task.tar.gz" -C "$work" task
    as_root install -m 0755 "$work/task" /usr/local/bin/task
fi

if ! shellcheck --version 2>/dev/null | grep -q '^version: 0\.11\.0$'; then
    curl -fsSL --proto '=https' --retry 3 -o "$work/shellcheck.tar.xz" \
        https://github.com/koalaman/shellcheck/releases/download/v0.11.0/shellcheck-v0.11.0.linux.x86_64.tar.xz
    echo "8c3be12b05d5c177a04c29e3c78ce89ac86f1595681cab149b65b97c4e227198  $work/shellcheck.tar.xz" | sha256sum -c
    tar -xJf "$work/shellcheck.tar.xz" -C "$work"
    as_root install -m 0755 "$work/shellcheck-v0.11.0/shellcheck" /usr/local/bin/shellcheck
fi

cargo install cargo-deny --version 0.19.9 --locked
cargo install cargo-xwin --version 0.23.1 --locked
for tool in cargo-deny cargo-xwin; do
    as_root install -m 0755 "${CARGO_HOME:-$HOME/.cargo}/bin/$tool" "/usr/local/bin/$tool"
done

if command -v pwsh >/dev/null 2>&1; then
    # PowerShell expands its variables inside this argument.
    # shellcheck disable=SC2016
    pwsh -NoLogo -NoProfile -NonInteractive -Command \
        '$ErrorActionPreference = "Stop"; if (-not (Get-Module -ListAvailable PSScriptAnalyzer)) { Install-Module PSScriptAnalyzer -Scope CurrentUser -Force -Repository PSGallery }; Import-Module PSScriptAnalyzer'
fi

cargo fetch --locked
task docs:install
# Populate the Windows SDK and advisory caches using the normal checks.
task lint
task deny
echo "codex-install: ready; run task ci before publishing the environment"
