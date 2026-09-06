# Rewrite Guidance

Standing instructions for the rewrite. `steps.md` is the order of work; this is
how the work is done. Read this before writing any code.

## Why there is a rewrite

The first implementation reached 15,385 lines of Rust with `sync` unimplemented.
`reach.rs`, `scope.rs`, `condition.rs`, and `dynamic/` — about 5,250 lines —
were unreachable from any command. The tool was built breadth-first from a
finished specification: layer by layer, each layer complete before the next
started, none of them ever called by a user-facing command.

Every rule below exists to prevent that specific failure. None of them are style
preferences.

## Rules

**1. No code without a caller, by the end of the slice.** CI already runs
`clippy -D warnings`, so this mostly enforces itself. Code that no command
reaches once the slice is done does not stay committed. The old crate carried
eight `#[allow(dead_code)]` annotations, each one the compiler correctly
reporting the problem and being overruled. `allow` is never permitted; the two
exceptions below are both spelled `#[expect(dead_code, reason = "…")]` and both
name the step that makes the item live. The stubbed CLI needs neither — clap's
derive reads every field.

**The slice is the unit that has to be clean, and the step is not.** Splitting a
slice into steps small enough to land one at a time is worth more than a tree
that is fully reachable at every intermediate commit, so an item whose caller
arrives at a later step *of the same slice* carries an expectation naming that
step: 4.1 may build what 4.2 calls, and 4.4 need not read a list it has nothing
to do with yet just to keep the parser live. Same slice is the whole of the
bound. A slice ends with a binary a user can run and a `tests/cli/` test that
runs it, so an item still unreachable *then* is unreachable in exactly the sense
that produced 5,250 dead lines, and an expectation reaching into a later slice is
that failure with a note attached. It is not licence to build breadth-first
inside a slice either: the caller has to be a step `steps.md` already defines, and
that step is the one that deletes the annotation.

**A field of a serde record is the second exception, and the only one that may
name a step in a later slice.** A record mirrors a file whose shape the
format already fixes, so a field nothing reads yet is not an abstraction built
ahead of its caller; it is one line of a document that exists either way.
`symlink`'s `dest` is written the same way at 0.6 whether or not 0.7 has been
built. What rule 1 exists to stop is unreachable *code* — types, layers, and
functions no command reaches and no external shape constrains. Leaving the field
out instead is the worse outcome: a closed record without `dest` rejects the
manifests the format calls valid, and opening the record stops it rejecting the
mistyped ones.

Use `expect`, never `allow`, and give each one a `reason` naming the step that
makes the item live. `expect` is self-cleaning: once the caller lands the
annotation becomes an unfulfilled expectation, which is a warning, which under
`-D warnings` fails CI until it is deleted. That makes these the one kind of
carry-forward note that cannot rot, because the compiler is holding the list.

It is self-enforcing: `tests/hygiene.rs` fails if `allow(dead_code)` appears
anywhere under `src/`, if an `expect(dead_code)` is missing its `reason`, or if
that reason names a step `steps.md` marks ✅, names one it does not define, or
names none at all. The last three close the gap the by-the-end-of-the-slice
reading opens: the compiler deletes the note when the caller lands and says
nothing when it never does, so an expectation whose step is done and whose item
is still unread is invisible to it, and the check rejects one exactly as it
rejects a spent `CARRY` marker.

What no check enforces is the same-slice bound above, because nothing textual
can tell a serde record's field — the one exception allowed to reach into a
later slice — from an ordinary one. That half stays yours to hold. The rest is
the rule that can be checked mechanically, and the old crate is the proof that
the honor-system version of it loses.

**2. Vertical slices, never horizontal layers.** Every slice ends with a
`batfiles` binary that does something a user can run, and a test in
`tests/cli/` that runs it. Do not build "all the serde types" or "all the
validation"; build the one type the current slice needs.

**3. A later slice may reshape an earlier one.** That is the point of the
ordering. Do not generalize in anticipation of a slice that has not started.
Wait for the third instance before extracting the shared thing.

**4. Rationale goes in commit messages, not doc comments.** The old `reach.rs`
carried 337 comment lines over 965 code lines, arguing against alternatives that
were never built and citing steps of a plan that no longer existed. A doc
comment says what an item does and what a caller must know. "Why not X", "this
is deliberately not built here", and cross-module design argument go in the
commit message, where they are free to become obsolete.

