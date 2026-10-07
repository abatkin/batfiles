# `apply-group`

```text
batfiles apply-group --group <group> [--skip-action <id>]... [--dry-run]
```

Execute a group's actions in declaration order, with `sync`'s action behavior
and the [shared execution options](../cmdline.md#shared-action-execution-options).
`--group` is required; `--skip-action` is repeatable. See
[selection by command](../cmdline.md#selection-by-command) for member exclusions.

An invalid or unknown group exits 1; a group exists only when an action names
it. A qualified group in an unread inclusion fails naming that inclusion.
An existing group with no eligible actions exits 0 and reports:

```text
nothing to apply: every action in the group is disabled, skipped, excluded by its own condition, or was not contributed
```

`-v` gives individual reasons. Inclusions count only through the actions they
contribute, so a group containing an inclusion that contributes nothing also
has nothing to apply.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
