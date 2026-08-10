# Batfiles Command-Line Surface

This document is a compact inventory of the intended command-line interface.
Global and shared options are defined once rather than repeated under every
command.

## Command Overview

```text
batfiles [global-options] <command> [command-options]

Commands:
  init
  version

  clone
  sync

  disable-action
  enable-action
  disable-group
  enable-group

  apply-action
  apply-group

  vars set
  vars get
  vars list
  vars unset
  vars refresh
```

Global options may appear before or after the command name.

## Global Options

These options are accepted by every command, although a command only uses the
locations relevant to its work. `init`, for example, operates on the current
directory and does not use any selected roots.

| Option                          | Purpose                                                                                                           |
|---------------------------------|-------------------------------------------------------------------------------------------------------------------|
| `-v`, `--verbose`               | Increase diagnostic detail. May be repeated, such as `-vv`.                                                       |
| `-q`, `--quiet`                 | Suppress informational output and leave errors or explicitly requested data. Mutually exclusive with `--verbose`. |
| `--color <auto\|always\|never>` | Control colored output. Defaults to `auto`.                                                                       |
| `--batfiles-dir <path>`         | Select the leaf repository. Defaults to `<selected-home>/dotfiles`.                                               |
| `--home-dir <path>`             | Select the destination home directory. Defaults to the current user's home directory.                             |
| `--config-dir <path>`           | Select the directory containing `vars.toml` and `disabled.toml`. Defaults to the XDG config location.             |
| `--cache-dir <path>`            | Select the directory containing `dynamic-vars.toml`. Defaults to the XDG cache location.                          |

## Output Streams

Every command follows one rule for where its output goes:

- **Standard output** carries requested data: the value a command was asked
  for, and nothing else. It is never suppressed by `--quiet`, because `--quiet`
  suppresses what a command *did*, not what it was *asked for*. Data lines
  carry no label and no color, so `$(batfiles vars get editor)` yields the
  value alone.
- **Standard error** carries everything else: errors, warnings, progress, the
  lines describing what a command did, and any interactive prompt. `--quiet`
  suppresses the informational lines while leaving warnings and errors; `-v`
  adds detail.

