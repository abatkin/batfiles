# AGENTS.md

Guidance for AI agents (and humans) working in this repository. `CLAUDE.md` is
a symlink to this file.

## What batfiles is

A Rust-based dotfiles manager. The project is one Cargo package and produces
the `batfiles` binary.

## Where the documentation lives

Treat `docs/` as primary implementation guidance. It is plain markdown on
purpose so it reads as context.

Read the relevant spec before changing behavior. If a decision is settled,
honor it. When a behavior or design decision changes, update its owning
document in the same change.

## Source organization

Keep the implementation proportional to the tool:

- Use modules to group cohesive behavior inside the single crate.
- Keep clap definitions in `cli`, but command orchestration need not be an
  artificially thin composition layer.
- Preserve useful validated types and concrete planning structures. Keep pure
  calculations separate when that makes rules easier to understand and test.
- Prefer direct function calls and concrete types. Do not introduce capability
  traits, adapters, duplicate boundary types, or conversion layers without a
  current need that outweighs their cost.
- Direct filesystem, process, Git, clock, and network access is allowed in the
  module that owns the operation.
- On-disk and runtime representations may share a type when their shape and
  invariants agree.
- Use ordinary parameters, closures, fixtures, and temporary directories for
  tests. Add a trait only when it provides meaningful polymorphism or is the
  clearest practical test seam.
- Add modules when implemented behavior needs them; do not create speculative
  placeholder layers.

See [docs/architecture.md](docs/architecture.md) for the durable design
guidance.

## Canonical commands

`Taskfile.yml` (go-task) is the single source of truth. CI runs the identical
entry point, so what passes locally passes in CI.

- `task ci` — everything CI runs (fmt + lint + test + deny + build + build:release).
- `task test` — the project test suite.
- `task fmt` — formatting check.
- `task lint` — clippy with warnings denied.
- `task build` — build the project (debug).
- `task build:release` — build the project (release).

The toolchain is pinned in `rust-toolchain.toml`; do not bypass the pin.
Commit `Cargo.lock`.
