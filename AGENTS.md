# AGENTS.md

Guidance for agents and humans working in this repository. `CLAUDE.md` is a
symlink to this file.

## Project

Batfiles is a Rust dotfiles manager: one Cargo package producing the `batfiles`
binary. [`docs/architecture.md`](docs/architecture.md) owns implementation
design, including [source
organization](docs/architecture.md#source-organization) and [test
environments](docs/architecture.md#test-environments). Read it before writing
code.

## Documentation

`docs/` specifies implemented behavior; `docs/future/` contains unbuilt proposals
and binds nothing. Product goals may describe intended scope when clearly
distinguished from supported behavior. Read the owning specification before
changing behavior and update it in the same change.

| Subject | Owner |
| --- | --- |
| Product overview, quick start, supported features | Project `README.md` |
| Product goals and intended scope | `docs/goals.md` |
| Command syntax, output, selection, dry-run, exit statuses | `docs/cmdline.md` |
| Environment parsing, location/color precedence, dynamic-command execution | `docs/environment.md` |
| Manifest schema, action fields, clone-list syntax | `docs/repoformat.md` |
| Destination safety, seed installation, archive safety, Git update policy | `docs/safety.md` |
| State schemas, lifecycle, and atomic document replacement | `docs/state.md` |
| Release tree, release tasks, and the release workflow | `docs/distribution.md` |
| Cutting a release, and the repository settings it relies on | `dist/README.md` |
| Implementation design | `docs/architecture.md` |
| Filesystem-owner inventory | `tests/hygiene.rs` |
| Remaining work and its acceptance | `docs/future/roadmap.md` |
| Branch workflow and canonical commands | This file |

Specify each rule once and link to its owner. Field tables may summarize a
shared policy with a link. API comments describe caller contracts, not the
history or justification of an implementation. Future documents contain only
remaining proposals and their dependencies.

Promote a section in the change implementing it: check it against the code,
rewrite discrepancies, and remove the duplicated proposal, leaving a link where
future work depends on the implemented behavior. Keep development history and
design rationale in commit messages.

## Workflow

Every change goes on a branch; never commit directly to `main`. Name a branch
for its step when applicable. Commit completed changes as you work, including
corrections.

**Merging is asked for, never assumed.** Finished work stops on its branch and
says so; review may still be owed, and merging is what forecloses it. Do not
merge because a step passed `task ci` or because the work reads as complete.

When a merge is requested, squash it. Describe the final behavior and rationale
in the squash message, without recounting the branch's intermediate work. Delete
the branch after merging unless instructed to keep it. A squash-merged branch may
require `git branch -D`.

### Definition of done

- `task ci` passes, including source-hygiene checks.
- A CLI test drives the behavior through the binary.
- Documentation for completed behavior is promoted and checked against the code.
- The project README, action-type inventories, and fixtures are current.
- Cross-document links and step references resolve.
- Implemented options are removed from the unsupported list.
- No dead-code expectation or carry marker names a completed step.
- Outstanding work is assigned to its future owner.

Within a slice, code may await a caller under [rule
1](docs/architecture.md#rules), an action may explicitly report that it is
unimplemented with a carry marker, and documentation may await completion of
that behavior. Correctness and CI checks hold at every commit.

### Carrying work forward

Keep completed step entries in
[`docs/future/roadmap.md`](docs/future/roadmap.md) to a short status line. Route
remaining material by its reader:

| Reader | Owner |
| --- | --- |
| A specific later step | That step's instruction |
| All implementation work | `docs/architecture.md` |
| Users of implemented behavior | Its owning document in `docs/` |
| Readers of unbuilt proposals | `docs/future/` |
| Readers of design rationale or history | The commit message |

Give actionable work a step or a named enhancement. Use
`// CARRY(<step>): <note>` for source reminders and `expect(dead_code)` for
unread items. `tests/hygiene.rs` checks carry syntax and step status in Rust
files under `src/` and `tests/`.

## Canonical commands

`Taskfile.yml` is the single source of truth. CI runs the same entry point.

- `task ci`: format, lint, test, dependency checks, debug and release builds.
- `task test`: project test suite.
- `task test:docker`: the pristine-machine acceptance, in a container. Part of
  `task ci` and not of `task test`; a machine with no working container runtime
  says so and passes.
- `task fmt`: formatting check.
- `task lint`: clippy with warnings denied, including the Windows target, and
  `shellcheck` over the shell scripts.
- `task build`: debug build.
- `task build:release`: release build.

Use the toolchain pinned in `rust-toolchain.toml`. Commit `Cargo.lock`.
