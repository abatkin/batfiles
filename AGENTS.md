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
            └──→  batfiles-io      ──→  batfiles-core
```

- `batfiles-core` — domain logic and domain types (the testable heart):
  repository and action semantics, selection, conditions, variable precedence,
  refresh policy, include expansion, structural planning, and dynamic-variable
  resolver policy. It defines every outward-facing capability interface it
  needs and depends on nothing batfiles-specific. **No `serde`, no TOML, no
  XDG/`dirs`, and no direct I/O.**
- `batfiles-config` — the `batfiles.toml` and state-file schemas (`serde`),
  XDG path resolution, filesystem I/O for configuration documents, source-aware
  parse diagnostics, atomic state-file writes, and the conversion layer mapping
  on-disk representation → core domain types. Its documents are leaf and
  included `batfiles.toml`, the three state files, and `git-clone-list`
  manifests. Depends on `batfiles-core`.
- `batfiles-io` — reusable, non-configuration side effects: Git
  materialization, URL fetch, archive extraction, subprocess execution, and the
  clock. Plain reusable primitives plus thin adapters implementing core's
  capability interfaces. Decides no policy. Depends on `batfiles-core`.
- `batfiles-cli` — the composition root: argument parsing (clap), environment
  capture, output formatting, the binary. It constructs the concrete
  capabilities, invokes core, persists what core hands back, and renders
  results and diagnostics. Stays thin and delegates to core. Depends on all
  three.

`batfiles-core` must **never** depend on or name `batfiles-config` or
`batfiles-io`. Reads and effects core needs mid-computation are pull
capabilities it defines; terminal writes it has decided but need not observe
are pushed back to the CLI in the command outcome.

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