**5. An error type exists only if some caller matches on it.** The old crate had
17 error types, 24 hand-written `Display` impls, and 16 `error::Error` impls —
and `app::dispatch` flattened all of them to `error.to_string()`. Default to one
crate-level error enum, and let `thiserror` generate its `Display` so a
variant's message sits next to the variant rather than in a match arm a screen
away.

**A variant carries facts; the `#[error]` attribute carries prose.** No variant
holds a message formatted at the call site. That is the mechanism by which one
variant becomes the place unrelated failures go: a field typed `String` accepts
anything, so the fourth caller with nowhere to put its failure puts it there,
and the enum stops describing what can go wrong. Slice 0 shipped two of these
and both were replaced — `Invalid { path, message: String }` became
`DuplicateActionId { path, id, first, second }`, and `Path { field, value,
message }` became one variant per rule it was carrying. A hand-written `Display`
is still right where rendering is conditional and no attribute can express it;
`ExistingNode` is the one instance, and it is a field's `Display`, not a second
chain over the error.

**The enum is allowed to grow, and nesting is what it grows into — later.** A
flat list of thirty variants is hard to read, but a category layer designed
ahead of the failures it will hold is the same mistake as a type per noun. Two
things happen in order:

1. **Section the flat enum first.** Group the variants by what was being done,
   with a one-line comment per group, in the declaration. This is free and
   reversible and covers most of what nesting is wanted for.
2. **Nest when a subsystem earns it**, meaning three or more failures sharing
   vocabulary of their own — git, HTTP, archive extraction, remotes — or the
   first time a caller genuinely matches, which is likelier to come first than
   it sounds: 4.5 has to tell a dirty clone from a network failure to decide
   whether to skip or fail. Then that subsystem's failures become one
   `Error::Git(git::Failure)` variant, with the sub-enum in the module that
   raises it rather than in `error.rs`, for the same reason `ItemId` lives in
   `item.rs`. Note the name: a module that refers to the crate's own `Error`
   cannot also call its sub-enum `Error`, so the first one built is
   `git::Failure`, and the next subsystem should read the same way.

Do not carve categories any earlier. `Read` and `Write` are the proof: they
serve documents, actions, and state files alike, so any category cut made today
either duplicates them or files them under a mechanism-shaped tag that tells a
reader nothing.

**6. Shell out to `git`.** Never link a git library. The user's `~/.gitconfig`,
credential helpers, and SSH agent have to apply. The old `init.rs` already did
this correctly and is the pattern to copy.

**7. Dry-run never simulates a filesystem.** See below.

**8. Dogfood partially at slice 3, fully at slice 4.** Your own dotfiles should
be managed by this binary as soon as symlinks, ordering, and enable/disable work,
and completely once the fetching actions land. If a slice cannot move you closer
to that, the slice order is wrong and should be changed rather than worked
around. This is the feedback loop whose absence caused the rewrite, and it is why
slice 4 is a harder slice than slice 5 but comes first.

**9. Docs are promoted, not inherited.** Nothing in `docs/` describes behavior
that does not run. See `docs.md`.

**10. `tests/cli/` is the primary safety net.** One test target split into
files: `support` holds the fixtures every group is written against, and the rest
are grouped by what they exercise. It survives internal restructuring, which is
what the next several slices consist of. Unit tests are
for logic that is pure and genuinely tricky: the truthiness table, variable
precedence, destination path safety. The old crate's roughly 1:1 inline
test-to-code ratio is a large part of what made its internals expensive to
delete — every speculative layer came with tests that made deleting it feel like
losing work.

**11. The manager serves the configuration, not the reverse.** A vim setup that
expects its plugins cloned into `~/.vim/bundle` is a requirement, not a
misconfiguration to argue with. Where an action type looks awkward, the
awkwardness is batfiles' to absorb. This is why `git-clone-list` and `fetch-file`
are slice 4 rather than "use a plugin manager instead", and it is the test to
apply whenever a feature looks easier to decline than to build.

**12. A parsed-but-unimplemented option fails; it is never ignored.** Porting
`cli/` whole at 0.2 means `sync` accepts `--refresh-content` from day one, and
silently ignoring it is worse than not accepting it, because the user believes
something happened. Each command checks its not-yet-live options at entry and
exits with a message naming the option and the slice it arrives in. The list
shrinking to empty is how you know a command is finished.

