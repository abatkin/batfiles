#!/bin/sh
# The batfiles hosted installer. A placeholder until the installer is built:
# it names where the release's binaries are, and installs nothing.
#
# `dist/assemble.sh` stamps the release base into the line below; this unstamped
# source refuses to run.
batfiles_stamped_base=unstamped

main() {
    if [ "$batfiles_stamped_base" = unstamped ]; then
        echo "install.sh: this copy was never stamped with a release base; use the one published with a release" >&2
        exit 1
    fi
    base=${BATFILES_BASE:-$batfiles_stamped_base}
    echo "install.sh: this release has no installer yet; download batfiles-<target> from $base/latest/download/" >&2
    exit 1
}

main "$@"
