# AGENTS.md

Guidance for AI agents (and humans) working in this repository. `CLAUDE.md` is
a symlink to this file.

## What batfiles is

A Rust-based dotfiles manager. The binary is `batfiles`. The code
is organized as a Cargo workspace.

## Where the documentation lives

Treat `docs/` as primary implementation guidance. It is plain markdown on
purpose so it reads as context.

Read the relevant spec before changing behavior. If a decision is settled,
honor it; if you are changing a settled decision ask the user about updating the docs.

## Crate boundaries

The dependency direction always points **toward the domain**:

```
batfiles-cli  ──→  batfiles-config  ──→  batfiles-core
```

- `batfiles-core` — domain logic and domain types (the testable heart).
  Depends on nothing batfiles-specific. **No `serde`, no TOML, no XDG/`dirs`.**
- `batfiles-config` — `batfiles.toml` schema (`serde`), XDG path resolution,
  and the conversion layer mapping on-disk representation → core domain types.
  Depends on `batfiles-core`.
- `batfiles-cli` — argument parsing (clap), output formatting, the binary.
  Stays thin and delegates to core. Depends on both.

`batfiles-core` must **never** depend on `batfiles-config`.

Keep logic in the testable core, not tangled in argument parsing.

## Canonical commands

`Taskfile.yml` (go-task) is the single source of truth. CI runs the identical
entry point, so what passes locally passes in CI.

- `task ci` — everything CI runs (fmt + lint + test + deny + build + build:release).
- `task test` — the workspace test suite.
- `task fmt` — formatting check.
- `task lint` — clippy with warnings denied.
- `task build` — build the workspace (debug).
- `task build:release` — build the workspace (release).

The toolchain is pinned in `rust-toolchain.toml`; do not bypass the pin.
Commit `Cargo.lock`.
