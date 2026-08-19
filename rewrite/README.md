# Rewrite

Batfiles is being rebuilt from scratch. The first implementation reached 15,385
lines of Rust without a working `sync`; roughly 5,250 of those lines were
unreachable from any command. The cause was building breadth-first from a
2,400-line specification, so that every noun in the spec became a type and no
caller ever existed to push back on a design.

## What it has to handle

Two real repositories are the target, and each is an acceptance test.

**Personal** — symlinks and seeded copies, plus oh-my-zsh fetched over HTTP and
vim plugins cloned from a manifest. Needs no variables, conditions, or remotes.
Step 4.8 retires the shell script that does this today.

**Work** — reachable only from the work network. Composes the personal
repository and a corporate-only one and assembles them, today by hand-written
shell scripts. Step 7.9 retires those. This repository is why remotes and
`include-remote` are the product rather than a nice-to-have.

Slices are ordered to reach those two as early as the dependencies allow, not by
ascending difficulty. That is why slice 4 is harder than slice 5 and comes first.

## The documents

Read them in this order:

1. [guidance.md](guidance.md) — the rules of the road, the reasoning behind the
   three decisions most likely to be gotten wrong (dry-run, variables,
   newtypes), and the five seams that keep the late slices additive. Standing
   instructions for the whole rewrite.
2. [steps.md](steps.md) — the order of work, as eleven vertical slices.
3. [keep.md](keep.md) — what to salvage from the old crate, and how each piece
   has to be adapted before it goes back in.
4. [docs.md](docs.md) — how `docs/` gets cut down, and the rule for growing it
   back.

Settled decisions live in the document they govern rather than in a register of
their own: why there is no `types/` module is in guidance.md, why `trace.rs` was
cut is in keep.md, which tag holds the old crate is in keep.md. If one of them
comes up again, the answer and its reasoning are next to the thing they affect.

`rewrite/guidance.md` is the sole owner of implementation shape for the
duration of the rewrite: step 0.1 deleted `docs/architecture.md`, which said the
same things less usefully, and guidance.md becomes the new `architecture.md` at
slice 8. It outranks `CLAUDE.md` meanwhile; when they disagree, follow
guidance.md and fix the other one.

## Retiring this directory

Slice 8 ends the rewrite. Slices 9 and 10 are ordinary feature work on a tool
that already works, so they do not need this scaffolding and should not keep it
alive. Retire `rewrite/` at slice 8 in three moves:

1. The durable rules in `guidance.md` move into `AGENTS.md`; the spent
   scaffolding does not. `docs.md` names both lists.
2. Steps 9.1 through 10.3 move to `docs/future/roadmap.md`, or to issues if you
   would rather track them there.
3. The directory is deleted. It stays reachable at the tag.