The list is one file — `src/cli/unsupported.rs` — checked once at dispatch
entry, ahead of root resolution and ahead of a stub command's own message,
because an unsupported option means nothing was attempted. Like rule 1's
annotations it is self-cleaning: `tests/hygiene.rs` rejects any step it names
that `steps.md` marks ✅, so the slice that makes an option live cannot land
while the entry withholding it survives.

A consequence worth stating, because it answers "what about types the CLI shape
needs early": **a stubbed option's value can stay a `String`.** `--var
NAME=VALUE` is rejected wholesale until 5.3 makes it live, so nothing needs
`VarName` before then. Strip the validation from ported argument types whose
options are not yet live rather than pulling their newtypes forward — the
rejection check is what makes the stub honest, and it also keeps the field live
for rule 1.

**13. Never destroy what you did not create.** Until the backup policy exists at
9.4, an action that finds its destination occupied by anything it cannot safely
repair fails and names the path.

**The rule protects data, so what it turns on is whether the node holds any.**
Two kinds hold none and are therefore batfiles' to replace: a symlink resolving
inside the repository — one batfiles would have made, where relinking loses
nothing — and a symlink resolving nowhere at all, which reaches no content and
gives access to none, so there is nothing there to give back. A regular file, a
directory, and a symlink that both leaves the repository *and* lands on something
are someone's data, and slice 0 has no way to return them.

That test is asked in one place and gets one answer, so a broken link is cleared
wherever it turns up: at a destination an action installs over, at a directory an
action installs *into*, and at any ancestor of one that has to be created on the
way, where the alternative was a bare `EEXIST` naming nothing. Where a broken
link pointed decides nothing and is never used to classify it — batfiles will not
create the far end of a link somebody else made, inside the repository or out.

**"Reaches nothing" is a question about the whole resolution, not about the last
hop.** A link naming an absent path and a link naming `<some-file>/child` both
end nowhere; the operating system distinguishes them only by whether it gives up
with `ENOENT` or `ENOTDIR`, and a check that reads one and not the other calls
half of them someone's data. A symlink *loop* is the case that stays refused:
that resolution did not end nowhere, it did not end, and removing what it cannot
explain is what this rule exists to stop batfiles doing.

**14. Judge a path by where it points, not by how it is spelled.** Anchor the
roots to absolute paths before writing a target into a link, and resolve an
existing link's target from the link's own directory before comparing or
classifying it. Rule 13 means nothing otherwise: a spelling that starts with the
repository can still leave it, and one that does not can still be inside it.
Every action that stores a path, or classifies one already stored, needs the same
treatment. The containment check is lexical and lives in exactly one place, which
is the seam 6.3 extends rather than replaces.

**15. Install completely or not at all.** A destination holding half of something
is worse than one holding nothing, because no later run can tell the difference:
it finds the path occupied, treats the work as done, and reports success over the
wreckage on every run from then on. A tool that converges on a broken state is
worse than one that fails. So build what you are installing somewhere else and
move it into place in one step, and never let the destination hold a partial
write, a placeholder, or anything else standing in for the finished thing.

**Cleanup is not where this is enforced.** A run that fails can tidy up after
itself; a run that is killed returns nothing and tidies nothing, and a copied
tree carrying a read-only directory's permissions is one the tool can no longer
remove. The property has to hold when no cleanup runs at all — which is what
building elsewhere buys, and why it is not merely tidier.

Two corollaries, each cheap and each learned by getting it wrong first. What is
being built is **created closed and widened at the end**, so a private thing is
never briefly a public one — it is left behind by an interrupted run, so
"briefly" is not guaranteed. And cleanup runs **only on a path this run
created**, never on one that was already there: "it is probably ours" in front of
a recursive delete is how a tool destroys data it was written to protect.

`copy` at 1.3 is the worked example, and `src/install.rs` is where it lives —
`build_and_publish`, `publish`, and `discard` in particular, plus `with_scratch`
for an action that has to have the whole thing on disk before it can build
anything: `fetch-archive` at 4.2 downloads and verifies a third sibling of the
destination before it unpacks a single entry into the staging tree. Slice 4
inherits all of it — `fetch-file` and `fetch-archive` are seeds with the same
destinations and the same failure — so reuse that path rather than deriving it
again.

**Building elsewhere is the mechanism, not the rule.** What the rule is about is
the *convergence*: a later run that finds the path occupied, calls the work done,
and reports success over wreckage forever. Building beside the destination buys
that by making a half-thing impossible to leave at the destination at all, which
is the right answer wherever the tool is producing the content. `git-clone` at
4.3 is the one action where it is not: git manages its own destination, and a
staging sibling would buy nothing the alternative does not. That action satisfies
the rule the other way — **by making the wreckage recognizable.** It classifies a
destination as absent, as a healthy worktree rooted exactly there, or as
something refused by name, so an interrupted clone lands in the third arm rather
than the second. Either mechanism is fine; converging on a broken state is not,
and an action that does neither is the bug.

## Budgets

Not hard limits. If you are far over one, stop and ask why.

- A validated string newtype plus its error: about 40 lines. The old `VarName`
  was 248 and `ItemId` 452, and the excess was prose and tests, not type.
- A module doc comment: about 5 lines.
- A slice: reviewable in one sitting.

## How a slice lands

**Every change goes on a branch.** Nothing is committed to `main` directly, not
a slice and not a one-line fix. One branch per change, named for the step it
lands where it has one. CI runs on `main` and on pull requests.

**Commit as you go.** Every change on the branch gets a commit when it is made,
including the ones that correct what the previous commit got wrong. A branch is
a working record and nothing on it is permanent, so there is no reason to hold
work uncommitted while deciding whether it was right.

**Merging to `main` is a squash merge**, unless the merge says otherwise. `main`
gets one commit per change, and its message describes the change as a whole:
what the tree does now that it did not before, and why it was done that way. Not
the route — a correction made mid-branch, a test that failed first, an approach
tried and dropped are how the change was arrived at rather than what it is. This
is the message rule 4 means by "rationale goes in commit messages", and after
the merge it is the only account of the change that exists, so whatever is worth
keeping has to be in it.

**The branch is deleted once it is merged**, unless the merge says to keep it. A
squash merge leaves no merge parent, so git does not consider the branch merged
and `git branch -d` refuses it: `-D` is the ordinary spelling here rather than a
sign that something went wrong. That deletion is what makes the paragraph above
a rule instead of a preference — the intermediate commits go with the branch.

Nothing merges back from the tag that holds the old crate — see `keep.md`.

## Definition of done for a slice

Every slice, without exception:

- `task ci` passes, including the source-hygiene checks in `tests/hygiene.rs`.
- A `tests/cli/` test drives the new behavior through the binary.
- Any `docs/future/` section the slice implemented has been promoted into
  `docs/`, re-read against what was actually built rather than pasted.
- The project `README.md` still describes what the binary does. It is the only
  document here with an audience outside the project, so a slice that changes
  what `sync` can do changes it too. `tests/hygiene.rs` holds the action-type
  line to the `Action` enum; the rest — the worked example, the status note, the
  not-built-yet list — is prose and is yours to keep true.
- Every cross-document link and step reference still resolves.
- The unimplemented-option list (rule 12) shrank if the slice made an option
  live.
- No `expect(dead_code)` naming a step in this slice survives it. The compiler
  deletes the ones whose callers landed; whatever is left is code the slice
  turned out not to need (rule 1).
- Anything the slice leaves for later has been routed, and the step it was
  discovered on carries none of it. See "Carrying work forward".

## What may lag within a slice

The list above is a statement about the slice and not about each step, and the
gap between them is deliberate. A slice whose every intermediate commit had to be
reachable, documented, and complete could not be split into steps small enough to
land one at a time, and the steps would grow until a single one built a format, a
parser, a caller, a test, and a document at once — which is the size of change
this rewrite exists to stop making. Three things may therefore be behind between
two steps of the same slice, each held by a carrier that expires:

- **Code with no caller yet**, carrying `#[expect(dead_code, reason = "…")]` that
  names the step in this slice which calls it (rule 1).
