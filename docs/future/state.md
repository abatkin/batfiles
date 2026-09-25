# Local state that is not built yet

`disabled.toml`, `vars.toml`, `dynamic-vars.toml`, and the shared write path are
built, and are specified in [`docs/state.md`](../state.md). What stays here is
what `vars refresh` needs from them, and the parts of `disabled.toml` that need
more of remote inclusion to mean anything.

## `vars refresh` and `vars.toml`

**`vars refresh`** reads `vars.toml`, but only as an input to
[reachability](#reachability). It evaluates the `[remotes]` and `include-remote`
gates to decide which remotes are in play; those gates read the leaf scope, and
machine-local values are one of that scope's layers. A `vars refresh` that
skipped this file could refresh a different set of remotes than the `sync` it is
meant to prepare for. The file has no other role here: dynamic command
arguments are not interpolated, and — unlike `vars list` — a machine-local value
does not suppress the refresh of the declaration it shadows.

If neither the leaf nor any remote in play declares a dynamic variable, the
command does not load, create, or rewrite the cache or its directory.

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
that remote's [dynamic variables](../repoformat.md#dynamic-variables); what
`vars refresh` does is [reachability](#reachability)'s question.

**Bootstrap adoption** is built, and is specified in
[`docs/state.md`](../state.md#bootstrap-adoption) along with what a `clone`
decides and what it leaves alone.

## Reachability

A run resolves the leaf's [dynamic variables](../repoformat.md#dynamic-variables),
and a remote's only for an inclusion it opens: one whose own gate and whose
remote's gate both pass against the leaf scope, that the selection admits, and
whose remote is materialized and [allowed](../repoformat.md#git) to run
commands. That is [specified](../state.md#when-declarations-are-evaluated) and
built. What is proposed here is the set `vars refresh` refreshes, which has no
selection of its own to open inclusions with.

- Every declaration in the leaf repository's `[vars]` is in play.
- A remote's declarations are in play when an `include-remote` names it and
  **both** gates pass: the action's own `when`/`unless`, and the
  `when`/`unless` on the leaf's `[remotes]` entry. Either closing excludes that
  inclusion, and a remote no surviving inclusion names is not in play.
- Both records are *leaf* records, so both evaluate against the **leaf scope**,
  which needs nothing but the leaf repository on disk. The layering is what
  keeps reachability acyclic: nothing in the leaf scope depends on any remote,
  and a remote's variable only ever affects scopes inside its own inclusion.
- An inclusion's own `vars` overrides do not participate in its gate.
- Declarations resolve before the conditions that read them: the leaf's run
  first, the leaf scope decides which remotes are in play, and only those
  remotes' declarations run after that.
- A remote excluded by a condition runs none of its commands and writes no cache
  entry, even with `allow-dynamic-vars = true`.
- **Open:** whether a disabled or skipped inclusion is in play. A run does not
  open one, so refreshing it prepares for no run this machine would make; but
  `disabled.toml` decides what is planned rather than what is fetched, and a
  refresh that ignored it would match the materialization `sync` keeps.

A remote's manifest is read at the point that remote is known to be in play,
after the leaf's commands have run. A malformed manifest in a remote in play is
still fatal before anything in that remote runs; a remote not in play is never
read at all.
