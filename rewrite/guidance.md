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

**1. No `#[allow(dead_code)]`.** CI already runs `clippy -D warnings`, so this
enforces itself. If code has no caller reachable from `main`, it does not get
committed. The old crate carried eight of these annotations, each one the
compiler correctly reporting the problem and being overruled. The stubbed CLI is
not an exception and needs none — clap's derive reads every field.

**A field of a serde record is the one exception, and it is spelled
`#[expect(dead_code, reason = "…")]`.** A record mirrors a file whose shape the
format already fixes, so a field nothing reads yet is not an abstraction built
ahead of its caller; it is one line of a document that exists either way.
`symlink`'s `dest` is written the same way at 0.6 whether or not 0.7 has been
built. What rule 1 exists to stop is unreachable *code* — types, layers, and
functions no command reaches and no external shape constrains. Leaving the field
out instead is the worse outcome: a closed record without `dest` rejects the
manifests the format calls valid, and opening the record stops it rejecting the
mistyped ones.

Use `expect`, never `allow`, and give each one a `reason` naming the step that
reads the field. `expect` is self-cleaning: once the reader lands the annotation
becomes an unfulfilled expectation, which is a warning, which under `-D
warnings` fails CI until it is deleted. That makes these the one kind of
carry-forward note that cannot rot, because the compiler is holding the list.

Make it self-enforcing: have `task ci` fail if `allow(dead_code)` appears
anywhere under `src/`, or if any `expect(dead_code)` is missing its `reason`.
This is the one rule that can be checked mechanically, and the old crate is the
proof that the honor-system version of it loses.

**2. Vertical slices, never horizontal layers.** Every slice ends with a
`batfiles` binary that does something a user can run, and a test in
`tests/cli.rs` that runs it. Do not build "all the serde types" or "all the
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
   `Error::Git(git::Error)` variant, with the sub-enum in the module that raises
   it rather than in `error.rs`, for the same reason `ItemId` lives in
   `item.rs`.

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

**10. `tests/cli.rs` is the primary safety net.** It survives internal
restructuring, which is what the next several slices consist of. Unit tests are
for logic that is pure and genuinely tricky: the truthiness table, variable
precedence, destination path safety. The old crate's roughly 1:1 inline
test-to-code ratio is a large part of what made its internals expensive to
delete — every speculative layer came with tests that made deleting it feel like
losing work.

**11. The manager serves the configuration, not the reverse.** A vim setup that
expects its plugins cloned into `~/.vim/bundle` is a requirement, not a
misconfiguration to argue with. Where an action type looks awkward, the
awkwardness is batfiles' to absorb. This is why `git-clone-list` and `fetch-url`
are slice 4 rather than "use a plugin manager instead", and it is the test to
apply whenever a feature looks easier to decline than to build.

**12. A parsed-but-unimplemented option fails; it is never ignored.** Porting
`cli/` whole at 0.2 means `sync` accepts `--refresh-content` from day one, and
silently ignoring it is worse than not accepting it, because the user believes
something happened. Each command checks its not-yet-live options at entry and
exits with a message naming the option and the slice it arrives in. The list
shrinking to empty is how you know a command is finished.

A consequence worth stating, because it answers "what about types the CLI shape
needs early": **a stubbed option's value can stay a `String`.** `--var
NAME=VALUE` is rejected wholesale until 5.3 makes it live, so nothing needs
`VarName` before then. Strip the validation from ported argument types whose
options are not yet live rather than pulling their newtypes forward — the
rejection check is what makes the stub honest, and it also keeps the field live
for rule 1.

**13. Never destroy what you did not create.** Until the backup policy exists at
9.4, an action that finds its destination occupied by anything it cannot safely
repair fails and names the path. A symlink batfiles would have made is
repairable — relinking loses nothing. A regular file, a directory, or a symlink
pointing somewhere unexpected is not; it is someone's data, and slice 0 has no
way to give it back.

**14. Judge a path by where it points, not by how it is spelled.** Anchor the
roots to absolute paths before writing a target into a link, and resolve an
existing link's target from the link's own directory before comparing or
classifying it. Rule 13 means nothing otherwise: a spelling that starts with the
repository can still leave it, and one that does not can still be inside it.
Every action that stores a path, or classifies one already stored, needs the same
treatment. The containment check is lexical and lives in exactly one place, which
is the seam 6.3 extends rather than replaces.

## Budgets

Not hard limits. If you are far over one, stop and ask why.

- A validated string newtype plus its error: about 40 lines. The old `VarName`
  was 248 and `ItemId` 452, and the excess was prose and tests, not type.
- A module doc comment: about 5 lines.
- A slice: reviewable in one sitting.

## How a slice lands

Work on `main`; CI runs on `main` and on pull requests. Branch per slice if you
want one reviewed. Nothing merges back from the tag that holds the old crate —
see `keep.md`.

## Definition of done for a slice

Every slice, without exception:

- `task ci` passes, including the `allow(dead_code)` check added at 0.11.
- A `tests/cli.rs` test drives the new behavior through the binary.
- Any `docs/future/` section the slice implemented has been promoted into
  `docs/`, re-read against what was actually built rather than pasted.
- Every cross-document link and step reference still resolves.
- The unimplemented-option list (rule 12) shrank if the slice made an option
  live.
- Anything the slice leaves for later has been routed, and the step it was
  discovered on carries none of it. See "Carrying work forward".

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
  shrinking to empty), and anything else greppable (`// CARRY(1.3): …`, which
  0.13 teaches `task ci` to reject once 1.3 is marked ✅). Write prose only for
  what none of these can hold.