- **An action type or option that parses but does not act**, provided it says so
  at the moment it would have acted and carries a `// CARRY(<step>)` marker. 4.4
  is the worked example: `git-clone-list` landed reading its list and warning that
  it cloned nothing, and 4.5 replaces the warning and the marker together. What
  rule 12 forbids is silence, and it forbids it mid-slice for the same reason it
  forbids it in a release — the user believes something happened.
- **Prose in `docs/`**, promoted by the step that finishes a behavior rather than
  the step that starts it.

Everything else holds at every commit, because `main` is green at every commit:
`task ci` passes, including every check in `tests/hygiene.rs`, so the
filesystem-owner list, the action-type lines in `README.md` and `docs/goals.md`,
the fixtures, and rule 12's list are current the day each step lands rather than
the day the slice ends. Correctness is not on the list at all. A step may leave a
behavior unfinished and must never leave one wrong: the three lags above are all
things a *reader* notices, and nothing here licenses a commit that installs the
wrong thing, reports something that did not happen, or fails a test.

## Carrying work forward

The one job a plan document has is getting a decision to the step that needs it.
`steps.md` is a list of instructions to the future, not a record of the past, and
it stays one line per step because of one rule:

**A step marked ✅ collapses to its one line.** Everything written under it while
it was in progress gets routed first, by asking who reads this, and when:

