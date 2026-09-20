# Rewrite

Slices 0–7 provide local installation, selection, dry-run, fetching, Git clone
actions, variables and conditions, Git remote materialization, and one-level
remote inclusion: an `include-remote` contributes a remote's actions to the run's
list, they are addressed under it, its four selection filters say which of them
it takes, the conditions of what it contributed are decided against a scope
holding the remote's own `[vars]`, the leaf's, and the inclusion's overrides, and
the included manifest's own `[remotes]` is ignored. Slice 8 is nearly done:
`init` lays out a new leaf repository, and `clone` brings one down onto a
machine, adopts the bootstrap policy it declares, and synchronizes it. The
pristine-machine test is what remains. [steps.md](steps.md) tracks remaining
implementation work.

## Acceptance repositories

- **Personal:** symlinks, seeded copies, an HTTP download, and Git plugin lists.
- **Work:** the personal repository composed with a corporate repository
  reachable only from the work network. Remote inclusion assembles both under
  `sync`, which slice 7 accepted against the `leaf` and `corporate` fixtures.

## The documents

1. [guidance.md](guidance.md): implementation rules and workflow.
2. [steps.md](steps.md): numbered work and acceptance criteria.
3. [keep.md](keep.md): reference code available for remaining steps.
4. [docs.md](docs.md): documentation ownership and promotion.

`guidance.md` owns implementation shape during the rewrite and takes precedence
over `AGENTS.md` and `docs/`. Correct conflicting guidance in the same change.

## Retiring this directory

Retire `rewrite/` at slice 8. Slices 9 and 10 remain ordinary feature work.
[docs.md](docs.md#retirement-at-slice-8) specifies the document destinations and
required link and hygiene-check updates.