## Test environments

Use the cheapest environment that still exercises the real thing.

- Temporary directories for `$HOME` and for repositories — slices 0–3.
- A local bare git repository for anything that clones — slices 4, 6, 7.
- A local HTTP server for `fetch-url` — slice 4.
- Docker only where a pristine machine is the thing under test: `clone` at 8.4
  and `install.sh` at 10.3. Keep it a separate CI task, not part of `task test`.

**No test may reach the network.** A suite that fails on a plane, or on the work
network, is a suite that gets skipped.

**Platform gating is one place, not scattered.** The execution tests that need a
working `symlink` are a single `#[cfg(unix)] mod linking` in `tests/cli.rs`, with
a `#[cfg(not(unix))]` test covering the refusal; an action type that is not
platform-specific does not belong in that module. CI is ubuntu-only, so until
0.12 adds it to `task ci` nothing catches a gate that rots — run `cargo clippy
--all-targets --target x86_64-pc-windows-msvc -- -D warnings` yourself after
touching `tests/cli.rs`. It needs `rustup target add` first and no linker.

Keep several differently-shaped fixture repositories — one leaf-only, one with
remotes, one with deliberately overlapping destination paths — and run the suite
against each rather than growing a single fixture into something no real
repository resembles.

## Dry-run

Dry-run is not a second code path, and the split is **per action, interleaved** —
not a global plan phase followed by a global apply phase. A global two-phase
design is impossible: an action's effects can depend on what an earlier action
did, and `fetch-url`, `git-clone-list`, and `include-remote` all produce content
whose shape is not knowable until they run.

The loop is:

```
for action in plan:
    let planned = action.effects(&ctx)?;   // inspects the filesystem as earlier actions left it
    report(planned);
    if !dry_run { action.apply(planned)?; }
```

**A real run is therefore always accurate.** `effects()` for action N runs
immediately before `apply()` for action N, against the real filesystem including
everything actions 1..N-1 did. Nothing is forecast. `apply` still never
re-derives a decision — if it needs a fact, that fact is in the effect — but the
facts it is given are ground truth. This is what `goals.md` already specifies:
concrete effects are determined as the first phase of *each action's* execution,
using the state left by earlier successful actions.

**Dry-run is where the unknowability lives, and it is inherent.** Skipping every
`apply` means action N inspects a filesystem missing everything earlier actions
would have done. That is not fixable, and the fix is not a simulated filesystem.
Three rules make it honest and cheap:

1. **`effects()` returns `Known(Vec<Effect>)` or `Unknown(reason)`.** Write this
   enum at slice 2, when it is five lines and nothing returns the second variant.
   It is the one concession the fetching slices need in advance, because
   threading a second return case through nine action types later is exactly the
   retrofit worth avoiding. Every other part of unknowability can wait.

2. **A dry run may write to the tool-owned `remotes/` tree; it must not touch
   `$HOME`.** `goals.md` already treats `remotes/` as tool-owned scratch that
   never installs anything by itself. This matters because the deepest
   unknowability is `include-remote`: with an unmaterialized remote you do not
   merely have unknown effects, you have unknown *actions*. Letting a dry run
   materialize turns the worst case into a non-case and keeps the plan complete.

3. **After that, the only unknowable actions are those whose outputs are named
   by remote content** — `fetch-url` unpacking an archive, `git-clone-list`
   reading a manifest. In practice these are leaves of the dependency graph:
   nothing installs *into* an oh-my-zsh checkout or a vim bundle directory, it
   only gets read there. They report "would fetch X into Y" without enumerating,
   nothing downstream is affected, and the plan stays complete. If a repository
   ever does install into a fetched action's output, that action returns
   `Unknown` and the plan is reported partial — which is what the specification
   already says to do.

Attributing an `Unknown` to the specific earlier action responsible is better
output and about fifty lines. Wait until 4.6 has produced real `Unknown`s and
shown whether the plain reason is good enough — not at slice 2, and not before
there is an action that can trigger one.

Written this way, dry-run costs almost nothing at slice 2 and needs no rework
when the fetching actions arrive at slice 4. Retrofitted across nine action types
it is a rewrite, which is why it is not deferred. A tool that writes to `$HOME`
and cannot say what it is about to do is not trustworthy enough to dogfood, and
rule 8 depends on it.

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

2. **`effects()` already returns `Known` or `Unknown`** (step 2.3). Nothing else
   about unknowability needs to exist before slice 4.

3. **One action-execution loop.** `--refresh-content` (9.4) becomes a flag on the
   context that seed-style actions read. With one loop that is a parameter; with
   a loop per command it is a sweep.

4. **One repository-path resolver.** Step 6.3 adds the `@remote/path` case to it.
   Never resolve a source path inline inside an action.

5. **One ordered action list.** `include-remote` (7.2) splices into it. Anything
   that iterates actions iterates that list rather than rebuilding its own.