| Reader | Destination |
| --- | --- |
| One specific later step | That step's bullet, written as an instruction |
| Every later step | A rule in this document |
| Someone using the behavior | `docs/` — see rule 9 and `docs.md` |
| Someone doing archaeology on this change | The commit message |

The last row is the default. If you cannot name a reader who is not doing
archaeology, it is history, and git already has it. The step carries the
instruction rather than a separate ledger, because the step is what someone reads
at the moment they need it.

Two consequences:

- **A note that names no step becomes a step.** A caveat ending "nothing enforces
  this, expect it to rot" is a work item wearing a caveat's clothes.

- **Prefer a mechanical carrier to a sentence.** `#[expect(dead_code, reason =
  "read at 3.2")]` cannot rot, because the compiler deletes the note for you when
  the reader lands (rule 1). The same shape covers an ordering not yet pinned
  (`#[ignore = "3.1"]`), an option that is parsed but dead (rule 12's list,
  shrinking to empty), and anything else greppable (`// CARRY(1.3): …`). Write
  prose only for what none of these can hold.

  A marker is spelled `// CARRY(<step>): <note>` in a `.rs` file under `src/` or
  `tests/`, and `tests/hygiene.rs` rejects three things: a marker whose step
  `steps.md` marks ✅, a marker naming a step `steps.md` does not define, and a
  `CARRY` written any other way. The last two matter because a marker nothing can
  clear is prose again, wearing an annotation's clothes.

## Test environments

Use the cheapest environment that still exercises the real thing.

- Temporary directories for `$HOME` and for repositories — slices 0–3.
- A local bare git repository for anything that clones — slices 4, 6, 7.
- A local HTTP server for the fetching actions — slice 4.
- Docker only where a pristine machine is the thing under test: `clone` at 8.4
  and `install.sh` at 10.3. Keep it a separate CI task, not part of `task test`.

**No test may reach the network.** A suite that fails on a plane, or on the work
network, is a suite that gets skipped. Where a test names a host it never
reaches, write one that cannot resolve — `e.example`, `example.invalid` — so
that a test which starts reaching the network fails in the suite rather than in
somebody's DNS.

**Assert the evidence, not the absence of change.** An unchanged tree passes for
an implementation that fetched and then declined to write, which is most of what
a dry-run test is trying to rule out. So assert the thing only the work produces:
`Server::requests` staying at zero, a clone's `.git/FETCH_HEAD` staying absent.
Reach for the weaker claim only where there is no such artifact — a destination
that was never created leaves nothing behind to check — and pair it with the
whole-tree snapshot.

**Platform gating is one place, not scattered.** The execution tests that need a
working `symlink` are a single `#[cfg(unix)] mod linking;` in
`tests/cli/main.rs`, with a `#[cfg(not(unix))]` test covering the refusal; an
action type that is not platform-specific does not belong in that module. CI is ubuntu-only, so nothing
there runs the Windows side; `task lint` checks it instead, running clippy
against `x86_64-pc-windows-msvc` as well, and that is what catches a gate that
has rotted. The target is pinned in `rust-toolchain.toml` and needs no linker.

Keep several differently-shaped fixture repositories — one leaf-only, one with
remotes, one with deliberately overlapping destination paths — and run the suite
against each rather than growing a single fixture into something no real
repository resembles.

## Dry-run

Dry-run is not a second code path, and it is not a planning phase. **Every action
runs once, in one piece**: it inspects the filesystem as the previous action left
it, decides, and then either writes or — under `--dry-run` — says what it would
have written. There is no `effects` phase, no effect type, and nothing forecast
in either mode.

**What `--dry-run` promises is that none of the plan is carried out** — nothing
installed, replaced, fetched, cloned, or materialized. It is not a promise that
the process writes nothing anywhere: batfiles' own bookkeeping runs in both
modes. Keep it stated as the work not happening, and never as a claim about a
directory; "nothing under the home changes" is a weaker promise that is also
false, since the repository defaults to `<selected-home>/dotfiles`.

