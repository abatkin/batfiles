# Batfiles architecture

Implementation design: the rules the code is written to, how the crate is
organized, and what its tests look like. User-visible behavior is specified by
the documents listed in [the index](README.md); where a rule here concerns
something a user can observe, it links to the document that owns it rather than
restating it.

## Rules

**1. No code without a caller.** Do not commit unused scaffolding or suppress
`dead_code` with `allow` or `expect`. Add code with the behavior that uses it;
clippy denies unused items, and `tests/hygiene.rs` rejects any mention of
`dead_code` under `src/` and `tests/`.

**2. Build complete behavior.** A behavior change includes its callers,
documentation, and a CLI test driving it through the binary.

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
and SSH agent. Keep Git's subprocess launch and environment handling in
`git.rs`. The supported environment is specified in
[environment.md](environment.md#variables-passed-on-to-git). The only other
subprocesses are a dynamic variable's command, launched in `dynamic/run.rs`, and
in `update.rs` the `version` of a release `update` has downloaded and, on
Windows, the hidden PowerShell that removes the executable it set aside.

**7. Dry-run never simulates a filesystem.** Use the same action implementations
in both modes, with writes gated at helpers. See [Dry-run](#dry-run).

**8. Validate against real dotfiles.** Maintain the personal repository
acceptance and use the corporate composition as the acceptance for remote
inclusion. Both are [fixture repositories](#acceptance-repositories).

**9. Document implemented behavior.** Follow the ownership rules in
[AGENTS.md](../AGENTS.md#documentation). Potential future work belongs in
[enhancements.md](enhancements.md) and does not bind implementation.

**10. Prefer CLI tests.** Use unit tests for tricky calculations, parsing,
validation, and safety invariants that need a direct test seam. Use ordinary
parameters, closures, fixtures, and temporary directories; add traits only for
useful polymorphism or a clearer practical test seam.

**11. Serve the configuration.** Installation locations required by a user's
configuration are requirements for batfiles to support.

**12. Honor every accepted option.** Add CLI options with their implementation.
Do not accept options that silently do nothing or promise unbuilt behavior.

**13. Preserve existing user content.** Apply the destination policy in
[safety.md](safety.md). Settle an unmanaged node through `replace.rs`, which
owns the conflict policy, backups, and putting a node back; refuse one at a
tool-owned destination. Cleanup removes only paths created by the current
operation.

**14. Centralize path resolution.** Anchor roots before storing symlink targets.
Resolve existing targets relative to their link's directory for classification.
Keep repository source resolution in `RunContext` and filesystem classification
in `paths.rs`. Use the documented distinction between lexical path construction
and filesystem target resolution. Separate inspecting a node from deciding
whether to replace it: a caller that needs only what is at a path uses
`paths::symlink_metadata_if_present`, which never reads a symlink's target;
only a caller that may replace a symlink uses `Occupancy::at`.

**15. Install complete content.** Build seeds in staging and publish after
completion; a refresh builds and compares complete content before it sets
anything aside. Create staging nodes privately and apply final permissions only
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

### Vocabulary

Code names follow these terms beside the user-facing ones:

- **Action**: a configured operation, as a manifest declares it. The everyday
  word, since users write `[[actions]]` and run `apply-action`.
- **Record**: an action's representation in a run, and its execution
  bookkeeping (`RunRecord`, `IncludedRecord`, `RecordName`).
- **Item**: only what actions and groups share (`ItemId`, `ItemAddress`).
  `ItemKind` says which of the two a name refers to, and displays as the word
  output uses for it.
- **Node**: whatever filesystem entry occupies a path, as
  [safety.md](safety.md#path-resolution) defines it. A manifest entry is never
  a node.
- **Contributor**: the manifest a record came from, the leaf's or an
  inclusion's, which decides how its names are qualified.
- **Disposition**: what a run does with one record or clone-list entry: not
  requested, excluded, allowed, or allowed in part — an inclusion or a list
  opened only because the target names something inside it.
- **Subject**: what selection decides a record or an entry on: its addresses and
  its gate.

### Execution

`sync`, `clone`'s bootstrap, and the apply commands share one pipeline:

| Module | Owns |
| --- | --- |
| `execute/mod.rs` | Command entry points, phase order, clone-list preparation, and the execution loop. |
| `execute/record.rs` | The run's list: each record's identity, heading, and disposition, and each inclusion's ownership of the records it contributed, with their scope. |
| `execute/assemble.rs` | Expansion and selection in one pass, and each opened inclusion's scope. |
| `execute/inclusion.rs` | What an opened inclusion contributes: its filters' verdicts, dropped nested inclusions, and the warnings composing it gives. |
| `inclusion.rs` | An inclusion's identity, whether this machine opens it, and reading its manifest from a materialization. Shared with `vars refresh`. |
| `dynamic/` | Running dynamic variables' commands, and their cache. |
| `selection.rs` | Targets and exclusions, decided on a record's or a clone-list entry's addresses and condition. |
| `action/` | Dispatch of prepared install records. |

A run proceeds in this order:

1. Load the leaf manifest, resolve variables — running the leaf's dynamic
   variables and saving what they captured — and capture host inputs.
2. For `clone` and `sync --bootstrap`, [adopt](state.md#bootstrap-adoption)
   the bootstrap policy, which writes `disabled.toml` before it is read, or
   under `--dry-run` hands the run the lists it would have written.
3. Capture the selection, decide remote conditions, and build the `RunContext`.
4. For `sync` and `clone`, [materialize](repoformat.md#materialization) remotes.
   Apply commands use the trees already present.
5. Assemble and select, opening only reached, admitted
   [inclusions](repoformat.md#include-remote) and resolving each opened
   remote's dynamic variables, if the leaf allows them.
6. [Prepare](cmdline.md#clone-list-preparation) every selected clone list,
   and every list the target reaches into for one entry, deciding each entry.
   Then warn about unmatched skips, and fail an apply command whose target
   names nothing, before any action writes.
7. Walk the list once in declaration order: report exclusions and dispatch
   admitted records. An admitted inclusion prints its heading and then walks
   the records it contributed, reading their repository paths from its
   remote's materialization.

Steps 1, 2, 4, and 5 can write before assembly or preparation fails. Preparation
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
- A change: reviewable in one sitting.

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

The release scripts under `dist/` are tested by the `tests/dist/` target, which
runs them with `sh` against stand-in binaries and `file://` or loopback release
trees, and on Windows runs the PowerShell installer and stub with `pwsh` over
loopback HTTP. Building real binaries and publishing them are left to the
release workflow. Where a stand-in must be a real executable, as on Windows, the
tests compile one from `tests/common/` once per run, and it reads the version it
plays from a trailer appended to its own file.

Gate platform-specific execution tests together where practical; a test that
needs symlink actions or a Unix shell is Unix-only. CI runs `task ci` on Linux
and `task test` on macOS and on Windows, so the test suite, including the
installers' tests, also runs against BSD tools, macOS's shell, and Windows
itself. The pinned toolchain and [Taskfile](../Taskfile.yml) own toolchain setup
and checks.

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
any kind. Keep it to acceptance -- end-to-end scenarios over the fixture
repositories above, with the container-only additions to a manifest in an
overlay beside the Dockerfile. Each scenario runs in a fresh container of the
same image: `scenario.sh` sets a machine up with the hosted installer's `clone`
one-liner, and `stub-scenario.sh` with a plain `git clone` and the
[leaf stub](distribution.md#leaf-stub) the leaf fixture carries.

The machine starts with no batfiles. The image assembles its binary into a
release tree under `/srv/releases` with `dist/assemble.sh`, named as the
[hosted installer](distribution.md#hosted-installer) asks for it on that
architecture, and each scenario gets batfiles from there, the one-liner's under
`dash`. The binary is a Linux one whatever the machine running the test is: a
Linux host hands over the one `task build` produced, and a host that builds
something a container cannot execute has the image build batfiles itself,
against the pinned toolchain. Because the task is part of `task ci`, a host with
no working container runtime reports that it did not run rather than failing.

## Dry-run

Every action inspects the real filesystem and performs or reports its work.
Keep `RunMode` checks at write helpers, after the inspection needed to report
intent. Seed builders are never invoked in dry-run mode; Git is never launched.
Actions must not implement an alternate dry-run path or simulate earlier writes.

The user-visible contract, including source validation, shared destinations,
per-child output, and reporting limits, is owned by
[cmdline.md](cmdline.md#dry-run-behavior). Batfiles bookkeeping runs in both
modes: the state documents `tomlfile.rs` rewrites, and the dynamic-variable
commands `dynamic/run.rs` runs, which never consult `RunMode` because the plan
itself depends on their values.

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
The leaf's four sources resolve into one flat scope, in which a name resolves
the same way whatever declared it. The representation is not prescribed; the
seam below is. Each opened inclusion derives its own scope from that one, adding
its `vars` overrides and the included remote's `[vars]`; the scope is held
beside the records the inclusion contributed and decides every condition on
them. Conditions and precedence are specified in
[repoformat.md](repoformat.md#conditions) and
[environment.md](environment.md#variable-precedence). A manifest's layer holds
its dynamic variables' resolved values beside its static ones; one
`DynamicVarResolver` per command resolves each declaration once and owns the
cache.

## Centralized decisions

Keep each of these in one place, without building future abstractions:

1. Effective variable values and their origins are produced by one function.
2. Run settings are carried by `RunContext`; write helpers consult the mode.
3. Execution captures selection once, prepares selected clone lists, and
   walks the run's list once, visiting each inclusion's records at its
   position.
4. Repository source paths use one resolver, which takes the tree a record came
   from.
5. The manifest owns declaration order; `execute` assembles the run's list from
   it, opening every inclusion it reaches. Each opened inclusion owns the
   records it contributed; inclusion is one level deep, so they are never
   inclusions themselves. Assembly and selection are one pass, and both finish
   before clone-list preparation. The assembled list is not a simulation and
   never becomes one: it is the same records, read from more than one file. See
   [Dry-run](#dry-run).
6. Whether an inclusion is opened, and the reading of its manifest, belong to
   `inclusion.rs`, which `execute` and `vars refresh` both call, admission
   first. It takes the anchored repository path rather than a `RunContext`.
   Composing what an inclusion contributes belongs to `execute` alone; refresh
   folds admitted inclusions by remote instead.
