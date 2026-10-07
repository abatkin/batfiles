# Installation reference

## Hosted installer

### Invocation

```sh
curl -fsSL <base>/latest/download/install.sh | sh
curl -fsSL <base>/latest/download/install.sh | sh -s -- clone https://github.com/me/dotfiles
```

The official installers are also served from `https://batfiles.dev/`; see
[GitHub Pages](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#github-pages).

With no arguments it ensures a binary and exits. With arguments it ensures a
binary and then `exec`s it with those arguments, which makes the second form
the one-command bootstrap of a new machine: [`clone`](commands/clone.md#clone) does
the rest. When its standard input is not a terminal, as when it is piped into
`sh`, and the process has a controlling terminal, batfiles gets `/dev/tty` as
standard input, so a command such as `clone --interactive` can still ask.

Inputs, all optional:

| Variable | Meaning |
| --- | --- |
| `BATFILES_BASE` | The release base. Defaults to the stamped base. |
| `BATFILES_VERSION` | A release to fetch, a [version](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions) with or without a leading `v`. Empty or `latest` means the latest release. |
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
  [`update`](commands/update.md#update) is for; an installer run
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
versions compare as [versions](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions) order. A candidate that fails to run,
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
  the [pristine-machine](https://github.com/abatkin/batfiles/blob/main/docs/contributing/architecture.md#the-pristine-machine) container.
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

[`init`](commands/init.md#init) writes `install.sh`, executable, and its [Windows
counterpart](#the-windows-stub) `install.ps1`, as the last entries of its
skeleton; [`init --stubs`](commands/init.md#init) adds either to a repository that
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

`init` fills in `<base>` from the [release base](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#the-release-base), so a
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
   `batfiles <version>` in the [version grammar](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions).
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
   [`sync --bootstrap`](commands/sync.md#sync).

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
  grammar](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions);
- the binary candidates and their order, on each platform; and
- `sync --bootstrap --batfiles-dir`, with the enable and disable options.

Nothing rewrites a stub in a repository: `sync` writes nothing in the leaf but
[`remotes/`](repoformat.md#materialization). `init`, with or without `--stubs`,
writes only a missing stub, and warns about an existing one that lacks the
marker line. The marker lets a future stub format be recognized.

