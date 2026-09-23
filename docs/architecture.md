# Batfiles Architecture

Implementation design: the rules the code is written to, how the crate is
organized, and what its tests look like. User-visible behavior is specified by
the documents listed in [the index](README.md); where a rule here concerns
something a user can observe, it links to the document that owns it rather than
restating it.

## Rules

**1. No code without a caller, by the end of the slice.** Never use
`allow(dead_code)`. An item waiting for a defined step in the same slice may use
`#[expect(dead_code, reason = "read at <step>")]`. Only fields of serde records
whose file format already requires them may name a later slice. Remove spent
expectations. The compiler and `tests/hygiene.rs` check expectations and their
step references; reviewers enforce the same-slice bound.

**2. Build vertical slices.** Every slice ends with usable command behavior and
a CLI test driving it. Add only the types and validation that behavior needs.

**3. Refactor when there are real callers.** Later work may reshape earlier
work. Wait for three instances before extracting shared behavior, unless a
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
[environment.md](environment.md#variables-passed-on-to-git).

**7. Dry-run never simulates a filesystem.** Use the same action implementations
in both modes, with writes gated at helpers. See [Dry-run](#dry-run).

**8. Validate against real dotfiles.** Maintain the personal repository
acceptance and use the corporate composition as the acceptance for remote
inclusion. Both are [fixture repositories](#acceptance-repositories).

**9. Document implemented behavior.** Follow the ownership rules in
[AGENTS.md](../AGENTS.md#documentation). Unbuilt behavior belongs in
`docs/future/` and does not bind implementation.

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
[safety.md](safety.md). Cleanup removes only paths created by the current
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

### Execution

`sync`, `clone`'s bootstrap, and the apply commands share one pipeline:

| Module | Owns |
| --- | --- |
| `execute/mod.rs` | Command entry points, phase order, clone-list preparation, and the execution loop. |
| `execute/record.rs` | The run's list: each record's identity, provenance, heading, scope, and disposition. |
| `execute/assemble.rs` | Expansion and selection in one pass, and each opened inclusion's scope. |
| `execute/inclusion.rs` | An inclusion's identity, filters, and the reading of its manifest. |
| `selection.rs` | Targets and exclusions, applied to run records. |
| `action/` | Dispatch of prepared install records. |

A run proceeds in this order:

1. Load the leaf manifest, resolve variables, and capture host inputs.
2. For `clone`, [adopt](state.md#bootstrap-adoption) the bootstrap policy,
   which writes `disabled.toml` before it is read.
3. Capture the selection, decide remote conditions, and build the `RunContext`.
4. For `sync` and `clone`, [materialize](repoformat.md#materialization) remotes.
   Apply commands use the trees already present.
5. Assemble and select, opening only reached, admitted
   [inclusions](repoformat.md#include-remote).
6. Warn about unmatched skips and [prepare](cmdline.md#clone-list-preparation)
   every selected clone list.
7. Walk the list once: report exclusions, print admitted inclusions' headings,
   and dispatch every other admitted record.

Steps 2 and 4 can write before assembly or preparation fails. Preparation
precedes every action's writes, not every write in the command. Only
dispatched records count as applied work; an inclusion never reaches the
dispatcher. The user-visible rules are specified in
[selection by command](cmdline.md#selection-by-command) and
[execution failures](cmdline.md#execution-failures). Dry runs follow the same
pipeline; see [Dry-run](#dry-run).

## Budgets

Guidelines for review, not hard limits:

- A validated string newtype and its error: about 40 lines.
- A module doc comment: about five lines.
- A slice: reviewable in one sitting.

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

### Acceptance repositories

Two fixture repositories stand in for real dotfiles under rule 8. The personal
one, `tests/fixtures/leaf`, holds symlinks, seeded copies, and a tree with files
no action names. The work one is that repository composed with
`tests/fixtures/corporate`, a repository reachable only from a corporate
network, which remote inclusion assembles under `sync`.

### The pristine machine

What only a whole machine can answer belongs in `tests/docker/`, which
`task test:docker` builds and runs and `task ci` includes. The tests under
`tests/cli/` pin all four roots at a temporary directory, so the container is
where batfiles decides for itself: a real `$HOME`, XDG defaults, and no state of
any kind. Keep it to acceptance -- one scenario end to end, over the fixture
repositories above, with the container-only additions to a manifest in an
overlay beside the Dockerfile.

The image runs a Linux binary whatever the machine running the test is: a Linux
host hands over the one `task build` produced, and a host that builds something
a container cannot execute has the image build batfiles itself, against the
pinned toolchain. The task is part of `task ci`, so it must not fail for being
run somewhere unusual -- a host with no working container runtime reports that
it did not run.

## Dry-run

Every action inspects the real filesystem and performs or reports its work.
Keep `RunMode` checks at write helpers, after the inspection needed to report
intent. Seed builders are never invoked in dry-run mode; Git is never launched.
Actions must not implement an alternate dry-run path or simulate earlier writes.

The user-visible contract, including source validation, shared destinations,
per-child output, and reporting limits, is owned by
[cmdline.md](cmdline.md#dry-run-behavior). Batfiles bookkeeping may run in both
modes; document additional bookkeeping only when implemented.

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
The four sources resolve into one flat scope: one namespace, in which a name
resolves the same way whatever declared it. The representation is not
prescribed; the seam below is. An opened inclusion derives a second scope from
that one, holding its `vars` overrides and the included remote's own `[vars]`: a
scope is derived once per opened inclusion, carried by the records it
contributed, and every condition on a record is decided against the scope that
record holds. Conditions and precedence are specified in
[repoformat.md](repoformat.md#conditions) and
[environment.md](environment.md#variable-precedence); the dynamic-variable
proposals stay in `docs/future/` until implemented.

## Centralized decisions

Keep each of these in one place, without building future abstractions:

1. Effective variable values and their origins are produced by one function.
2. Run settings are carried by `RunContext`; write helpers consult the mode.
3. Execution captures selection once, prepares selected clone lists, and uses
   one loop over the run's list.
4. Repository source paths use one resolver, which takes the tree a record came
   from.
5. The manifest owns declaration order; `execute` assembles the run's list from
   it, expanding every inclusion it reaches. Assembly and selection are one
   pass, and both finish before clone-list preparation. The assembled list is
   not a simulation and never becomes one: it is the same records, read from
   more than one file. See [Dry-run](#dry-run).
