# Batfiles Command-Line Surface

A compact inventory of the command-line interface that is not built yet. Global
and shared options are defined once rather than repeated under every command.

The command overview, the global options, the output streams, and the exit
statuses are built, and are specified in
[`docs/cmdline.md`](../cmdline.md). Everything below is intended behavior and
binds nothing.

## Shared Action Execution Options

These controls are accepted by every command that executes actions. `clone`
accepts them because it forwards them to its follow-up synchronization. The
section and its two built options, `--var` and `--refresh-vars`, are specified
in [`docs/cmdline.md`](../cmdline.md#shared-action-execution-options); the rest
are proposed here.

| Option                | Purpose                                                                                          |
|-----------------------|--------------------------------------------------------------------------------------------------|
| `--refresh-content`   | Refresh existing [seed content](safety.md#seed-actions-and-deletion).                            |
| `--no-overwrite`      | Skip unmanaged destination conflicts instead of backing them up and replacing them.             |
| `--interactive`       | At each unmanaged destination conflict, choose backup-and-replace (default), overwrite, or skip. |

`--no-overwrite` and `--interactive` are mutually exclusive. Without either,
batfiles backs up conflicting unmanaged destinations and proceeds. Interactive
overwrite is an explicit waiver of the backup for that conflict only.

## Shared Selection Options

`sync` and `clone` accept both run-only selectors; `apply-group` accepts
`--skip-action` alone, and `apply-action` neither. Which command takes which, and
why, is in [`docs/cmdline.md`](../cmdline.md#selecting-what-a-run-does). Both
already take an [address](../cmdline.md#addresses), and a qualified one reaches
what an inclusion contributed. What is not built is the rest of that reach:

| Option                 | Comes to reach                                                  |
|------------------------|-----------------------------------------------------------------|
| `--skip-action <id>`   | An addressable clone-list entry, as well as an action.          |

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

- An addressable entry inside a `git-clone-list` becomes applyable by
  `<action-id>.<entry-id>` when that lookup exists.

That an `include-remote` is never directly applyable is built, and is specified
with [`apply-action`](../cmdline.md#apply-action).

### `vars set`, `vars get`, `vars list`, and `vars unset`

All four are built, and are specified in
[`docs/cmdline.md`](../cmdline.md#vars-set), dynamic variables and
`vars list --no-refresh` included. One question about them is open:

- A listing shows the leaf repository's flat set, which is
  [specified](../cmdline.md#vars-list) and deliberately leaves out what an
  inclusion's scope holds: those values hold inside one inclusion's records, and
  an action command's `-vv` output reports them. Whether a listing should grow a
  section per inclusion is open; such a section would have to read every
  inclusion this machine would open, which is work a listing does not do today.

`vars refresh`, below, is the rest of the family.

### `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Refresh selected dynamic variables, as `--refresh-vars` does for a run's. With
no keys, refresh the leaf repository's dynamic variables together with those of
every remote that is in the effective inclusion set, is allowed to run commands,
and is materialized.

What that inclusion set is belongs to this command. A run resolves a remote's
declarations only for an inclusion it [opens](../state.md#when-declarations-are-evaluated),
which a disable, a skip, or an apply command's target can prevent; a refresh has
no selection of its own, so it must decide whether those inclusions count. See
[reachability](state.md#reachability).

### `clone`

[`clone`](../cmdline.md#clone) is built, bootstrap adoption and its four enable
and disable options included, and is specified in
[`docs/cmdline.md`](../cmdline.md#clone). What it still refuses are the shared
action-execution options above, which are nothing to do with the bootstrap.

### The `init` skeleton

[`init`](../cmdline.md#init) is built. What it does not lay down yet is
`install.sh`, the bootstrap entry point a new machine runs before batfiles is on
it. That script is written by the installer work: see the roadmap's slice 10,
which owns both the template and the release URLs it downloads from. Until then
a new repository is synchronized with `batfiles sync`, and `init` creates no
script that only reports it cannot do that.

## Address Forms

Address syntax, and the four forms that resolve, are built and specified in
[`docs/cmdline.md`](../cmdline.md#addresses) — including the rule that an
unqualified name means the leaf repository alone, and that only items with the
required IDs can be addressed individually. These are the forms that need
something batfiles cannot yet contribute:

| Form                                  | Meaning                                               |
|---------------------------------------|-------------------------------------------------------|
| `<action-id>.<entry-id>`              | Addressable entry in a leaf `git-clone-list`.         |
| `<inclusion>.<action-id>.<entry-id>`  | Entry inside a list an inclusion contributed.         |

Each parses today and resolves to nothing, so what arrives is the *lookup*, not
the name.
