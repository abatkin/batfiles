# Distribution

How releases are built, published, installed, and self-hosted.

| Piece | Lives | Changes | Job |
| --- | --- | --- | --- |
| [Release tree](#release-tree) | A release base URL | Every release | Binaries, checksums, installers, `VERSION` |
| [Hosted installer](#hosted-installer) | `install.sh` and `install.ps1` in the release tree | Every release | Put a verified binary at its install location, then optionally run it |
| [Pages site](#github-pages) | `site/` and the installers, at the repository's GitHub Pages site | Every stable release | Serve the hosted installers at a shorter URL |
| [Leaf stub](#leaf-stub) | `install.sh` and `install.ps1` in a leaf repository | Frozen | Find or fetch `batfiles`, then `sync` its own checkout |
| [`batfiles update`](cmdline.md#update) | The binary | Every release | Replace the running binary with another release |

All logic that follows releases — platform detection, asset names, checksum
verification — lives in the hosted installer and the binary. The stub only
locates things, so the one way it can go stale is a change to the contracts in
[stub stability](#stability), which are frozen.

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

A version is [SemVer](https://semver.org/) without build metadata:

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
`BATFILES_RELEASE_VERSION`, so [`batfiles version`](cmdline.md#version)
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
and [`update`](cmdline.md#update) downloads that target's asset. A build for a
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
  overrides it, for the [stub](#leaf-stub) `init` writes and for
  [`update`](cmdline.md#update); see [environment](environment.md#release-base).

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

## Hosted installer

### Invocation

```sh
curl -fsSL <base>/latest/download/install.sh | sh
curl -fsSL <base>/latest/download/install.sh | sh -s -- clone https://github.com/me/dotfiles
```

The official installers are also served from `https://batfiles.dev/`; see
[GitHub Pages](#github-pages).

With no arguments it ensures a binary and exits. With arguments it ensures a
binary and then `exec`s it with those arguments, which makes the second form
the one-command bootstrap of a new machine: [`clone`](cmdline.md#clone) does
the rest. When its standard input is not a terminal, as when it is piped into
`sh`, and the process has a controlling terminal, batfiles gets `/dev/tty` as
standard input, so a command such as `clone --interactive` can still ask.

Inputs, all optional:

| Variable | Meaning |
| --- | --- |
| `BATFILES_BASE` | The release base. Defaults to the stamped base. |
| `BATFILES_VERSION` | A release to fetch, a [version](#versions) with or without a leading `v`. Empty or `latest` means the latest release. |
| `BATFILES_BIN` | The path of the one batfiles to use, or to install. A relative path is taken from the working directory. Unset, the installer searches as [resolving a binary](#resolving-a-binary) describes. |

Because the script is piped, a variable reaches it as `curl … | BATFILES_BASE=…
sh`. `latest` never names a pre-release, so installing one takes
`BATFILES_VERSION`.

### Resolving a binary

With `BATFILES_BIN` set, it is the only candidate, and where a download goes.
Otherwise the candidates, in order, are the `batfiles` found on `PATH` and
`$HOME/.local/bin/batfiles`, and a download goes to the second. Either way, the
last candidate is the **install location**.

- **No version requested:** the first candidate that runs is used, and nothing
  is downloaded. Upgrading an existing binary is what
  [`update`](cmdline.md#update) is for; an installer run
  with no arguments reports which binary it found and suggests
  `batfiles update`.
- **`BATFILES_VERSION` set:** it names exactly what is downloaded, and is a
  floor for what is accepted. The first candidate whose version is at least the
  requested one is used. If none is, the requested release is installed at the
  install location, replacing an older batfiles there, and used by its
  absolute path. A candidate on `PATH` that was passed over is named in a
  warning and never modified.

Because the pin is a floor, a repository that needs 1.4 accepts 1.5 without
downgrading it, while a machine holding 1.2 gets 1.4.

A candidate's version is read from `batfiles version`, whose output (`batfiles
<version>`) is a [frozen contract](#stability), and
versions compare as [versions](#versions) order. A candidate that fails to run,
prints something else, or reports a version outside the grammar is passed over,
and the installer says so.

### Installing

The installer only ever writes in the directory holding the install location,
creating it if needed. It never replaces something there that is not batfiles:
a file at the install location that does not report a batfiles version, or a
directory, stops the installer before it downloads anything, naming the path.
Otherwise:

1. Map `uname -s` and `uname -m` to a target: `Linux` to musl, `Darwin` to
   Apple; `x86_64`/`amd64` and `aarch64`/`arm64`. An x86_64 shell on Apple
   silicon (Rosetta, reported by `sysctl hw.optional.arm64`) gets the aarch64
   binary. An unmapped platform fails, naming what was detected.
2. Choose the release: the requested version, or else the one
   `<base>/latest/download/VERSION` names, read once so that a release
   published meanwhile cannot mix two. When that cannot be read, the installer
   suggests requesting a pre-release, since the base may have no stable release
   yet. Everything else comes from `<base>/download/v<version>/`.
3. Download the binary to a temporary file beside the install location, and
   `SHA256SUMS` beside it, with `curl`, falling back to `wget`.
4. Verify the digest with `sha256sum`, falling back to `shasum -a 256`. With
   neither available, or on a mismatch, stop without installing anything.
5. Make it executable and run its `version`, which catches a binary the machine
   cannot execute. It must report the chosen release.
6. Rename it to the install location, and report the version and the path.

Any failure removes the temporary files and leaves the directory as it was,
apart from creating it.

The installer never edits a shell startup file, since the first `sync` usually
installs the user's own. When the install directory is not on `PATH`, it says so
and continues; it runs batfiles by absolute path, so the bootstrap does not
depend on `PATH`.

### Script conventions

- POSIX `sh`, checked by `shellcheck` in `task lint` and run under `dash` in
  the [pristine-machine](architecture.md#the-pristine-machine) container.
- The whole body is a function called on the last line, so a truncated download
  runs nothing.
- The installer's own messages go to standard error, so a command it `exec`s
  owns standard output. They are ASCII, like everything else batfiles prints.
- Its own failures exit 1. Once it `exec`s batfiles, the exit status is
  batfiles'.

### On Windows

`install.ps1` is the same installer for PowerShell 7 or later:

```powershell
irm <base>/latest/download/install.ps1 | iex
& ([scriptblock]::Create((irm <base>/latest/download/install.ps1))) clone https://github.com/me/dotfiles
```

The first form ensures a binary; the second ensures one and then runs it with
the arguments after the scriptblock. It takes the same [three
variables](#invocation), set beforehand as `$env:BATFILES_VERSION = '1.2.3'`,
and differs from `install.sh` only where Windows does:

- **Candidates:** the `batfiles` that `Get-Command` finds on `PATH`, then
  `$env:LOCALAPPDATA\Programs\batfiles\batfiles.exe`, the install location.
  `BATFILES_BIN` replaces both, as [it does on Unix](#resolving-a-binary), and
  the same floor applies to `BATFILES_VERSION`.
- **Target:** `PROCESSOR_ARCHITECTURE`, or `PROCESSOR_ARCHITEW6432` in a 32-bit
  PowerShell on a 64-bit Windows, chooses it. `AMD64` and `ARM64` both get
  `x86_64-pc-windows-msvc`, which Windows on ARM runs under emulation; any other
  architecture fails, naming it. On Linux or macOS it refuses, naming
  `install.sh`.
- **Downloads** use `Invoke-WebRequest`, and `Get-FileHash` verifies the
  digest. The staged file's name ends in `.exe`, so that Windows will run its
  `version`.
- **`PATH`** is never changed. When the install directory is not on it, the
  installer prints the `[Environment]::SetEnvironmentVariable` command that
  would add it for the current user.
- **Failures** are terminating errors rather than `exit`, which inside `irm |
  iex` would close the session it ran in; under `pwsh -Command` or `-File` the
  status is 1. Batfiles' own status is left in `$LASTEXITCODE`.
- **Windows PowerShell 5.1**, which every Windows has, is refused with the
  `winget install Microsoft.PowerShell` that provides PowerShell 7. The script
  stays parseable by 5.1 so that it can say so.

It is ASCII, its body a function called on the last line, and its messages go
to standard error with an `install.ps1:` prefix. `task lint` runs
PSScriptAnalyzer over it where `pwsh` is installed.

## Leaf stub

[`init`](cmdline.md#init) writes `install.sh`, executable, and its [Windows
counterpart](#the-windows-stub) `install.ps1`, as the last entries of its
skeleton; [`init --stubs`](cmdline.md#init) adds either to a repository that
lacks it. They are for a machine that already has a checkout:

```sh
git clone https://github.com/me/dotfiles ~/dotfiles && ~/dotfiles/install.sh
```

A new machine with no checkout uses the hosted installer's `clone` one-liner
instead.

The stub's first line is `#!/bin/sh`, its second the marker `# batfiles-stub 1`,
and then two settings; the rest is fixed text, its body a function called on
the last line like the installer's:

```sh
BATFILES_BASE=${BATFILES_BASE:-'<base>'}
BATFILES_VERSION=${BATFILES_VERSION:-}
```

`init` fills in `<base>` from the [release base](#the-release-base), so a
self-hoster runs `BATFILES_BASE=https://mysite/batfiles batfiles init`, or edits
the line afterwards. Setting `BATFILES_VERSION` pins the repository to a release
at least that new. `BATFILES_BIN` chooses the batfiles to use or install, as it
does for the installer.

Run, the stub:

1. Finds its own directory from `$0`, and refuses — printing the `clone`
   one-liner — when `$0` is not a file or there is no `batfiles.toml` beside
   it. That is what happens when the stub is piped into `sh` instead of run
   from a checkout.
2. Looks for a binary among the installer's
   [candidates](#resolving-a-binary), taking the first that reports
   `batfiles <version>` in the [version grammar](#versions).
   - **No pin, and one found:** it is used, and nothing touches the network.
   - **Otherwise** — nothing found, or a pin, whose comparison the stub leaves
     to the installer — it fetches the hosted installer, from
     `<base>/download/v<version>/` when pinned so the installer matches the
     binary and from `latest` otherwise, pipes it into `sh` with its settings
     exported and the `sync` command below as its arguments, and exits with its
     status. The installer resolves, installs if it must, and `exec`s.
   - **The installer cannot be fetched:** a pinned stub that found a binary
     uses it, warning that the pin went unchecked; otherwise it fails.
3. `exec`s `batfiles sync --bootstrap --batfiles-dir <its directory>` with its
   own arguments appended, so `./install.sh --dry-run` works, a checkout
   outside the default location is still the one synchronized, and
   `./install.sh --disable-group gui` starts the machine as `clone` would. See
   [`sync --bootstrap`](cmdline.md#sync).

An installed binary makes an unpinned stub work offline. The stub carries no
version comparison, so nothing about version ordering is frozen into
repositories; it stays in the hosted installer, which changes with each
release. The parts the stub does share with the installer — the version
grammar, reading a candidate's version, and the candidates themselves — are the
same text in both, which `tests/dist` checks.

### The Windows stub

`install.ps1` does the same for PowerShell 7 or later, from a checkout:

```powershell
git clone https://github.com/me/dotfiles $HOME\dotfiles; & $HOME\dotfiles\install.ps1
```

Its first line is `#Requires -Version 7.0`, its second the same marker, and
then its two settings:

```powershell
$BatfilesBase = if ($env:BATFILES_BASE) { $env:BATFILES_BASE } else { '<base>' }
$BatfilesVersion = if ($env:BATFILES_VERSION) { $env:BATFILES_VERSION } else { '' }
```

A pin is written in the second line's `''`. Run, it follows the steps above with
the [Windows installer](#on-windows)'s candidates: it finds its directory from
`$PSScriptRoot`, refusing with the `clone` one-liner when it is not run from a
file beside a `batfiles.toml`; fetches `install.ps1` and runs it as a
scriptblock with the `sync` command as its arguments; and exits with batfiles'
status. The text it shares with the Windows installer is checked the same way.
A machine whose execution policy refuses local scripts runs it as `pwsh
-ExecutionPolicy Bypass -File .\install.ps1`.

### Stability

The stub depends on five contracts, and each is frozen:

- the release tree layout;
- the installer's input variables, and its running its arguments as a
  batfiles command;
- the `batfiles <version>` form of `batfiles version`, in the [version
  grammar](#versions);
- the binary candidates and their order, on each platform; and
- `sync --bootstrap --batfiles-dir`, with the enable and disable options.

Nothing rewrites a stub in a repository: `sync` writes nothing in the leaf but
[`remotes/`](repoformat.md#materialization). `init`, with or without `--stubs`,
writes only a missing stub, and warns about an existing one that lacks the
marker line. The marker lets a future stub format be recognized.

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
one-liner such as the [README's](../README.md#installing). The official site is `https://batfiles.dev`, the custom domain of this
repository's Pages site. A fork's site is at its own Pages address, or its own
domain; nothing in the build names a domain.

The copies are the release's own installers, byte for byte. Still stamped with
the [release base](#the-release-base), they read `VERSION` and download
binaries from the release tree as before; the site serves only the entry point.
A repository without such a site loses nothing: the release URL one-liner works
everywhere.

### The site

`site/` holds the rest of the Pages site, copied as it is, and may not hold an
`install.sh` or `install.ps1` at its root.

`dist:pages OUT=<dir> [FROM=<base>]` builds the site:

1. Read `<from>/latest/download/VERSION` once, so that a release published
   meanwhile cannot mix two. `FROM` defaults to the official base.
2. Fetch both installers from `<from>/download/v<version>/`.
3. Write `site/` and the two installers to `<dir>`, which must be absent or
   empty. Nothing appears there until the site is complete.
4. Print `version=<version>`.

It fails, writing nothing, when `latest` cannot be read, as before a first
stable release; when it names a pre-release; and when `site/` holds an
installer. Because it copies only `latest`, a pre-release never reaches the
site, whatever started the build.

### The Pages workflow

`.github/workflows/pages.yml` builds and deploys the site. It runs:

- on a push to `main` that changes `site/`;
- on demand, from the Actions tab; and
- from the [release workflow](#the-release-workflow), after `verify` passes for
  a stable release.

Its first job reads the repository's Pages configuration. Unless Pages is
enabled with *GitHub Actions* as its source, the workflow says so and succeeds
without deploying, so a fork without Pages, or one serving Pages from a branch,
is left alone. Otherwise one job runs `dist:pages` with `FROM` set to the
repository's releases URL, deploys the result to the `github-pages`
environment, and runs `dist:verify` with `PAGES` set to the deployed site.

That job's runs are serialized and never cancel one in progress, so the last
deploy copies a `latest` at least as new as any deploy before it. The settings
the site needs are in [`dist/README.md`](../dist/README.md#repository-setup).
