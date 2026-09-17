# Rewrite

Slices 0–6 provide local installation, selection, dry-run, fetching, Git clone
actions, variables and conditions, and Git remote materialization. Slice 7 is
under way: an `include-remote` contributes a remote's actions to the run's list,
they are addressed under it, its four selection filters say which of them it
takes, and the conditions of what it contributed are decided against a scope
holding the remote's own `[vars]`, the leaf's, and the inclusion's overrides.
What remains is the included `[remotes]` decision, stable inclusion labels, and
the fixtures and acceptance. [steps.md](steps.md) tracks remaining
implementation work.

## Acceptance repositories

- **Personal:** symlinks, seeded copies, an HTTP download, and Git plugin lists.
- **Work:** compose the personal repository with a corporate repository reachable
  only from the work network. Remote inclusion must assemble both under `sync`.

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