**A real run is therefore always accurate**, trivially: the inspection and the
write are one pass, so no decision can go stale between them.

**Dry-run is where the unknowability lives, and it is inherent.** Skipping the
writes means action N inspects a filesystem missing everything actions 1..N-1
would have done. That is not fixable, and the fix is not a simulated filesystem
(rule 7). It is exact for the overwhelmingly common case of actions with distinct
destinations, and wrong only where one action's output is another's input.

**Test it as a whole tree, not as a list of destinations.** The likeliest
dry-run bug is not a written destination — it is a staging node, a parent
directory created on the way, or a broken symlink cleared at an ancestor, none of
which any destination-by-destination assertion looks at, and the first of which
is what 2.1 exists to prevent. So snapshot the whole home root before and after
and compare. That is cheap because the `Tree` fixture gives the four roots as
*siblings* — `home`, `config`, `cache`, `repo` — so nothing batfiles writes for
itself lands under `home`, and the snapshot needs no exclusions.

### Where the mode is read

**Not at the top of an action — at the helpers that write.** An action that
checked a flag before inspecting would have nothing to report, and an action that
checked it at each `fs` call would have to remember, once per action type
forever. Every write an action performs sits behind one of these, and each
already has the branch the check goes in:

| Helper | The branch it already has | Arrives |
| --- | --- | --- |
| `directory::ensure_directory`, reached through `RunContext::ensure_directory` and `directory::create_parents` | the arm taken when nothing resolves at the path | built |
| `action::symlink::link_one` | the `Occupancy::at` arms that remove and create | built |
| `install::seed` | the `paths::occupied(dest)` check, ahead of any staging node | built |
| `git::clone_or_update` | the clone-or-update decision | built |

`RunMode` is a field on `action::RunContext`, beside the anchored roots — the same
value 9.4's `--refresh-content` becomes a second field on. `fetch-file` and
`fetch-archive` publish through `install::seed` (4.1, 4.2), so they are
dry-run correct on the day they are written. **The git helper is the one
addition, and it is needed because `seed` does not cover it**: an existing clone
is an occupied destination, which `install::seed` declines by design, so the
update path reaches the filesystem — and the network — through neither `seed` nor
`paths`. The helper takes the mode itself and under `DryRun` runs no git at all,
reporting `would clone` or `would update` from `paths::occupied` alone. What that
gives up is classification, and it is stated as such in `docs/cmdline.md`: a dry
run cannot tell a healthy clone from a directory that merely occupies the path,
so it says it would update either, and the real run is where the second is
refused.

**That holds for materializing a remote too, and it is a deliberate limit rather
than an oversight.** A dry run does not clone or update `remotes/<id>/`, so an
inclusion is described from whatever is on disk, which may be stale, and an
inclusion never materialized cannot be described at all. The alternative — a dry
run that fetches so its report is current — buys accuracy in one case by making
`--dry-run` a command that changes things and reaches the network, and the whole
value of the flag is that it does neither. A user who wants the current picture
refreshes the remote and runs it again. This is why the helper needs no write
scope and no caller-supplied exception: `DryRun` means no git, everywhere,
without a second question.

### Two lists, and why they are not the same one

Which modules may touch the filesystem, and which of them must read `RunMode`, are
different questions with different answers.

**Only the modules that own filesystem access may name `std::fs`**, per
`CLAUDE.md`'s rule that direct access belongs in the module that owns the
operation. Today that is `paths.rs`, `directory.rs`, `install.rs`,
`tomlfile.rs`, and `action/symlink.rs`, and `tests/hygiene.rs` enforces it at
the *import*, not
against a list of function names (2.3). A denylist of mutators is unbounded and
therefore fake: `fs::write`, `File::create`, `DirBuilder::create`, and
`OpenOptions::truncate` are four ways past one that names `create_dir` and
`rename`. A module that cannot name `fs` can call none of them.

**This list is meant to grow, and each entry says which kind of owner it is.**
Content producers arrive with slice 4 — `action/copy.rs` takes `copy_file`,
`copy_children`, and `mirror_permissions` at 4.1, and the fetcher and the archive
extractor are two more — and 9.1's command runner needs `Command`. A list that
only ever grew would be worthless, so the entry carries a one-line reason, and
the reason has to be one of exactly four:

