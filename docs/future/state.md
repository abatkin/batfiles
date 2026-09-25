# Local state that is not built yet

`disabled.toml`, `vars.toml`, `dynamic-vars.toml`, and the shared write path are
built, and are specified in [`docs/state.md`](../state.md). What stays here is
the parts of `disabled.toml` that need more of remote inclusion to mean
anything.

## `disabled.toml`: the parts that are not built

The document, its schema, and the four commands that maintain it are specified
in [`docs/state.md`](../state.md), including the addresses both lists hold and
the rules a `sync` applies to them. Three things about it are still unbuilt.

**Resolving a qualified address.** An address naming an included remote's action
or group is recorded today and matches nothing, since no remote can contribute
one. What arrives with `include-remote` is the lookup that makes such an entry
live; see the [address forms](cmdline.md#address-forms) that need it.

**What a disable does to a remote.** How a disabled `include-remote` interacts
with materialization belongs to the step that builds it. One part is settled
here already: `disabled.toml` decides which actions are *planned* and never what
is fetched, so disabling an `include-remote` does not stop that remote being
materialized. A run does not open a disabled inclusion, so it resolves none of
that remote's [dynamic variables](../repoformat.md#dynamic-variables), and
neither does [`vars refresh`](../state.md#when-declarations-are-evaluated).

**Bootstrap adoption** is built, and is specified in
[`docs/state.md`](../state.md#bootstrap-adoption) along with what a `clone`
decides and what it leaves alone.
