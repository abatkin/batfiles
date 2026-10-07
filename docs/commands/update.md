# `update`

```text
batfiles update [<version>] [--check]
```

Replace the running binary with the latest [release](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#release-tree),
or with `<version>`. Only this command installs or checks for updates, and only
when run. No location roots are resolved.

| Argument or option | Purpose |
|--------------------|---------|
| `<version>`        | The release to install: a [version](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions), with or without a leading `v`, or `latest`, the default. One outside the grammar is a usage error. |
| `--check`          | Print what is running and what is available, and install nothing. |

Releases come from the [release base](../environment.md#release-base).

Without `<version>`, `update` reads `<base>/latest/download/VERSION` once and
downloads nothing unless that release is newer, by [version
order](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#versions), than the running one; a pre-release therefore
stays until a stable release passes it. If `VERSION` cannot be read, the error
suggests naming a release, since the base may have published only pre-releases.
A named `<version>` is installed even when it is older or the same, which also
repairs a binary.

Installation uses `<base>/download/v<version>/`:

1. Create a private file beside the running executable, after resolving
   symlinks, so a link to batfiles stays a link. An unwritable directory fails
   before downloading. An existing file at that path, such as one an interrupted
   update left, is never replaced; remove it and retry.
2. Download `SHA256SUMS` and this build's [asset](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#targets) into
   that file, verifying its digest.
3. Make it executable and run its `version`, which must report the release.
4. Rename it over the running executable, and report both versions and the path.

Any failure removes the file and leaves the running binary unchanged.

On Windows, the staged file ends in `.exe`, and the running `batfiles.exe` is
first renamed aside to `batfiles.exe.batfiles-old` (and restored if the second
rename fails). A hidden `%SystemRoot%` `powershell.exe` removes that file after
`update` exits. If another running batfiles still holds it, the next `update`
removes it first, reporting at `-v` and warning on failure.

`--check` reads only the needed `VERSION`: `latest/download/` without
`<version>`, otherwise that release's own, which must hold the named version.
It prints two lines of requested data to standard output:

```text
running 1.2.0
available 1.3.0
```

It succeeds whether or not the available release is newer; an unreadable or
invalid `VERSION` fails. Without `--check`, all output is diagnostic.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
