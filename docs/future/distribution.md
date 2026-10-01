# Distribution

How a machine gets a `batfiles` binary: `batfiles update`, and the Windows
installer and stub. The [release tree](../distribution.md), the POSIX [hosted
installer](../distribution.md#hosted-installer), the [leaf
stub](../distribution.md#leaf-stub), and
[self-hosting](../distribution.md#self-hosting) are built; the rest is not.
The [product goals](../goals.md#product-model) state the scope; [slice
10](roadmap.md#slice-10--distribution) numbers the work.

## Pieces

| Piece | Lives | Changes | Job |
| --- | --- | --- | --- |
| [Release tree](../distribution.md#release-tree) | A release base URL | Every release | Binaries, checksums, installers, `VERSION` |
| [Hosted installer](../distribution.md#hosted-installer) | `install.sh` and `install.ps1` in the release tree | Every release | Put a verified binary at its install location, then optionally run it |
| [Leaf stub](../distribution.md#leaf-stub) | `install.sh` (and `install.ps1`) in a leaf repository | Frozen | Find or fetch `batfiles`, then `sync` its own checkout |
| `batfiles update` | The binary | Every release | Replace the running binary with another release |

All logic that follows releases — platform detection, asset names, checksum
verification — lives in the hosted installer and the binary. The stub only
locates things, so the one way it can go stale is a change to the contracts in
[stub stability](../distribution.md#stability), which are frozen.

## Release tree

The [release tree](../distribution.md#release-tree), its assets and targets, and
the tasks and workflow that [build a release](../distribution.md#building-a-release)
are implemented. What remains of the [release
base](../distribution.md#the-release-base):

- **The binary.** [`update`](#batfiles-update) reads the same base
  `init` does. The binary also compiles in its own target triple, so it knows
  its asset name.

### GitHub Pages

When the repository has GitHub Pages enabled, the release workflow also
publishes copies of the two hosted installers at the site root, for a shorter
one-liner; a fork without Pages skips that job, and everything keeps working
from the release URL alone. Open: how the job coexists with other content on the
same Pages site, and that only a stable release replaces the copies.

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

- `install.ps1` takes the same three variables. The default install location
  is `$env:LOCALAPPDATA\Programs\batfiles\batfiles.exe`.
  `$env:PROCESSOR_ARCHITECTURE` selects the target, and `Get-FileHash` verifies
  it.
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
tests, and only the release workflow uses a Windows runner, to build the binary.
That step adds a Windows CI job running the installer's tests natively, and a
Windows runner to `dist:smoke`; a `pwsh` parse on Linux cannot show that the
installer works on Windows.

## On promotion

Distribution material joins [`docs/distribution.md`](../distribution.md), and
`update` lands in [`cmdline.md`](../cmdline.md), with whatever it reads from the
environment in [`environment.md`](../environment.md).