- **Read-only**, which inspects and never writes, so there is no work for a mode
  to withhold and nothing a dry run has to be told about. `paths.rs` is this:
  what a path means and what is already at one are both questions. It is the
  safest kind and the one to reach for first — a module that cannot write cannot
  write in the wrong mode.
- **A mode reader**, which performs part of an action's work and therefore
  consults `RunMode` itself: the helpers in the table above.
- **Downstream of a mode reader**, which never sees it and does not need to.
  Every content producer is here, and the invariant is structural rather than a
  promise: `install::seed` creates no staging node under `DryRun`, so nothing
  that fills one is reachable. This is why 2.1 states that prohibition as a rule
  of its own.
- **Bookkeeping, which runs in both modes.** State files and caches are not the
  plan; batfiles writes them for itself, and withholding them would change what
  the next run does without protecting anything. `tomlfile.rs` is this, and 9.1's
  dynamic command runner is the awkward member: what it runs is arbitrary
  unsandboxed programs, so the entry should say so. A dry run does not carry out
  the plan it prints, and that is the whole of what it promises — not that the
  process writes nothing anywhere.

An addition that is none of these is the bug the check exists to catch. What the
check buys is not that the list stays short; it is that growing it takes an edit
to a test, where saying which kind you are adding is unavoidable. This is the
answer to the one real objection to a per-action check, and it is the same shape
as rule 1's: the honor-system version of it loses.

### What a dry run says

The two modes report the same lines in different tenses — `linked X -> Y` against
`would link X -> Y`. One `Verb`, shared by every action, holds both forms, so the
tense is decided in one place rather than at each call site (2.2).

**Tense is the only difference for actions with distinct destinations, and that
is the whole of the promise.** Where one action's output is another's input the
two runs diverge in substance rather than tense: a `create-dir` followed by a
seed at the same path says `would create` then `would copy`, where a real run
says `created` then `kept`, because the second action sees the first one's work.
That is the inherent gap above, not a defect, and closing it is the simulation
rule 7 forbids. 2.5 tests parity over actions with distinct destinations, which
is what makes the test a check on the mode rather than a restatement of the gap.

A dry run predicts **intent, not success**. It stops before the write, so a
permission failure, a bad digest, or a destination taken by another process
surfaces only in the real run. What it does get right is every decision resting
on inspection, which is the part a user is asking about: a seed whose destination
is already occupied reports that it would keep what is there, and copies nothing.

It still reads what the manifest names. **A `source` that is missing or
unreadable fails in both modes**, before the destination is considered, including
where the destination is occupied and a real run would have kept it. That is
deliberate rather than an artifact of the order `action::copy::copy` happens to
evaluate its arguments in: a manifest naming a source that is not there is a
repository bug, and a dry run that stayed quiet about it because the destination
was occupied would hide the bug until the day it was not.

**Both `-dir` actions still report per child.** `children::for_each_child`
enumerates the *source* directory, which is in the repository and therefore real
in either mode; only the destination's creation is skipped. A `symlink-dir` over
twelve files says twelve things in a dry run, as it would in a real one. One
consequence is a known divergence rather than a promise: where a `-dir`
destination is itself a broken symlink, a dry run does not remove it, so every
child rediscovers it and says again that it would be removed. 9.6 has the fix.

### Why there is no effect type

An earlier version of this plan split every action into an `effects` phase
returning `Known(Vec<Effect>)` or `Unknown(reason)` and an `apply` phase
consuming it. It was built, and it worked; it cost far more than the accuracy it
bought, and this section replaces it. Two things to know, so that it is not
reintroduced when the fetching actions arrive:

- **`Unknown` was an artifact of the type, not a fact about fetching.** A
  `Vec<Effect>` has to enumerate, and an archive cannot be enumerated without
  unpacking it, so the type needed a hole. Reporting needs no enumeration:
  "would fetch `<url>` and extract into `<dest>`" is complete at the granularity
  `copy` already reports a whole directory at. `git-clone-list` is better off
  still — its manifest is a `RepoPath` in the repository, and it reads fine at
  the moment the action runs, which in a dry run is the moment it prints.
- **Nothing could have triggered it.** `Unknown` was reserved for a repository
  that installs *into* a fetched action's output, and `goals.md` declines to look
  for exactly that: "do not perform cross-action destination conflict detection".
  By rule 1's standard the variant was unreachable.