A subprocess batfiles runs — a dynamic variable's command — inherits standard
error rather than writing through batfiles, and `--quiet` disconnects it instead
of leaving the noisiest output on an otherwise quiet channel. See [how dynamic
commands are run](environment.md#how-dynamic-commands-are-run) for the rest of
that contract.

Most commands produce no requested data at all and therefore write nothing to
standard output. `version` and `vars get` are the current exceptions, and
`vars list` joins them.

## Shared Action Execution Options

These controls are accepted by every command that executes actions. `clone`
accepts them because it forwards them to its follow-up synchronization.

| Option                | Purpose                                                                                          |
|-----------------------|--------------------------------------------------------------------------------------------------|
| `--var <key=value>`   | Set a one-shot string variable. Repeatable; the last value for a key wins.                       |
| `--refresh-vars`      | Recompute allowed dynamic variables instead of using fresh cached values.                        |
| `--refresh-content`   | Refresh existing [seed content](safety.md#seed-actions-and-deletion).                            |
| `--no-overwrite`      | Skip unmanaged destination conflicts instead of backing them up and replacing them.             |
| `--interactive`       | At each unmanaged destination conflict, choose backup-and-replace (default), overwrite, or skip. |

`--no-overwrite` and `--interactive` are mutually exclusive. Without either,
batfiles backs up conflicting unmanaged destinations and proceeds. Interactive
overwrite is an explicit waiver of the backup for that conflict only.

`sync`, `clone`, `apply-action`, and `apply-group` all use the same
variable resolution when evaluating conditions and interpolating action fields.
One-shot `--var` values participate at their normal highest precedence. Dynamic
variables use fresh cached values and automatically resolve stale or missing
values; `--refresh-vars` instead forces allowed dynamic variables to be
recomputed even when their cached values are fresh.

## Shared Sync and Clone Selection Options

These run-only selectors are accepted by `sync` and `clone` only:

| Option                 | Purpose                                                                                          |
|------------------------|--------------------------------------------------------------------------------------------------|
| `--skip-action <id>`   | Skip an addressable remote, action, included action, or manifest entry for this run. Repeatable. |
| `--skip-group <group>` | Skip a leaf or qualified included group for this run. Repeatable.                                |

## Dry-Run Behavior

`--dry-run` prevents action execution and changes to remote materializations,
but it evaluates actions in order against a shadow filesystem layered over the
real starting state. Each hypothetical create, replacement, or other modeled
mutation updates that shadow state, so later actions observe what earlier
actions would have left behind. This preserves normal sequential action
semantics without diagnosing overlaps as conflicts. When an effect cannot be
modeled reliably, the affected output is marked partial rather than pretending
to know the later state.

Dry run still performs normal dynamic-variable resolution. Allowed dynamic
commands may run, and successful results are written to `dynamic-vars.toml`;
with `--refresh-vars --dry-run`, fresh entries are recomputed as well.
Because dynamic commands are arbitrary programs, they may cause their own
filesystem, network, or other side effects. Dry run therefore describes the
action plan but does not promise that the `batfiles` process is completely
side-effect-free.

## Commands

### `sync`

```text
batfiles sync [action-options] [selection-options] [--dry-run] [--refresh-remotes]
```

Build and apply the desired installation plan for an existing leaf repository.
In addition to the shared action-execution and sync/clone selection options,
it accepts:

| Option              | Purpose                                                                                                                               |
|---------------------|---------------------------------------------------------------------------------------------------------------------------------------|
| `--dry-run`         | Report the action plan using the shared [dry-run behavior](#dry-run-behavior).                                                        |
| `--refresh-remotes` | Re-fetch file and archive remotes, replacing their tool-owned materializations. Git remotes are already fetched on every normal sync. |

`--dry-run` and `--refresh-remotes` are mutually exclusive.

### Enable and disable actions or groups

```text
batfiles disable-action <id>...
batfiles enable-action <id>...
batfiles disable-group <group>...
batfiles enable-group <group>...
```

Persistently add one or more action or group addresses to, or remove them from,
the machine-local disabled lists. These commands do not run synchronization or
remove installed content.

They read and write `disabled.toml` only. They do not resolve or load the leaf
repository, and they validate each supplied address for [syntax](#address-forms)
alone, as described in the state specification's [`disabled.toml`
lifecycle](state.md#semantics-and-lifecycle-1).

### `apply-action`

```text
batfiles apply-action --id <id> [action-options] [--dry-run]
```

Explicitly apply one directly executable action or addressable manifest entry
using the same action semantics as `sync`.

| Option      | Purpose                                                                                                                                 |
|-------------|-----------------------------------------------------------------------------------------------------------------------------------------|
| `--id <id>` | Required action or manifest-entry address. Included actions use qualified addresses. `include-remote` itself is not directly applyable. |
| `--dry-run` | Report the application plan using the shared [dry-run behavior](#dry-run-behavior).                                                     |

### `apply-group`

```text
batfiles apply-group --group <group> [action-options] [--dry-run]
```

Explicitly apply the enabled, directly executable actions in one group using
the same action semantics as `sync`.

| Option            | Purpose                                                                                   |
|-------------------|-------------------------------------------------------------------------------------------|
| `--group <group>` | Required leaf or qualified included group address.                                        |
| `--dry-run`       | Report the group application plan using the shared [dry-run behavior](#dry-run-behavior). |

### `vars set`

```text
batfiles vars set <key> <value>
```

Set one persisted machine-local variable. The value is stored verbatim as a
string, the empty string included; `vars get`'s absent-key failure is what keeps
an empty value distinguishable from no value.

The confirmation line names the key and never the value. A value may be a token
or a path that identifies a machine, and an informational line would put it in
terminal scrollback and in a calling script's logs. `vars get` is the way to
read a value back.

### `vars get`

```text
batfiles vars get <key>
```

Print the stored machine-local string for one variable on standard output. It
does not resolve repository defaults, dynamic values, facts, or environment
values.

A key with no machine-local value is a failure: nothing is written to standard
output, and the diagnostic naming the key goes to standard error. Printing an
empty line and exiting successfully would be indistinguishable from a key stored
as the empty string.

### `vars list`

```text
batfiles vars list [--machine-only] [--no-refresh]
```

List the effective variables from the selected leaf repository after applying
persisted machine-local values and `BATFILES_VAR_*` environment overrides. Host
environment values are also resolved through the read-only `env.*` namespace.

| Option           | Purpose                                                                                                  |
|------------------|----------------------------------------------------------------------------------------------------------|
| `--machine-only` | List only persisted machine-local variables without reading the repository or dynamic-variable cache.    |
| `--no-refresh`   | Do not run dynamic commands or write the cache; show available cached state as fresh, stale, or missing. |

The two options may be combined; `--no-refresh` has no additional effect when
`--machine-only` is used.

### `vars unset`

```text
batfiles vars unset <key>
```

Remove one persisted machine-local variable. An absent key is an idempotent
success.

### `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Refresh selected dynamic variables declared by the leaf repository. With no
keys, refresh all of the leaf repository's dynamic variables. This command does
not refresh variables declared by remotes.

### `clone`

```text
batfiles clone <url> [action-options] [selection-options]
    [--enable-action <id>]... [--disable-action <id>]...
    [--enable-group <group>]... [--disable-group <group>]...
```

Clone a new leaf repository into the selected batfiles directory, adopt its
bootstrap enable/disable policy, and then synchronize it. The destination must
not already exist.

The shared action-execution and sync/clone selection options are listed above.
Bootstrap-only options are:

| Option                    | Purpose                                                                              |
|---------------------------|--------------------------------------------------------------------------------------|
| `--enable-action <id>`    | Remove an action address from persisted disabled state during bootstrap. Repeatable. |
| `--disable-action <id>`   | Add an action address to persisted disabled state during bootstrap. Repeatable.      |
| `--enable-group <group>`  | Remove a group address from persisted disabled state during bootstrap. Repeatable.   |
| `--disable-group <group>` | Add a group address to persisted disabled state during bootstrap. Repeatable.        |

The environment specification defines the authoritative [bootstrap adoption
precedence](environment.md#bootstrap-enable-and-disable-lists).

`clone` intentionally does not accept `--dry-run` or `--refresh-remotes`. A
fresh clone materializes its remotes during the follow-up sync; use
`sync --dry-run` after cloning to inspect later plans.

### `init`

```text
batfiles init [--no-git-init]
```

Initialize the current directory with the conventional leaf-repository layout
without overwriting existing files.

| Option          | Purpose                                                                                                   |
|-----------------|-----------------------------------------------------------------------------------------------------------|
| `--no-git-init` | Do not run `git init`. Batfiles also skips `git init` automatically when already inside a Git repository. |

The conventional layout is `batfiles.toml`, `install.sh`, `.gitignore`, `bin/`,
`files/`, and `local-files/`. The generated `remotes/` tree is not created; the
written `.gitignore` excludes it instead. An existing path of the expected kind
is left exactly as it is, including its contents and permissions.

`init` refuses to run, before creating anything, when:

- The current directory already contains anything named `batfiles.toml`,
  whatever kind of filesystem node it is. The directory is already a batfiles
  repository, and `init` is not a repair path for one.
- The current directory is the invoking user's OS home directory. Only that
  directory is refused; a directory below it, such as the default
  `~/dotfiles`, is the normal case. A home that cannot be determined is not
  fatal here, because `init` needs one only for this check.
- A path of the conventional layout exists as the wrong kind of filesystem
  node, such as a regular file named `files`. Symlinks are judged by what they
  point at, and a symlink pointing at nothing is a wrong kind too.

`init` also fails when Git initialization was requested and `git` could not be
run or `git init` failed. Whatever was already created stays; `init` does not
unwind a partial layout. Use `--no-git-init` to initialize without Git.

Everything `init` prints is a diagnostic on standard error. It produces no
requested data, so `--quiet` leaves only warnings and errors.

### `version`

```text
batfiles version
```

Print the batfiles version to standard output and exit successfully. This
command does not resolve the selected repository, home, config, or cache
directories.

## Address Forms

Options that accept action IDs or group names may use these forms:

| Form                                  | Meaning                                               |
|---------------------------------------|-------------------------------------------------------|
| `<action-id>`                         | Top-level action in the leaf repository.              |
| `<group>`                             | Group in the leaf repository.                         |
| `<action-id>.<entry-id>`              | Addressable entry in a leaf `git-clone-list`.         |
| `<remote>.<action-id>`                | Action spliced from an addressable included remote.   |
| `<remote>.<group>`                    | Group spliced from an addressable included remote.    |
| `<remote>.<action-id>.<entry-id>`     | Manifest entry inside an addressable included action. |

Every placeholder between dots must match the repository format's [shared ID
syntax](repoformat.md#names-and-ids). Dots are address separators and are not
part of an individual ID.

Address *syntax* is just that rule: an address is a nonempty dot-separated list
of valid IDs, with no upper bound on the number of segments. The table above
lists the shapes the current repository model can *resolve*, which is a separate
question. A syntactically valid address with more segments than any of those
forms — `a.b.c.d.e` — is well formed and simply names nothing, so a command that
resolves it reports that it was not found rather than that it was malformed.
Commands that record an address without resolving it, such as the persistent
enable and disable commands, accept any syntactically valid address.

An unqualified action or group always refers to the leaf repository; batfiles
does not search included remotes for a matching unqualified name. In a qualified
address, `<remote>` is the `id` of the leaf's `include-remote` action. It is the
address prefix for that particular inclusion and need not match the remote name
in the action's `remote` field.

Only configuration items with the required IDs can be addressed individually.
Groups and included content from an `include-remote` action without an ID may
still run during normal synchronization but cannot be targeted through a
qualified command-line address.
