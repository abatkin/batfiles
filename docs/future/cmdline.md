# Batfiles Command-Line Surface

A compact inventory of the command-line interface that is not built yet.

The command overview, the global and shared options, the output streams, and
the exit statuses are built, and are specified in
[`docs/cmdline.md`](../cmdline.md). Everything below is intended behavior and
binds nothing.

## Commands

### `apply-action` and `apply-group`

Both are built, and are specified in
[`docs/cmdline.md`](../cmdline.md#apply-action), including naming a single
clone-list entry and the rule that an `include-remote` is never directly
applyable.

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

Address syntax and every form that resolves, clone-list entries included, are
built and specified in [`docs/cmdline.md`](../cmdline.md#addresses).
