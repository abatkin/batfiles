# Rewrite Guidance

Implementation rules for the rewrite. [steps.md](steps.md) owns the remaining
work; [docs.md](docs.md) owns documentation placement and promotion.

## Rules

**1. No code without a caller, by the end of the slice.** Never use
`allow(dead_code)`. An item waiting for a defined step in the same slice may use
`#[expect(dead_code, reason = "read at <step>")]`. Only fields of serde records
whose file format already requires them may name a later slice. Remove spent
expectations. The compiler and `tests/hygiene.rs` check expectations and their
step references; reviewers enforce the same-slice bound.

**2. Build vertical slices.** Every slice ends with usable command behavior and
a CLI test driving it. Add only the types and validation that behavior needs.

**3. Refactor when there are real callers.** Later slices may reshape earlier
ones. Wait for three instances before extracting shared behavior, unless a
current correctness requirement warrants it.

**4. Comments describe use and contracts.** Document inputs, outputs, side
effects, errors, and invariants a caller needs. Keep short inline notes for
non-obvious implementation constraints. Put design rationale and rejected
alternatives in commit messages. Do not retain development history in source
comments or documentation.

**5. Keep errors concrete.** Start with the crate error enum and derive its
messages with `thiserror`. Variants carry facts, with prose in `#[error]`;
subprocess diagnostics may carry captured output. Section a growing enum first.
Introduce a subsystem error when callers match it or three or more failures
share subsystem-specific vocabulary. Put it beside that subsystem. Keep
cross-cutting read and write failures shared. Use hand-written `Display` only
where rendering requires it.

