# Distribution

How releases are built, published, installed, and self-hosted.

| Piece | Lives | Changes | Job |
| --- | --- | --- | --- |
| [Release tree](#release-tree) | A release base URL | Every release | Binaries, checksums, installers, `VERSION` |
| [Hosted installer](../installer.md#hosted-installer) | `install.sh` and `install.ps1` in the release tree | Every release | Put a verified binary at its install location, then optionally run it |
| [Pages site](#github-pages) | Landing page, generated docs, and installers at GitHub Pages | Documentation changes and stable releases | Publish user docs and the installer entry points |
| [Leaf stub](../installer.md#leaf-stub) | `install.sh` and `install.ps1` in a leaf repository | Frozen | Find or fetch `batfiles`, then `sync` its own checkout |
| [`batfiles update`](../commands/update.md#update) | The binary | Every release | Replace the running binary with another release |

All logic that follows releases — platform detection, asset names, checksum
verification — lives in the hosted installer and the binary. The stub only
locates things, so the one way it can go stale is a change to the contracts in
[stub stability](../installer.md#stability), which are frozen.

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

A version is [SemVer](https://semver.org) without build metadata:

- `X`, `Y`, and `Z` are numbers without leading zeros.
- A pre-release is one or more identifiers separated by dots. Each is a number
  without leading zeros, or letters, digits, and hyphens with at least one
  letter or hyphen. `rc.2`, `beta`, and `alpha.1.x-2` are pre-releases; `rc..2`,
  `rc.02`, and `rc_2` are not.
- Nothing follows with `+`.

Versions order by SemVer precedence: `X`, `Y`, and `Z` numerically; a
pre-release below the release it precedes; and pre-release identifiers in turn,
numbers numerically and below any non-number, non-numbers as ASCII strings, and
a shorter list below a longer one it begins. So `1.2.3-rc.2` precedes
`1.2.3-rc.10`, which precedes `1.2.3`. Numbers compare exactly, at any length.

Every release script and the installer accept exactly this grammar, from one
pattern in `dist/version.sh` that `install.sh` repeats, so nothing is published
that the installer cannot order. A release build compiles its version in as
`BATFILES_RELEASE_VERSION`, so [`batfiles version`](../commands/version.md#version)
reports exactly the release, suffix included. `latest` never names a
pre-release.

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

### Targets

- `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`: static, so one
  binary runs on any Linux distribution, whatever its C library. The TLS stack
  is rustls, so there is no OpenSSL to link.
- `x86_64-apple-darwin`, `aarch64-apple-darwin`.
- `x86_64-pc-windows-msvc`, with the C runtime linked statically. Windows on ARM
  runs it under emulation.

A build compiles in the target triple it is built for, as `BATFILES_TARGET`,
and [`update`](../commands/update.md#update) downloads that target's asset. A build for a
target no release publishes takes its platform's: a Linux build of either
architecture the musl asset of that architecture, whatever its C library, and
a Windows build of either architecture the x86_64 one. A build for any other
platform, such as FreeBSD, has no asset, and `update` fails naming its target.

## The release base

The release base is the one parameter a fork or self-hoster changes:

- **Release workflow.** The repository variable `BATFILES_BASE`, defaulting to
  `https://github.com/<owner>/<repo>/releases` for the repository the workflow
  runs in. A fork's releases point at the fork with no configuration.
- **Installers.** Each holds its default base on one line of a fixed form:
  `batfiles_stamped_base='<base>'` in `install.sh`, and
  `$BatfilesStampedBase = '<base>'` in `install.ps1`. Assembling a release
  replaces that line. The unstamped source in `dist/` holds the value
  `unstamped`, and runs only when `BATFILES_BASE` supplies a base.
- **Binaries.** `dist:binary` compiles the base in as `BATFILES_DEFAULT_BASE`;
  a build without it takes the official base. At run time `BATFILES_BASE`
  overrides it, for the [stub](../installer.md#leaf-stub) `init` writes and for
  [`update`](../commands/update.md#update); see [environment](../environment.md#release-base).

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
| `dist:tag TAG=v<version>` | Refuses a tag that is not a [version](#versions) the `Cargo.toml` version allows, and a stable one whose commit is not on `origin/main` or whose published documentation still carries an [Unreleased label](authoring.md#links-and-examples). Prints `version=` and `prerelease=` lines. |
| `dist:binary TARGET=<triple> [BASE=<url>] [CROSS=xwin] [VERSION=<version>]` | Builds the `dist` profile for one target, reporting `VERSION` (by default the `Cargo.toml` version), and stages it as `target/assets/batfiles-<triple>[.exe]`. `CROSS=xwin` builds a Windows target with `cargo xwin`. |
| `dist:assemble VERSION=<version> BASE=<url> OUT=<dir> [IN=<dir>] [TARGETS="<triple> ..."]` | Writes one release's complete asset set from the staged binaries. |
| `dist:mirror VERSION=<version>\|latest BASE=<url> OUT=<dir> [FROM=<base>]` | Copies a published release for [another base](#self-hosting). |
| `dist:pages OUT=<dir> [FROM=<base>]` | Builds the [Pages site](#the-site) from `site/` and the latest stable release's installers. |
| `dist:publish VERSION=<version> DIR=<dir>` | Publishes an assembled set as the GitHub release for the existing tag. |
| `dist:verify URL=<base> VERSION=<version> LATEST=yes\|no [STAMPED=<base>] [PAGES=<url>]` | Checks a published release through its release tree, and a Pages site's copies of its installers. |
| `dist:smoke URL=<base> VERSION=<version> LATEST=yes\|no` | Installs a published release on this machine with its own one-liner, and checks what it installed and, for the latest, what its `update --check` finds. |

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

`dist:smoke` pipes `<base>/download/v<version>/install.sh` into `sh` with that
version requested, installing to a scratch path that leaves any batfiles on
the machine alone, and requires the result to report the version. With
`LATEST=yes` it does the same through `<base>/latest/download/install.sh` with
no version requested, and then requires that binary's `update --check`, with
`BATFILES_BASE` set to `URL`, to report the version available. Under Git Bash on
Windows it runs `install.ps1` with `pwsh` instead: the scriptblock one-liner for
the requested version, and `irm | iex` for the latest.

`dist:verify` fetches `<base>/download/v<version>/`: `VERSION` must hold the
version, every binary `SHA256SUMS` lists must match it, and both installers must
carry the stamped base, `STAMPED` or else `URL`. With `LATEST=yes`,
`<base>/latest/download/` must serve the same `VERSION` and `SHA256SUMS`, and
`PAGES`, when given, must serve at its root the same `install.sh` and
`install.ps1` as that directory, byte for byte. Pages caches what it serves for
up to ten minutes, so that check retries for `PAGES_WAIT` seconds, by default
600, before failing.

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
6. `dist:smoke` installs it with its own one-liner on Linux x86_64 and aarch64,
   macOS, and Windows runners, again including `latest` for a stable release,
   where the installed binary's `update --check` must find it.
7. For a stable release, after `verify`, the [Pages
   workflow](#the-pages-workflow) replaces the Pages site's copies of the
   installers.

A failure before publishing leaves no release, or at most a draft to delete.

## Self-hosting

A self-hoster serves a release tree from any static host:

1. `task dist:mirror VERSION=<version> BASE=<url> OUT=<dir>` downloads a
   release from `FROM`, by default the official base, verifies every binary
   against its `SHA256SUMS`, and writes it to `<dir>` with both installers'
   [stamp lines](#the-release-base) restamped with `<url>`. `VERSION=latest`
   mirrors the latest release. The task runs `dist/mirror.sh`, which a mirror
   can run without Task.
2. Upload `<dir>` to `<url>/download/v<version>/`, and for the latest release
   also to `<url>/latest/download/`. Both are needed: the installer reads only
   `VERSION` from `latest/download/` and downloads the rest from the release's
   own directory. `task dist:verify URL=<url>
   VERSION=<version> LATEST=yes` checks the result.

After that, `<url>` works like the official base for the installer one-liner.

## GitHub Pages

When a repository's Pages site is built by GitHub Actions, it serves copies of
the latest stable release's hosted installers at its root, for a shorter
one-liner such as the [README's](https://github.com/abatkin/batfiles/blob/main/README.md#installing). The official site is `https://batfiles.dev`, the custom domain of this
repository's Pages site. A fork's site is at its own Pages address, or its own
domain; nothing in the build names a domain.

The copies are the release's own installers, byte for byte. Still stamped with
the [release base](#the-release-base), they read `VERSION` and download
binaries from the release tree as before; the site serves only the entry point.
A repository without such a site loses nothing: the release URL one-liner works
everywhere.

### The site

`site/` holds the landing page and styles. `docs/` holds the Markdown book,
whose navigation is [SUMMARY.md](../SUMMARY.md); [book.toml](../../book.toml) configures
mdBook, with [theme/docs.css](../../theme/docs.css) for its styles. mdBook and the
lychee link checker are pinned in `dist/docs-tools`. See
[authoring](authoring.md#building-and-previewing) for setup and local preview.

`task dist:pages OUT=<dir> [FROM=<base>] [SITE_URL=/]` builds the complete site:

1. Build the documentation and check source and generated links, without
   contacting external sites. The docs are written under `target/docs`.
2. Read `<from>/latest/download/VERSION` once, so a release published meanwhile
   cannot mix two. `FROM` defaults to the official release base.
3. Fetch both installers from `<from>/download/v<version>/`.
4. Stage `site/`, the generated book under `docs/`, and the installers together.
   Check the assembled site's local links, heading fragments, and stylesheet and
   image references as served under `SITE_URL`, including links from the landing
   page into the book.
5. Unless `site/` has its own `404.html`, copy the book's to the site root,
   which is the only 404 page Pages serves. Its links resolve through the
   `<base>` it declares, so they hold anywhere in the site.
6. Publish the complete directory at `<dir>`, which must be absent or empty,
   and print `version=<version>` on standard output. Build diagnostics go to
   standard error.

`SITE_URL` is the site's URL path with leading and trailing slashes: `/` for a
custom domain, or `/batfiles/` for a project site. The book uses that prefix
followed by `docs/`. Relative navigation works at either location.

The assembler, `dist/pages.sh`, takes `<out> [<from>] [<site>] [<docs>]
[<site-url>]`; the last three default to the repository's `site/`, `target/docs`,
and `/`. It requires previously built documentation and the pinned lychee.
Tests pass small generated page fixtures and local release trees; `task test`
installs the pinned tools first on Linux and macOS, where those tests run.
`task ci` also builds and checks the real book through `docs:check`.

Any build, download, or validation failure leaves `<dir>` absent or empty.
The build refuses missing documentation, a missing stable release, a `latest`
that names a pre-release, and `site/` entries named `docs`, `install.sh`, or
`install.ps1` that would collide with generated or downloaded content. Generated
HTML is not committed. Installers are always copied from the stable release,
byte for byte, independent of the documentation revision.

### The Pages workflow

`.github/workflows/pages.yml` builds and deploys the site. It runs:

- on a push to `main` changing documentation, site assets, the pinned tools or
  their configuration, or the build and verification scripts listed in the
  workflow;
- on demand, from the Actions tab; and
- from the [release workflow](#the-release-workflow), after `verify` passes for
  a stable release.

Its first job reads the repository's Pages configuration and public URL. Unless
Pages uses *GitHub Actions* as its source, the workflow succeeds without deploying.
The URL's path becomes `SITE_URL`, so custom domains and project sites use the
same build commands.

The deployment job is serialized and never cancels one already running. After
acquiring that slot, it checks out current `main`, even when called by a release
tag, installs the pinned tools, and builds and validates that one revision.
An older release invocation therefore cannot publish its older documentation
over a newer main revision. The book follows main; unreleased behavior is
labeled according to [authoring policy](authoring.md).

The job runs `dist:pages` with `FROM` set to the repository's releases URL,
uploads and deploys the complete artifact, then runs `dist:verify` with `PAGES`
set to the deployed URL. Each run deploys under a build version of its own,
made from the commit, run ID, and attempt: Pages treats a build version it has
already deployed as done, so reusing the commit alone would let a push's
deployment stand in for a later release or manual run of the same commit, with
older installers. Verification checks the installer bytes and that the
documentation landing page and first-repository tutorial can be fetched and are
nonempty. The Pages checks retry while caches settle.

Pull requests run the same documentation checks through `task ci` and do not
deploy. The repository settings are in
[release management](../../dist/README.md#repository-setup).
