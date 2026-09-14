# Batfiles Command-Line Surface

A compact inventory of the command-line interface that is not built yet. Global
and shared options are defined once rather than repeated under every command.

The command overview, the global options, the output streams, and the exit
statuses are built, and are specified in
[`docs/cmdline.md`](../cmdline.md). Everything below is intended behavior and
binds nothing.

## Output Streams

The stream rule itself is in [`docs/cmdline.md`](../cmdline.md#output-streams).
One case is specific to a command that does not exist yet:

A subprocess batfiles runs — a dynamic variable's command — inherits standard
error rather than writing through batfiles, and `--quiet` disconnects it instead
of leaving the noisiest output on an otherwise quiet channel. See [how dynamic
commands are run](environment.md#how-dynamic-commands-are-run) for the rest of
that contract.

## Shared Action Execution Options

These controls are accepted by every command that executes actions. `clone`
accepts them because it forwards them to its follow-up synchronization. The
section and its one built option, `--var`, are specified in
[`docs/cmdline.md`](../cmdline.md#shared-action-execution-options); the rest are
proposed here.

| Option                | Purpose                                                                                          |
|-----------------------|--------------------------------------------------------------------------------------------------|
| `--refresh-vars`      | Recompute allowed dynamic variables instead of using fresh cached values.                        |
| `--refresh-content`   | Refresh existing [seed content](safety.md#seed-actions-and-deletion).                            |
| `--no-overwrite`      | Skip unmanaged destination conflicts instead of backing them up and replacing them.             |
| `--interactive`       | At each unmanaged destination conflict, choose backup-and-replace (default), overwrite, or skip. |

`--no-overwrite` and `--interactive` are mutually exclusive. Without either,
batfiles backs up conflicting unmanaged destinations and proceeds. Interactive
overwrite is an explicit waiver of the backup for that conflict only.

`sync`, `clone`, `apply-action`, and `apply-group` already merge the same
[effective variable set](../environment.md#variable-precedence); what is not
built is the evaluation it feeds — see [string-valued
variables](repoformat.md#string-valued-variables). Dynamic variables will use
fresh cached values and automatically resolve stale or missing ones;
`--refresh-vars` instead forces allowed dynamic variables to be recomputed even
when their cached values are fresh.

## Shared Selection Options

`sync` and `clone` accept both run-only selectors; `apply-group` accepts
`--skip-action` alone, and `apply-action` neither. Which command takes which, and
why, is in [`docs/cmdline.md`](../cmdline.md#selecting-what-a-run-does). Both
already take an [address](../cmdline.md#addresses); what is not built is the
reach of one, since nothing yet contributes what a qualified name asks for:

| Option                 | Comes to reach                                                                    |
|------------------------|-------------------------------------------------------------------------------------|
| `--skip-action <id>`   | An addressable remote, included action, or manifest entry, as well as a leaf action. |
| `--skip-group <group>` | A qualified included group, as well as a leaf one.                                   |

## Dry-Run Behavior

The mechanism, the tense, and what a dry run promises are specified in
[`docs/cmdline.md`](../cmdline.md#dry-run-behavior), which describes what runs
today. What is left here is what the unbuilt half of the tool adds to it: the
remotes an inclusion composes over, and the one thing a dry run does that is not
describing.

**An inclusion is described from the materialization on disk.** That a dry run
neither clones nor updates a remote, and reads whatever tree the last `sync`
left, is already specified in
[`docs/cmdline.md`](../cmdline.md#dry-run-behavior); here it costs more, because
what comes out of that tree is not one action's content but the actions
themselves. An inclusion is described exactly as its materialization declares
it, which may be out of date and knowingly so. An inclusion with no
materialization at all contributes actions that cannot be listed, so it is
reported with a reason and the plan is marked **partial** rather than pretending
to be whole — where a leaf action reaching the same absent tree is simply
refused, because one action's source is a thing the plan can do without and the
list itself is not. A complete plan is one in which nothing was reported that
way. To see the rest, refresh the remote — by running `sync`, or by updating
that checkout by hand — and repeat the dry run.

Dry run still performs normal dynamic-variable resolution. Allowed dynamic
commands may run, and successful results are written to `dynamic-vars.toml`;
with `--refresh-vars --dry-run`, fresh entries are recomputed as well. Those
commands are arbitrary programs and may have filesystem, network, or other side
effects of their own, which is the one part of a dry run batfiles does not
control.

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
| `--dry-run`         | Report the action plan using the shared [dry-run behavior](../cmdline.md#dry-run-behavior).                                                        |
| `--refresh-remotes` | Re-fetch file and archive remotes, replacing their tool-owned materializations. Git remotes are already fetched on every normal sync. |

`--dry-run` and `--refresh-remotes` are mutually exclusive: a dry run
materializes nothing, so there is nothing for it to refresh.

### `apply-action` and `apply-group`

Both are built, and are specified in
[`docs/cmdline.md`](../cmdline.md#apply-action). What is not built is the half
that needs something to refer to:

- `--id` and `--group` take an [address](../cmdline.md#addresses), so a
  qualified one already parses and is already reported as naming nothing. What
  arrives with `include-remote` is a spliced action or group for it to find.
- An addressable entry inside a `git-clone-list` becomes applyable by
  `<action-id>.<entry-id>` when that action exists.
- `include-remote` is never directly applyable, whatever its address.

### `vars set`, `vars get`, `vars list`, and `vars unset`

All four are built, and are specified in
[`docs/cmdline.md`](../cmdline.md#vars-set). What is not built is the dynamic
layer they would have to account for:

- `vars list --no-refresh` is [refused for now](../cmdline.md#unimplemented-options).
  It says not to run dynamic commands or write the cache, and to show available
  cached state as fresh, stale, or missing — none of which exists to be shown.
  It has no additional effect alongside `--machine-only`, which reads neither.
- A listing shows the leaf repository's variables. A remote's arrive with
  per-inclusion scopes at 7.5, whose labels a line would have to name.

`vars refresh`, below, is the rest of the family.

### `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Refresh selected dynamic variables. With no keys, refresh the leaf repository's
dynamic variables together with those of every remote that is in the effective
inclusion set, is allowed to run commands, and is materialized.

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

## Address Forms

Address syntax, and the two forms that resolve against a leaf repository, are
built and specified in [`docs/cmdline.md`](../cmdline.md#addresses). These are
the forms that need something batfiles cannot yet contribute:

| Form                                  | Meaning                                               |
|---------------------------------------|-------------------------------------------------------|
| `<action-id>.<entry-id>`              | Addressable entry in a leaf `git-clone-list`.         |
| `<remote>.<action-id>`                | Action spliced from an addressable included remote.   |
| `<remote>.<group>`                    | Group spliced from an addressable included remote.    |
| `<remote>.<action-id>.<entry-id>`     | Manifest entry inside an addressable included action. |

Each of these parses today and resolves to nothing, so what arrives with the
`git-clone-list` action and with `include-remote` is the *lookup*, not the name.

An unqualified action or group always refers to the leaf repository; batfiles
does not search included remotes for a matching unqualified name. In a qualified
address, `<remote>` is the `id` of the leaf's `include-remote` action. It is the
address prefix for that particular inclusion and need not match the remote name
in the action's `remote` field.

Only configuration items with the required IDs can be addressed individually.
Groups and included content from an `include-remote` action without an ID may
still run during normal synchronization but cannot be targeted through a
qualified command-line address.
