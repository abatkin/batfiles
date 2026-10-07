# `apply-action`

```text
batfiles apply-action --id <id> [--dry-run]
```

Execute one action or clone-list entry by [address](../cmdline.md#addresses), with `sync`'s
action behavior and the [shared execution options](../cmdline.md#shared-action-execution-options).
`--id` is required. An action without an ID cannot be named directly.

The [selection table](../cmdline.md#selection-by-command) defines which exclusions naming
an action bypasses. Apply commands use existing remote materializations.

These failures exit 1:

- An invalid address, checked before the repository is opened.
- An address matching nothing, with the address and manifest in the error.
- An unread enclosing inclusion or clone list, with its name and reason.
- An address naming an `include-remote` itself: it contributes actions but is
  not an executable action.

A contributed action excluded by inclusion filters still resolves, so naming it
succeeds without installing anything. `-v` identifies the record and reason:

```text
nothing to apply: the inclusion that contributed the action did not select it
```

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
