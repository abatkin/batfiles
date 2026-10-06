# AGENTS.md

Guidance for agents and humans working in this repository. `CLAUDE.md` is a
symlink to this file.

## Project

Batfiles is a Rust dotfiles manager: one Cargo package producing the `batfiles`
binary. Its core dotfiles workflow is implemented and usable; potential additions
are tracked in [enhancements](docs/enhancements.md).
[`docs/architecture.md`](docs/architecture.md) owns implementation
design, including [source
organization](docs/architecture.md#source-organization) and [test
environments](docs/architecture.md#test-environments). Read it before writing
code.

## Documentation

The references in `docs/` specify implemented behavior. `docs/enhancements.md`
tracks potential future work and binds no implementation. Read the owning
specification before changing behavior and update it in the same change.

| Subject | Owner |
| --- | --- |
| Product overview, quick start, supported features | Project `README.md` |
| Product principles | Project `README.md` |
| Command syntax, output, selection, dry-run, exit statuses | `docs/cmdline.md` |
| Environment parsing, location/color precedence, dynamic-command execution | `docs/environment.md` |
| Manifest schema, action fields, clone-list syntax | `docs/repoformat.md` |
| Destination safety, seed installation, archive safety, Git update policy | `docs/safety.md` |
| State schemas, lifecycle, and atomic document replacement | `docs/state.md` |
| Release tree, release tasks, and the release workflow | `docs/distribution.md` |
| Cutting a release, and the repository settings it relies on | `dist/README.md` |
| Implementation design | `docs/architecture.md` |
| Filesystem-owner inventory | `tests/hygiene.rs` |
| Potential future enhancements | `docs/enhancements.md` |
| Branch workflow and canonical commands | This file |

Specify each rule once and link to its owner. Field tables may summarize a
shared policy with a link. API comments describe caller contracts, not the
history or justification of an implementation. When implementing an enhancement,
update the owning reference and remove the completed proposal. Keep development
history and design rationale in commit messages.

## Workflow

Every change goes on a branch; never commit directly to `main`. Name the branch
for the change. Commit completed changes as you work, including corrections.

**Merging is asked for, never assumed.** Finished work stops on its branch and
says so; review may still be owed, and merging is what forecloses it. Do not
merge because a change passed `task ci` or because the work reads as complete.

When a merge is requested, squash it. Describe the final behavior and rationale
in the squash message, without recounting the branch's intermediate work. Delete
the branch after merging unless instructed to keep it. A squash-merged branch may
require `git branch -D`.

### Definition of done

- `task ci` passes, including source-hygiene checks.
- Behavior changes have a CLI test driving them through the binary.
- Documentation matches the implemented behavior.
- The project README, action-type inventories, and fixtures are current.
- Cross-document links resolve.
- Accepted CLI options are implemented, and no unused scaffolding is committed.
- Potential follow-up work is recorded in `docs/enhancements.md`.

Correctness and CI checks hold at every commit.

## Canonical commands

`Taskfile.yml` is the single source of truth. CI runs the same entry point.

- `task ci`: format, lint, test, dependency checks, debug and release builds.
- `task test`: project test suite; arguments after `--` go to `cargo test`.
- `task test:docker`: the pristine-machine acceptance, in a container. Part of
  `task ci` and not of `task test`; a machine with no working container runtime
  says so and passes.
- `task fmt`: formatting check.
- `task lint`: clippy with warnings denied, including the Windows target,
  `shellcheck` over the shell scripts, and PSScriptAnalyzer over the PowerShell
  scripts where `pwsh` is installed, failing if `pwsh` cannot load it.
- `task build`: debug build.
- `task build:release`: release build.

Use the toolchain pinned in `rust-toolchain.toml`. Commit `Cargo.lock`.