One case stays genuinely unknowable, and it is not about effects: an
`include-remote` with nothing materialized under `remotes/` contributes actions
the dry run cannot list at all, and since a dry run does not materialize, that is
an ordinary outcome rather than a failure. It is an incomplete *action list*,
slice 7 owns it, and the complete-versus-partial vocabulary in
`docs/future/cmdline.md` waits there with it. **Do not build that vocabulary
earlier.** Until something can report a partial plan, a run that always says
"complete" is a line the code cannot get wrong and the reader cannot use.

A tool that installs into `$HOME` and cannot say what it is about to do is not
trustworthy enough to dogfood, and rule 8 depends on it. That is why slice 2 is
where it is, even though — written this way — it is no longer expensive to
retrofit.

## Variables

Variables feed `when`/`unless` and nothing else. `docs/repoformat.md` makes every
exposed value a string and names conditions as the single enumerated exception;
there is no path interpolation anywhere in the format. Two consequences that
reorder the work:

- **The variable subsystem is a leaf of the dependency graph.** No action needs
  it. That is why it is slice 5, behind the fetching actions, and why dynamic
  variables are slice 9. The old crate built this first.

- **The layered precedence stack is a property of remotes, not of variables.**
  Until inclusions exist there is exactly one scope. Slice 5 is a four-source
  merge into one flat map: repo `[vars]` < `vars.toml` < `BATFILES_VAR_*` <
  `--var`. Do **not** build `Scope` / `Overlay` / `Outcomes` / `InclusionScope`
  at slice 5. That type family — 1,054 lines in the old crate — exists only
  because two inclusions have different scopes from each other, which first
  happens at step 7.5.

Dynamic variables are purely additive: a table where a string was expected,
coupled to nothing else in the format. That is the proof they belong last, and
the reason deferring them costs nothing later.

## Newtypes

Keep them. They prevent the mix-ups that actually happen — `ItemId` and
`VarName` have deliberately different rules and neither validator can stand in
for the other — they make a signature readable at a glance, and holding one is
proof the value was checked.

Two constraints:

- **Small.** A validated string is a struct, a `new`, a `TryFrom<String>` for
  serde, a `Display`, and an error carrying the rejected candidate. Skip
  `AsRef`, `Borrow`, and `Deref` until a call site needs them.

- **Next to their users, not collected in a `types/` module.** Grouping by kind
  is the same organize-by-layer instinct that produced the old structure, and it
  puts the validation rule a directory away from the code that depends on it.
  `item.rs` and `var.rs` at the crate root is the right shape: small,
  cross-cutting, and named for the domain rather than for the technique.

Only `ItemId` and `VarName` are pure validated strings. `RepoPath`, `Condition`,
`FriendlyDuration`, and `ItemAddress` all parse into structure, so a shared
`validated_string!` macro would not fit them. With two candidates, hand-write
both and keep them under budget.

## Seams the late slices need

Slices 9 and 10 are deliberately far out, and dynamic variables in particular
should not force a rewrite of what came before. They stay additive as long as
five things each live in exactly one place. **This is not a request to build
abstractions now.** It is a request not to spread the logic around, which costs
nothing today.

1. **One function produces the effective variable map** — manifest `[vars]` plus
   the override sources in, one map out (step 5.4). Dynamic variables (9.1)
   change that function's innards and widen a value from `String` to
   `Option<String>`, because a declaration can run and produce nothing. If the
   manifest's vars are instead read at fifteen call sites, that becomes a sweep.
   `allow-dynamic-vars` is then one filter inside the same function: a remote's
   dynamic declarations are local to that remote, so the trust boundary has no
   reach beyond this one place.

2. **One place decides whether a write happens** — `RunMode` on `action::RunContext`,
   read at the helpers listed under "Dry-run" (step 2.1). 9.4's
   `--refresh-content` is a second field on that same value, and the hygiene
   check at 2.3 is what keeps that list from growing behind your back. The list
   is not closed: slice 4 adds the git helper at 4.3, which is the one thing that
   does an action's work without going through `paths` or `install`. What stays
   fixed is that the decision is read at a helper and never at the top of an
   action.

3. **One action-execution loop.** `--refresh-content` (9.4) becomes a flag on the
   context that seed-style actions read. With one loop that is a parameter; with
   a loop per command it is a sweep.

4. **One repository-path resolver.** Step 6.3 adds the `@remote/path` case to it.
   Never resolve a source path inline inside an action.

5. **One ordered action list.** `include-remote` (7.2) splices into it. Anything
   that iterates actions iterates that list rather than rebuilding its own.
