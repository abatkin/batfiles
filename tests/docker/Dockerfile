# A machine that has never seen batfiles: no repository, no `disabled.toml`,
# no `~/.config` or `~/.local` at all, and none of the four `BATFILES_*`
# variables every test under `tests/cli/` sets. What the scenario proves is what
# batfiles decides when nothing tells it anything.
ARG BASE=docker.io/library/fedora:latest
ARG RUST=docker.io/library/rust:slim
ARG BINARY=host

# The image needs a Linux executable, and where it comes from depends on the
# machine running the test. A host that builds one hands it over, which is the
# fast path and the one CI takes; a macOS or Windows host builds nothing this
# image can run, so the binary is built here instead, against the toolchain
# `rust-toolchain.toml` pins. `run.sh` chooses, and only the stage it chooses is
# copied from.
FROM ${RUST} AS batfiles-built
WORKDIR /src
# The manifests alone first, so that editing batfiles does not fetch the
# dependency tree again. Cargo needs a target to read them at all.
COPY source/Cargo.toml source/Cargo.lock source/rust-toolchain.toml ./
RUN mkdir src && echo 'fn main() {}' >src/main.rs && cargo fetch --locked
COPY source/src ./src
RUN cargo build --locked && install -m 0755 target/debug/batfiles /batfiles

# The other source: whatever `task build` left on the host.
FROM ${BASE} AS batfiles-host
COPY batfiles /batfiles

FROM batfiles-${BINARY} AS binary

FROM ${BASE}

# Git is the one thing a leaf repository needs the machine to already have.
# dash is what the hosted installer runs under, as the strictest common sh.
RUN dnf -y install git-core dash && dnf clean all

# The user the scenario runs as. Nothing below writes to its home: what batfiles
# finds there is the image's own skeleton and nothing else.
RUN useradd --create-home dev

# What the repositories the scenario clones are built from. They are built when
# the container starts rather than here, because `git clone` of a local
# repository hardlinks its objects, and a hardlink to a file in an image layer
# fails the check git makes of it under podman's storage. Built at start, they
# live in the container's own layer, where a local clone behaves as it does on a
# machine.
COPY fixtures /usr/local/share/batfiles-fixtures
COPY overlay.toml /usr/local/share/
COPY origins.sh /usr/local/bin/origins
RUN chown dev:dev /srv

# No batfiles on PATH: the machine gets one from a release tree under /srv,
# assembled by the script a real release is, with the binary named as the
# installer asks for it on this architecture. It is a glibc build rather than a
# static one, which this image runs either way.
COPY dist /usr/local/share/batfiles-dist
COPY --from=binary /batfiles /tmp/batfiles
RUN set -eu; \
    version=$(/tmp/batfiles version); version=${version#batfiles }; \
    mkdir /tmp/assets; \
    mv /tmp/batfiles "/tmp/assets/batfiles-$(uname -m)-unknown-linux-musl"; \
    sh /usr/local/share/batfiles-dist/assemble.sh \
        "$version" file:///srv/releases /tmp/release /tmp/assets; \
    mkdir -p /srv/releases/download /srv/releases/latest; \
    cp -R /tmp/release "/srv/releases/download/v$version"; \
    cp -R /tmp/release /srv/releases/latest/download; \
    rm -rf /tmp/release /tmp/assets

COPY scenario.sh /usr/local/bin/scenario

USER dev
WORKDIR /home/dev
# The two halves of a run: the repositories to clone, and the scenario that
# clones them. Keeping them apart keeps the scenario about assertions alone.
CMD ["sh", "-c", "origins && exec scenario"]