**6. Shell out to Git.** Use the user's executable, configuration, credentials,
and SSH agent. Keep subprocess launch and environment handling in `git.rs`.
The supported environment is specified in
[environment.md](../docs/environment.md#variables-passed-on-to-git).

**7. Dry-run never simulates a filesystem.** Use the same action implementations
in both modes, with writes gated at helpers. See [Dry-run](#dry-run).

**8. Validate against real dotfiles.** Maintain the personal repository acceptance
and use the corporate composition as the acceptance for remote inclusion.

**9. Document implemented behavior.** Follow [docs.md](docs.md). Unbuilt behavior
belongs in `docs/future/` and does not bind implementation.

**10. Prefer CLI tests.** Use unit tests for tricky calculations, parsing,
validation, and safety invariants that need a direct test seam. Use ordinary
parameters, closures, fixtures, and temporary directories; add traits only for
useful polymorphism or a clearer practical test seam.

**11. Serve the configuration.** Installation locations required by a user's
configuration are requirements for batfiles to support.

**12. Reject parsed but unimplemented options.** Keep the list in
`src/cli/unsupported.rs`, checked at dispatch before root resolution. Diagnostics
name the option and implementation step. The hygiene test rejects references to
undefined or completed steps. Stubbed values may remain strings until their
validation has a caller.

**13. Preserve existing user content.** Apply the destination policy in
[safety.md](../docs/safety.md). Cleanup removes only paths created by the current
operation. Report paths that cannot be safely replaced.

**14. Centralize path resolution.** Anchor roots before storing symlink targets.
Resolve existing targets relative to their link's directory for classification.
Keep repository source resolution in `RunContext` and filesystem classification
in `paths.rs`. Use the documented distinction between lexical path construction
and filesystem target resolution.

**15. Install complete content.** Build seeds in staging and publish after
completion. Create staging nodes privately and apply final permissions only
when content is complete. Cleanup must not be required for correctness after
interruption. Git clones use destination validation to reject incomplete
checkouts on subsequent runs. State documents use their own atomic replacement
path; their replacement policy differs from seeds.

## Source organization

- One Cargo package, with cohesive modules inside the crate.
- Keep clap definitions in `cli`; orchestration may live with command behavior.
- Prefer direct calls and concrete types. Avoid speculative adapters, capability
  traits, placeholder modules, and duplicate boundary representations.
- On-disk and runtime types may be shared when their shape and invariants agree.
- Keep validated types near their users. A validated string generally needs only
  parsing, serialization support, and display. Add conversion traits as needed.
- Modules may access the filesystem or processes when they own the operation;
  register that ownership in `tests/hygiene.rs`.

## Budgets

Guidelines for review, not hard limits:

- A validated string newtype and its error: about 40 lines.
- A module doc comment: about five lines.
- A slice: reviewable in one sitting.

## How a slice lands

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

Do not merge from the salvage tag in [keep.md](keep.md).

## Definition of done for a slice

- `task ci` passes, including source-hygiene checks.
- A CLI test drives the behavior through the binary.
- Documentation for completed behavior is promoted and checked against the code.
- The project README, action-type inventories, and fixtures are current.
- Cross-document links and step references resolve.
- Implemented options are removed from the unsupported list.
- No dead-code expectation or carry marker names a completed step.
- Outstanding work is assigned to its future owner.

Between steps in one slice, code may await a caller under rule 1, an action may
explicitly report that it is unimplemented with a carry marker, and documentation
may await completion of that behavior. Correctness and CI checks hold at every
commit.

## Carrying work forward

Keep completed step entries to a short status line. Route remaining material by
its reader:

| Reader | Owner |
| --- | --- |
| A specific later step | That step's instruction |
| All implementation work | This guidance |
| Users of implemented behavior | Its owning document in `docs/` |
| Readers of unbuilt proposals | `docs/future/` |
| Readers of design rationale or history | The commit message |

Give actionable work a step or a named enhancement. Use
`// CARRY(<step>): <note>` for source reminders and `expect(dead_code)` for
unread items. `tests/hygiene.rs` checks carry syntax and step status in Rust
files under `src/` and `tests/`.

## Test environments

Use temporary roots, local bare Git repositories, and loopback HTTP servers.
Tests must not contact external services. Use reserved example domains where a
host is named but must never be contacted. Fixture Git commands must use an
isolated environment and configuration; production Git behavior is tested
through the binary with explicit test inputs.

Keep `tests/cli/` as one test target. Group tests by behavior and shared support
by responsibility. Keep realistic fixture repositories separate when they need
different environments. Snapshot whole trees for dry-run assertions and also
check direct evidence of work, such as HTTP request counts or `FETCH_HEAD`.

Gate platform-specific execution tests together where practical. Windows
compilation is checked by `task lint`; it is not a Windows runtime test. The
pinned toolchain and [Taskfile](../Taskfile.yml) own toolchain setup and checks.
Docker tests for pristine-machine behavior belong in a separate task, added at
8.4 and extended at 10.3.

## Dry-run

Every action inspects the real filesystem and performs or reports its work.
Keep `RunMode` checks at write helpers, after the inspection needed to report
intent. Seed builders are never invoked in dry-run mode; Git is never launched.
Actions must not implement an alternate dry-run path or simulate earlier writes.

The user-visible contract, including source validation, shared destinations,
per-child output, and reporting limits, is owned by
[cmdline.md](../docs/cmdline.md#dry-run-behavior). Batfiles bookkeeping may run in
both modes; document additional bookkeeping only when implemented.

### Filesystem ownership checks

`FILESYSTEM_OWNERS` in `tests/hygiene.rs` is the sole module inventory. Each
entry records one role: read-only inspection, a mode reader, content production
downstream of a mode reader, or bookkeeping.

The test checks recognized filesystem/process import spellings against that
list and checks that listed files exist. It does not parse Rust, prove that an
owner is read-only, or verify call paths and mode gating. Those properties need
code review and behavioral tests. Keep the scanner small.

## Variables

Variables feed `when` and `unless`; they do not interpolate paths or strings.
Slice 5 resolved the four sources into one flat scope: one namespace, in which
a name resolves the same way whatever declared it. The representation is not
prescribed; the seam below is. Add per-inclusion scopes only with remote
inclusion at 7.5. Conditions and precedence are specified in
[`docs/repoformat.md`](../docs/repoformat.md#conditions) and
[`docs/environment.md`](../docs/environment.md#variable-precedence); the
dynamic-variable proposals stay in `docs/future/` until implemented.

## Seams the late slices need

Keep each decision centralized without building future abstractions:

1. Effective variable values and their origins are produced by one function.
2. Run settings are carried by `RunContext`; write helpers consult the mode.
3. Execution captures selection once, prepares selected clone lists, and uses
   one loop over those action positions.
4. Repository source paths use one resolver.
5. The manifest owns declaration order. Inclusion must finish expanding it
   before selection and clone-list preparation.
