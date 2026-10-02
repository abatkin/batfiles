# Distribution

What remains of how a machine gets a `batfiles` binary: the Windows installer
and stub, replacing a running binary on Windows, and the GitHub Pages copies of
the hosted installers. The [release tree](../distribution.md), the POSIX [hosted
installer](../distribution.md#hosted-installer), the [leaf
stub](../distribution.md#leaf-stub), [`batfiles update`](../cmdline.md#update),
and [self-hosting](../distribution.md#self-hosting) are built. The [product
goals](../goals.md#product-model) state the scope; [slice
10](roadmap.md#slice-10--distribution) numbers the work.

## GitHub Pages

When the repository has GitHub Pages enabled, the release workflow also
publishes copies of the two hosted installers at the site root, for a shorter
one-liner; a fork without Pages skips that job, and everything keeps working
from the release URL alone. Open: how the job coexists with other content on the
same Pages site, and that only a stable release replaces the copies.

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
- [`update`](../cmdline.md#update) replaces the running `batfiles.exe`, which
  Windows will not rename over, by renaming it aside first and the staged
  release into its place. What becomes of the file set aside, which cannot be
  removed while it runs, is open.

The symlink actions fail on Windows today (see the [project
README](../../README.md#what-works-today)), which limits what a Windows bootstrap
can install and is why this work comes last. Until then the release carries a
placeholder `install.ps1`, and `update` fails on Windows before downloading
anything. CI checks Windows compilation but runs no Windows tests, and only the
release workflow uses a Windows runner, to build the binary. That step adds a
Windows CI job running the installer's tests, and `update`'s, natively, and a
Windows runner to `dist:smoke`; a `pwsh` parse on Linux cannot show that the
installer works on Windows.

## On promotion

The Windows material joins [`docs/distribution.md`](../distribution.md), and
Windows `update` its section of [`cmdline.md`](../cmdline.md#update).
