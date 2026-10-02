# Batfiles Command-Line Surface

A compact inventory of the command-line interface that is not built yet.

The command overview, the global and shared options, the output streams, and
the exit statuses are built, and are specified in
[`docs/cmdline.md`](../cmdline.md). Everything below is intended behavior and
binds nothing.

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

[`vars refresh`](../cmdline.md#vars-refresh), the rest of the family, is built
too.

### `clone`

[`clone`](../cmdline.md#clone) is built, bootstrap adoption, its four enable
and disable options, and the shared action-execution options it forwards
included, and is specified in [`docs/cmdline.md`](../cmdline.md#clone).

### `update`

[`update`](../cmdline.md#update) is built and specified in
[`docs/cmdline.md`](../cmdline.md#update).

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
