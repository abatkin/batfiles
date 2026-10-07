# shellcheck shell=sh
# Sourced by the release scripts: the version grammar every release follows,
# which docs/contributing/distribution.md#versions specifies. install.sh, which runs on its
# own, carries an identical `version_pattern` line; tests/dist checks they agree.

# SemVer without build metadata: X.Y.Z, optionally followed by a pre-release of
# dot-separated identifiers, each a number without leading zeros or a mix of
# letters, digits, and hyphens.
version_pattern='(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9A-Za-z-]*[A-Za-z-][0-9A-Za-z-]*))*)?'

# Whether $1 is a version.
is_version() {
    printf '%s\n' "$1" | grep -Eqx "$version_pattern"
}
