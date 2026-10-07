# `init`

```text
batfiles init [--no-git-init | --stubs]
```

Create a leaf-repository skeleton in the current directory. Location options
such as `--batfiles-dir` have no effect.

| Option | Purpose |
| --- | --- |
| `--no-git-init` | Skip `git init`; it is also skipped automatically inside a Git repository |
| `--stubs` | Add only missing installer stubs to an existing repository |

The layout is created in this order: `batfiles.toml`, `.gitignore`, `bin/`,
`files/`, executable [`install.sh`](../installer.md#leaf-stub), and
[`install.ps1`](../installer.md#the-windows-stub), on every platform. The
starter manifest has only commented samples and installs nothing. The
`.gitignore` excludes generated `remotes/`; that tree is not created by `init`.
Stubs use the selected [release base](../environment.md#release-base), validated
before anything is created.

Before writing, `init` refuses:

- Anything already named `batfiles.toml`.
- The invoking user's OS home directory, regardless of `--home-dir`. An
  undeterminable home does not fail this check.
- A layout path of the wrong filesystem kind. Symlinks are classified by their
  targets; broken ones are the wrong kind.

Existing paths of the expected kind retain contents and permissions and are
omitted from the creation report. An existing `.gitignore` that appears not to
cover `remotes/`, or an existing installer that is not a batfiles stub, warns
and is left alone. A failure to launch or complete `git init` fails the command;
any layout already created remains.

## Adding the stubs to a repository

`init --stubs` requires a `batfiles.toml` file in the current directory. It
writes only missing stubs, creates no other layout, and runs no Git. It cannot
be combined with `--no-git-init`. Existing stubs follow the rules above; a wrong
node kind refuses the command before writing. Output names the stubs created,
or reports that all were already present.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
