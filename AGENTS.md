# AGENTS.md

Guidance for agents and humans working in this repository. `CLAUDE.md` is a
symlink to this file.

## Rewrite status

Read [rewrite/README.md](rewrite/README.md) first. During the rewrite,
`rewrite/` is authoritative over this file and `docs/`. Its
[guidance](rewrite/guidance.md) owns implementation design and its
[documentation rules](rewrite/docs.md) own documentation placement.

## Project

Batfiles is a Rust dotfiles manager: one Cargo package producing the `batfiles`
binary. Source organization is specified in
[rewrite/guidance.md](rewrite/guidance.md#source-organization).

## Documentation

`docs/` specifies implemented behavior; `docs/future/` contains unbuilt proposals
and binds nothing. Read the owning specification before changing behavior and
update it in the same change. Keep each rule in its owner and link from other
documents. Keep development history and design rationale in commit messages.

## Workflow

Follow [How a slice lands](rewrite/guidance.md#how-a-slice-lands): work on a
branch, commit completed changes, squash when merging to `main`, and remove the
branch after merging unless instructed otherwise.

## Canonical commands

`Taskfile.yml` is the single source of truth. CI runs the same entry point.

- `task ci`: format, lint, test, dependency checks, debug and release builds.
- `task test`: project test suite.
- `task fmt`: formatting check.
- `task lint`: clippy with warnings denied, including the Windows target.
- `task build`: debug build.
- `task build:release`: release build.

Use the toolchain pinned in `rust-toolchain.toml`. Commit `Cargo.lock`.
