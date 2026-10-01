# Distribution

How a machine gets a `batfiles` binary: the hosted installers, the stub a leaf
repository carries, self-hosting, and `batfiles update`. The [release
tree](../distribution.md) they read is built; the rest is not. The [product
goals](../goals.md#product-model) state the scope; [slice
10](roadmap.md#slice-10--distribution) numbers the work.

## Pieces

| Piece | Lives | Changes | Job |
| --- | --- | --- | --- |
| [Release tree](../distribution.md#release-tree) | A release base URL | Every release | Binaries, checksums, installers, `VERSION` |
| Hosted installer | `install.sh` and `install.ps1` in the release tree | Every release | Put a verified binary in the install directory, then optionally run it |
| Leaf stub | `install.sh` (and `install.ps1`) in a leaf repository | Frozen | Find or fetch `batfiles`, then `sync` its own checkout |
| `batfiles update` | The binary | Every release | Replace the running binary with another release |

All logic that follows releases — platform detection, asset names, checksum
verification — lives in the hosted installer and the binary. The stub only
locates things, so the one way it can go stale is a change to the contracts in
[stub stability](#stability), which are frozen.

## Release tree

The [release tree](../distribution.md#release-tree), its assets and targets, and
the tasks and workflow that [build a release](../distribution.md#building-a-release)
are implemented. What remains of the [release
base](../distribution.md#the-release-base):

- **Hosted installers.** `BATFILES_BASE` in the environment overrides the
  stamped default.
- **The binary.** It compiles in `BATFILES_DEFAULT_BASE`, falling back to the
  official base; `BATFILES_BASE` overrides it at run time, for
  [`update`](#batfiles-update) and for the stub [`init`](#leaf-stub) writes. It
  also compiles in its own target triple, so it knows its asset name.

### GitHub Pages

When the repository has GitHub Pages enabled, the release workflow also
publishes copies of the two hosted installers at the site root, for a shorter
one-liner; a fork without Pages skips that job, and everything keeps working
from the release URL alone. Open: how the job coexists with other content on the
same Pages site, and that only a stable release replaces the copies.

## Hosted installer

### Invocation

```sh
curl -fsSL <base>/latest/download/install.sh | sh
curl -fsSL <base>/latest/download/install.sh | sh -s -- clone https://github.com/me/dotfiles
```

With no arguments it ensures a binary and exits. With arguments it ensures a
binary and then `exec`s it with those arguments, which makes the second form
the one-command bootstrap of a new machine: [`clone`](../cmdline.md#clone) does
the rest.

Inputs, all optional:

| Variable | Meaning |
| --- | --- |
| `BATFILES_BASE` | The release base. Defaults to the stamped base. |
| `BATFILES_VERSION` | A release to fetch, a [version](../distribution.md#release-tree) with or without a leading `v`. Empty or `latest` means the latest release. |
| `BATFILES_INSTALL_DIR` | Where a downloaded binary goes. Defaults to `$HOME/.local/bin`. |

Because the script is piped, a variable reaches it as `curl … | BATFILES_BASE=…
sh`.

### Resolving a binary

The installer and the stub resolve a binary the same way. The candidates, in
order, are the `batfiles` found on `PATH` and `$BATFILES_INSTALL_DIR/batfiles`.

- **No version requested:** the first candidate that exists is used, and
  nothing is downloaded. Upgrading an existing binary is what
  [`update`](#batfiles-update) is for; an installer that is run again reports
  which binary it found and suggests `batfiles update`.
- **`BATFILES_VERSION` set:** it names exactly what is downloaded, and is a
  floor for what is accepted. The first candidate whose version is at least the
  requested one is used. If none is, the requested release is installed into
  the install directory and used by its absolute path. A candidate on `PATH`
  that was passed over is named in a warning and never modified.

A floor rather than an exact match keeps a pin from fighting `update`: a
repository that needs 1.4 accepts 1.5 without downgrading it, and a machine
still holding 1.2 gets 1.4.

A candidate's version is read from `batfiles version`, whose output (`batfiles
X.Y.Z`) becomes a [frozen contract](#stability). Versions compare numerically
by major, minor, and patch; a pre-release suffix ranks below the release it
precedes. A candidate that fails to run or prints something else is passed over.

### Installing

The installer only ever writes inside the install directory, creating it if
needed:

1. Map `uname -s` and `uname -m` to a target: `Linux` to musl, `Darwin` to
   Apple; `x86_64`/`amd64` and `aarch64`/`arm64`. An x86_64 shell on Apple
   silicon (Rosetta, reported by `sysctl hw.optional.arm64`) gets the aarch64
   binary. An unmapped platform fails, naming what was detected.
2. Download the binary to a temporary file in the install directory, and
   `SHA256SUMS` beside it, with `curl`, falling back to `wget`.
3. Verify the digest with `sha256sum`, falling back to `shasum -a 256`. With
   neither available, or on a mismatch, stop without installing anything.
4. Make it executable and run its `version`, which catches a binary the machine
   cannot execute.
5. Rename it over `batfiles` in the install directory, and report the version
   and the path.

**The installer never edits a shell startup file.** The first `sync` is about to
install the user's own `.bashrc` or `.zshrc`; an installer that had just edited
one would put a file in the way of the repository's first action. When the
install directory is not on `PATH`, the installer says so and moves on. It runs
batfiles by absolute path, so the bootstrap never depends on `PATH`.

### Script conventions

- POSIX `sh`, checked by `shellcheck` in `task lint` (with the other scripts
  under `dist/` and `tests/docker/`) and run under `dash` in the
  pristine-machine container.
- The whole body is a function called on the last line, so a truncated download
  runs nothing.
- The installer's own messages go to standard error, so a command it `exec`s
  owns standard output. They are ASCII, like everything else batfiles prints.
- Its own failures exit 1. Once it `exec`s batfiles, the exit status is
  batfiles'.

## Leaf stub

[`init`](../cmdline.md#init) writes `install.sh` as the last entry of its
skeleton, executable, and under the same rule as the rest of the skeleton: an
existing path is left alone. It is for a machine that already has a checkout:

```sh
git clone https://github.com/me/dotfiles ~/dotfiles && ~/dotfiles/install.sh
```

A new machine with no checkout uses the hosted installer's `clone` one-liner
instead.

The stub opens with a marker line, `# batfiles-stub 1`, and two settings; the
rest is fixed text:

```sh
BATFILES_BASE=${BATFILES_BASE:-<base>}
BATFILES_VERSION=${BATFILES_VERSION:-}
```

`init` fills in `<base>` from the binary's [release base](#release-tree),
so a self-hoster runs `BATFILES_BASE=https://mysite/batfiles batfiles init`,
or edits the line afterwards. Setting `BATFILES_VERSION` pins the repository to
a release at least that new.

Run, the stub:

1. Finds its own directory, and refuses — pointing at the `clone` one-liner —
   when there is no `batfiles.toml` there. That is what happens when the stub
   is piped into `sh` instead of run from a checkout.
2. [Resolves a binary](#resolving-a-binary). Only when that would download
   does it fetch the hosted installer — from `<base>/download/v<X.Y.Z>/` when
   pinned, so the installer matches the binary, and from `latest` otherwise —
   and run it with its settings exported.
3. `exec`s `batfiles sync --batfiles-dir <its directory>` with its own arguments
   appended, so `./install.sh --dry-run` works and a checkout outside the
   default location is still the one synchronized.

An installed binary makes the stub work offline.

### Stability

The stub depends on four contracts, and each is frozen:

- the release tree layout;
- the installer's input variables;
- the `batfiles X.Y.Z` form of `batfiles version`; and
- `sync --batfiles-dir`.

Nothing inspects or rewrites a stub in a repository: `sync` writes nothing in
the leaf but [`remotes/`](../repoformat.md#materialization), and a tracked file
it changed would dirty the user's working tree. The marker line exists so that a
second stub format, should one ever be needed, can be recognized and regenerated
by an explicit command added then.

## `batfiles update`

```text
batfiles update [<version>] [--check]
```

Replace the running binary with the latest release, or with `<version>`. The
user runs it; batfiles never runs it, and no other command checks for updates.

- The base is `BATFILES_BASE`, else the compiled-in base. A self-hoster sets
  `BATFILES_BASE` in the shell environment their own dotfiles install.
- It reads `VERSION` from the chosen release. The latest release that is not
  newer than the running binary is reported and nothing is downloaded. An
  explicit `<version>` is installed even if it is older.
- It downloads its own target's asset and `SHA256SUMS`, verifies the digest,
  stages the file beside the running executable, checks that the staged binary
  runs `version`, and renames it into place. A directory the user cannot write
  fails before anything is downloaded, naming the path.
- `--check` downloads only `VERSION`, and prints what is available and what is
  running.
- It resolves none of the four roots, like `version`.

Replacing a running executable on Windows is done by renaming it aside first;
the Windows step specifies that.

## Windows

The Windows installer and stub mirror the Unix ones within reason:

- `install.ps1` takes the same three variables. The default install directory
  is `$env:LOCALAPPDATA\Programs\batfiles`. `$env:PROCESSOR_ARCHITECTURE`
  selects the target, and `Get-FileHash` verifies it.
- `irm <base>/latest/download/install.ps1 | iex` installs.
  `& ([scriptblock]::Create((irm <base>/latest/download/install.ps1))) clone
  <url>` installs and runs.
- It does not modify the user's `PATH`, matching the Unix installer; it prints
  the command that would.
- `init` writes an `install.ps1` stub beside `install.sh` on every platform,
  since one repository can serve both. It is frozen to the same contracts.

The symlink actions fail on Windows today (see the [project
README](../../README.md#what-works-today)), which limits what a Windows bootstrap
can install and is why this work comes last. Until then the release carries a
placeholder `install.ps1`. CI checks Windows compilation but runs no Windows
tests, and only the release workflow uses a Windows runner, to build the binary;
that step decides whether the installer is tested on one or stays checked by a
`pwsh` parse on Linux.

## Self-hosting

A self-hoster serves a release tree from any static host:

1. `task dist:mirror VERSION=<version> BASE=<url> OUT=<dir>` downloads an
   official release, verifies it against its `SHA256SUMS`, and restamps both
   installers' [stamp lines](../distribution.md#the-release-base) with the new
   base. The task wraps a POSIX script so a mirror can run without Task.
2. Upload `<dir>` to both `<url>/download/v<version>/` and
   `<url>/latest/download/`, and check it with `task dist:verify URL=<url>`.

After that, `<url>` works like the official base everywhere: the installer
one-liner, stubs written with `BATFILES_BASE=<url>`, and `update` under the same
variable.

## On promotion

Distribution material joins [`docs/distribution.md`](../distribution.md), and the
rest lands with its owners: `update` and the stub `init` writes in [`cmdline.md`](../cmdline.md),
`BATFILES_BASE` as the binary reads it in
[`environment.md`](../environment.md), the installer in the pristine-machine
section of [`architecture.md`](../architecture.md#the-pristine-machine), and the
one-liners in the project README's quick start.
