# Distribution

How a batfiles release is laid out, built, and published. The installers that
read a release, the stub a leaf repository carries, and `batfiles update` are
[proposed](future/distribution.md) and not yet built.

## Release tree

A release tree is a static directory tree under one **release base URL**, laid
out the way GitHub Releases serves assets:

```text
<base>/latest/download/<asset>        the latest stable release
<base>/download/v<version>/<asset>    one release, by tag
```

The official base is `https://github.com/abatkin/batfiles/releases`. Any static
host that serves those two paths is a release tree; no server logic or API is
involved.

### Versions

`Cargo.toml` holds `X.Y.Z`, the release being worked toward. A release is tagged
`v<version>`, and its version is one of:

- `X.Y.Z`, a stable release, from a commit on `main`; or
- `X.Y.Z-<pre-release>`, a pre-release of it, such as `X.Y.Z-rc.2`, from any
  commit.

A release build compiles its version in as `BATFILES_RELEASE_VERSION`, so
[`batfiles version`](cmdline.md#version) reports exactly the release, suffix
included. `latest` never names a pre-release.

Every release carries these assets, with version-free names:

| Asset | Contents |
| --- | --- |
| `batfiles-<target>` / `batfiles-<target>.exe` | The bare binary for one Rust target triple. |
| `SHA256SUMS` | `sha256sum` format (`<hex>  <asset>`), covering the binaries only. |
| `VERSION` | The release's version and a newline. |
| `install.sh`, `install.ps1` | The hosted installers, [stamped](#the-release-base) with the release base. |

Binaries are bare rather than archived, so nothing that fetches one needs to
unpack it. `SHA256SUMS` leaves the installers out so that restamping them does
not invalidate it. Checksums detect corruption, not a compromised host. The
official workflow also publishes a build-provenance attestation for each binary,
which `gh attestation verify` checks; nothing depends on it.

Until the [hosted installers](future/distribution.md#hosted-installer) are
built, `install.sh` and `install.ps1` are placeholders that install nothing:
each names its base's `latest/download/` and exits 1.

### Targets

- `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`: static, so one
  binary runs on any Linux distribution, whatever its C library. The TLS stack
  is rustls, so there is no OpenSSL to link.
- `x86_64-apple-darwin`, `aarch64-apple-darwin`.
- `x86_64-pc-windows-msvc`, with the C runtime linked statically. Windows on ARM
  runs it under emulation.

## The release base

The release base is the one parameter a fork or self-hoster changes:

- **Release workflow.** The repository variable `BATFILES_BASE`, defaulting to
  `https://github.com/<owner>/<repo>/releases` for the repository the workflow
  runs in. A fork's releases point at the fork with no configuration.
- **Installers.** Each holds its default base on one line of a fixed form:
  `batfiles_stamped_base='<base>'` in `install.sh`, and
  `$BatfilesStampedBase = '<base>'` in `install.ps1`. Assembling a release
  replaces that line. The unstamped source in `dist/` holds the value
  `unstamped`, and fails saying it was never stamped.
- **Binaries.** `dist:binary` exports the base to the build as
  `BATFILES_DEFAULT_BASE`.

A base is a URL with no trailing slash (one given is dropped), made of
characters the installers quote safely: letters, digits, and
`._~:/@%+=,;!*()-`.

## Building a release

`Taskfile.yml` owns every step, backed by POSIX scripts in `dist/`, so the
workflow runs nothing that cannot be run locally:

| Task | Does |
| --- | --- |
| `release:rc` | Tags the checked-out commit, locally, as the next release candidate: `vX.Y.Z-rc.<n>`, one past every rc tag of `X.Y.Z` here or on `origin`. |
| `release:stable` | Tags the checked-out commit, locally, as `vX.Y.Z`. |
| `dist:tag TAG=v<version>` | Refuses a tag that is not a [version](#versions) the `Cargo.toml` version allows, and a stable one whose commit is not on `origin/main`. Prints `version=` and `prerelease=` lines. |
| `dist:binary TARGET=<triple> [BASE=<url>] [CROSS=xwin] [VERSION=<version>]` | Builds the `dist` profile for one target, reporting `VERSION` (by default the `Cargo.toml` version), and stages it as `target/assets/batfiles-<triple>[.exe]`. `CROSS=xwin` builds a Windows target with `cargo xwin`. |
| `dist:assemble VERSION=<version> BASE=<url> OUT=<dir> [IN=<dir>] [TARGETS="<triple> ..."]` | Writes one release's complete asset set from the staged binaries. |
| `dist:publish VERSION=<version> DIR=<dir>` | Publishes an assembled set as the GitHub release for the existing tag. |
| `dist:verify URL=<base> VERSION=<version> LATEST=yes\|no [STAMPED=<base>]` | Checks a published release through its release tree. |

Both `release:` tasks refuse uncommitted changes and a version whose stable
release is already tagged, and check the tag as `dist:tag` does; pushing the
tag is what releases it. A repository with immutable releases can never
reuse a published tag, so a failed release candidate is followed by the next
number rather than retried.

`dist:binary` refuses a Linux binary that links any shared library and a
Windows binary that imports the Visual C++ runtime. When the host can execute
the binary, it runs `version`, which must report `VERSION`. A
native musl build uses `musl-gcc`, from the distribution's `musl-tools` or
`musl-gcc` package.

`dist:assemble` takes every `batfiles-<target>` in `IN` (by default, where
`dist:binary` stages them), so the host's binary alone makes a release tree.
`TARGETS` requires exactly that set. It writes `VERSION` and `SHA256SUMS`,
stamps `BASE` into both installers, and refuses an invalid version or base, a
Windows binary without `.exe` or another with it, and an `OUT` that is not
empty. Nothing appears at `OUT` until the set is complete.

`dist:publish` creates the release as a draft, uploads every asset, and only
then publishes it, so `latest/download/` never serves part of a release. A
pre-release is published as a pre-release and not marked latest. A tag that
already has a release is refused.

`dist:verify` fetches `<base>/download/v<version>/`: `VERSION` must hold the
version, every binary `SHA256SUMS` lists must match it, and both installers must
carry the stamped base, `STAMPED` or else `URL`. With `LATEST=yes`,
`<base>/latest/download/` must serve the same `VERSION` and `SHA256SUMS`.

## The release workflow

`.github/workflows/release.yml` runs on a pushed `v*` tag:

1. `dist:tag` checks the tag. The job lists the targets, each with the runner
   that builds it; adding a target is one line there, plus any toolchain setup
   its runner needs.
2. `dist:binary` runs once per target on its runner: Linux targets on Ubuntu
   runners of their architecture with `musl-tools`, macOS targets on a macOS
   runner, and Windows on a Windows runner.
3. `dist:assemble` collects the binaries on Linux, requiring the whole target
   list.
4. After `task ci` passes for the tagged commit, a job in the `release`
   environment attests each binary and runs `dist:publish`. The repository's
   settings for that environment can restrict it to `v*` tags and require an
   approval.
5. `dist:verify` checks the published release at this repository's releases
   URL, including `latest` for a stable release.

A failure before publishing leaves no release, or at most a draft to delete.
