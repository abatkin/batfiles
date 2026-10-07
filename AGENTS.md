# AGENTS.md

Guidance for agents and humans working in this repository. `CLAUDE.md` is a
symlink to this file.

## Project

Batfiles is a Rust dotfiles manager: one Cargo package producing the `batfiles`
binary. Its core dotfiles workflow is implemented and usable; potential additions
are tracked in [enhancements](docs/contributing/enhancements.md).
[`docs/contributing/architecture.md`](docs/contributing/architecture.md) owns implementation
design, including [source
organization](docs/contributing/architecture.md#source-organization) and [test
environments](docs/contributing/architecture.md#test-environments). Read it before writing
code.

## Documentation

The references in `docs/` specify implemented behavior and are published as the
user documentation. `docs/contributing/` holds the unpublished contributor
documents; its `enhancements.md` tracks potential future work and binds no
implementation. Read the owning
specification before changing behavior and update it in the same change.

| Subject | Owner |
| --- | --- |
| Product overview, quick start, supported features | Project `README.md` |
| Product principles | Project `README.md` |
| Command syntax and command-specific behavior | `docs/commands/` |
| Shared options, output, selection, dry-run, exit statuses | `docs/cmdline.md` |
| Environment parsing, location/color precedence, dynamic-command execution | `docs/environment.md` |
| Manifest schema, shared action fields, clone-list syntax | `docs/repoformat.md` |
| Action-specific fields and behavior | `docs/actions/` |
| Hosted installer and checkout stubs | `docs/installer.md` |
| Destination safety, seed installation, archive safety, Git update policy | `docs/safety.md` |
| State schemas, lifecycle, and atomic document replacement | `docs/state.md` |
| Release tree, release tasks, and the release workflow | `docs/contributing/distribution.md` |
| Cutting a release, and the repository settings it relies on | `dist/README.md` |
| Implementation design | `docs/contributing/architecture.md` |
| Filesystem-owner inventory | `tests/hygiene.rs` |
| Potential future enhancements | `docs/contributing/enhancements.md` |
| Branch workflow and canonical commands | This file |

Specify each rule once and link to its owner. Field tables may summarize a
shared policy with a link. API comments describe caller contracts, not the
history or justification of an implementation. When implementing an enhancement,
update the owning reference and remove the completed proposal. Keep development
history and design rationale in commit messages.

Follow [Writing documentation](docs/contributing/authoring.md) for guide and reference
structure, preserving coverage, and examples. User guides summarize and link to
the owning reference; they do not introduce another specification of its rules.

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
- Potential follow-up work is recorded in `docs/contributing/enhancements.md`.

Correctness and CI checks hold at every commit.

## Canonical commands

`Taskfile.yml` is the single source of truth. CI runs the same entry point.

- `task ci`: format, lint, test, documentation checks, dependency checks, debug and release builds.
- `task test`: project test suite; arguments after `--` go to `cargo test`.
- `task test:docker`: the pristine-machine acceptance, in a container. Part of
  `task ci` and not of `task test`; a machine with no working container runtime
  says so and passes.
- `task fmt`: formatting check.
- `task lint`: clippy with warnings denied, including the Windows target,
  `shellcheck` over the shell scripts, and PSScriptAnalyzer over the PowerShell
  scripts where `pwsh` is installed, failing if `pwsh` cannot load it.
- `task docs:install`: install the pinned mdBook and lychee on Linux or macOS.
  The documentation tasks and `task test` run it first.
- `task docs:check`: build and validate Markdown and HTML links.
- `task docs:serve`: preview documentation with live reload.
- `task build`: debug build.
- `task build:release`: release build.

Use the toolchain pinned in `rust-toolchain.toml`. Commit `Cargo.lock`.
