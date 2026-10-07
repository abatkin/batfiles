# Enable and disable actions or groups

```text
batfiles disable-action <id>...
batfiles enable-action <id>...
batfiles disable-group <group>...
batfiles enable-group <group>...
```

Add or remove persistent [addresses](../cmdline.md#addresses) in `disabled.toml`. These
commands load no repository, run no synchronization, and remove no installed
content. Unknown names are accepted; malformed addresses exit 1 before opening
the document or applying any supplied name. Repeated names warn and apply once.

One diagnostic per name reports whether state changed:

```text
disabled action `p10k`
action `zshrc` was already disabled
```

`--quiet` suppresses reports, not edits. See [state lifecycle](../state.md#semantics-and-lifecycle)
for idempotence and persistence.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
