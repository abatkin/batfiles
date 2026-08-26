# Rewrite Steps

Eleven vertical slices. Each ends with a working binary and a `tests/cli.rs`
test. Steps are numbered so other documents can reference them. Read
`guidance.md` first — several steps below are short because the reasoning lives
there.

The order is driven by one thing: **how soon the tool can manage real
dotfiles.** Slice 4 exists because the personal repository needs `fetch-url` and
`git-clone-list` and nothing else exotic, so those come before variables,
conditions, and remotes even though they are individually harder.

A step marked **✅** is done, and a done step is one line: whatever it discovered
was routed to the step, rule, or document that needed it before the ✅ went on
(`guidance.md`, "Carrying work forward"). An undone step may have grown since it
was written, and that growth is the point — it is what an earlier slice learned,
waiting where you will read it.

Check `rewrite/README.md` for what happens once slice 8 is done: much of the
"rewrite" scaffolding gets retired at that point.

## Slice 0 — Walking skeleton

`batfiles sync` turns one `[[actions]]` symlink record into a symlink on disk.

- **0.1** ✅ Tag the old crate as named in `keep.md`, empty `src/`, and make the
  initial `docs/` cut described in `docs.md`.
- **0.2** ✅ Port `cli/` whole, with every command parsed and every unimplemented
  one exiting 2, promoting the command overview, global options, output streams,
  exit statuses, and color resolution.
- **0.3** ✅ Port the `Reporter` and the four-root resolution so diagnostics and
  paths work from the first commit, promoting location selection and its
  precedence.
- **0.4** ✅ Port the read half of `tomlfile.rs`, promoting the reading rules into
  a new `docs/repoformat.md`; `sync` parses the leaf `batfiles.toml` before
  reporting that it is unimplemented.
- **0.5** ✅ Define the smallest useful `batfiles.toml`: a list of actions, each
  with an `id`, a `type`, a `source`, a `dest`, and an optional `group`. Landed
  with 0.6, there being no artifact separable from the parser that enforces it.
- **0.6** ✅ Parse it as an internally-tagged enum with one variant, rejecting
  unknown fields, and promote the top-level schema, names and IDs, and the
  `symlink` variant into `docs/repoformat.md`.
- **0.7** ✅ Execute a single symlink action against the resolved home directory,
  creating the link, repairing one that points into the repository, and refusing
  every destination occupied by anything else.
- **0.8** ✅ Say what a refused destination already holds — a regular file, a
  directory, a symlink outside the repository named as written and as it
  resolves, or none of those — in the diagnostic and in
  `docs/repoformat.md`'s destination table.
- **0.10** ✅ Promote `safety.md`'s destination resolution and symlink traversal
  rules into `docs/`, lifted to general statements before `create-dir` and `copy`
  arrive. Absorbed 0.9, so no bullet defines that number.
- **0.11** ✅ Make rule 1 mechanical in `tests/hygiene.rs`: no `allow(dead_code)`
  under `src/`, and every `expect(dead_code)` carrying a `reason`.
- **0.12** ✅ Add the cross-target build 0.7 left on the honor system: `task
  lint` checks `x86_64-pc-windows-msvc` too, so an ubuntu-only CI catches a
  `#[cfg(unix)]` gate that has rotted.
- **0.13** ✅ Reject a stale `// CARRY(x.y)` marker whose step is already marked
  ✅, so a carried-forward note self-cleans the way rule 1's annotations do
  (`guidance.md`, "Carrying work forward").
- **0.14** ✅ Add the unimplemented-option check every command calls at entry,
  and populate it from the options ported at 0.2 (`guidance.md`, rule 12).
- **0.15** ✅ Add a real leaf repository under `tests/fixtures/` and CLI tests
  that sync it, assert the symlink, assert an occupied destination fails without
  writing, and assert an unimplemented option fails.
- **0.16** ✅ Rewrite the project `README.md` to describe what the binary does
  today.

## Slice 1 — The rest of the local actions

- **1.1** ✅ Add `symlink-dir`: one link per direct child of a directory, into
  one destination directory, with an optional `dot-prefix`.
- **1.2** ✅ Add `create-dir`, and settle what `symlink-dir` does with an empty
  `source-dir`: it creates its `dest-dir` and says so.
- **1.3** ✅ Add `copy` with its missing-only seed semantics, preserved once
  created. Landed as two actions, `copy` and `copy-dir`.
- **1.4** ✅ Extend `tests/fixtures/leaf` to declare every action type, splitting
  its expectations into the symlink half inside `mod linking` and the portable
  half outside it, which the manifest declares first so a platform that cannot
  make a symlink still runs it.
- **1.5** ✅ Extract what the five variants genuinely share, splitting the
  831-line `sync.rs` by action *pair* into `action/`, with the seed machinery
  moved unchanged into `install.rs` and the roots behind a `Context`.

## Slice 2 — Dry-run

Do this before a fourth action type exists. See `guidance.md`, "Dry-run".

- **2.1** Split every action into an `effects` phase that inspects and a
  separate `apply` phase that performs. `copy` is the variant to design against:
  it is the first whose effects are neither one thing nor one per child, since a
  directory installed whole is one decision covering a tree of unknown size. An
  effect that names what it will write, rather than enumerating it, is what that
  wants — which is also the shape 4.1 needs, so getting it wrong here is paid
  for twice.

  1.5 laid out what this works within, so the split is per file rather than
  across an 831-line one: `action/symlink.rs`, `action/copy.rs`, and
  `create_dir` in `action/mod.rs`. Three things it left are the ones to use
  rather than work around. `action::Context` is where the dry-run flag goes —
  it already holds the anchored roots and the reporter, and 9.4's
  `--refresh-content` is a second flag on the same value. `install_children`
  takes the per-child work as a closure, which is the seam through which a
  `-dir` action's effects come back one child at a time; its `install_one`
  parameter becomes the thing that returns effects rather than performs them.
  And `install.rs`'s `seed` already asks `paths::occupied` before doing
  anything — that call is the whole of a seed's `effects`, so the phase
  boundary is a line that already exists.
- **2.2** Interleave the two per action, so a real run computes each action's
  effects against the filesystem the previous action left.
- **2.3** Make `effects` return either known effects or an unknown-with-reason,
  even though nothing returns unknown until slice 4.
- **2.4** Render the computed effects for `--dry-run`, marking the plan partial
  if any action reported unknown.
- **2.5** Never simulate a filesystem to close the gap.
- **2.6** CLI test: a dry run changes nothing on disk, and its output matches
  what the real run then does.

## Slice 3 — Selection and ordering

- **3.1** Make declaration order the execution order, explicitly and tested.
  0.15 pinned half of it: syncing the fixture over an occupied destination
  refuses that action and leaves the ones after it undone, so stopping at the
  first failure is covered. What is still owed is order itself, which needs two
  actions whose order is observable — one creating what the next depends on —
  rather than two that merely both happen. 1.4 left a second consumer of
  declaration order and no coverage of it:
  `the_actions_that_need_no_symlink_run_where_symlinks_cannot_be_made` reads the
  `leaf` fixture's portable actions as the ones ahead of the first `symlink`
  record, and being `#[cfg(not(unix))]` it runs on no CI runner. Reordering that
  manifest breaks a test nothing here would notice, so the observable pair this
  step owes belongs in the fixture rather than in a manifest written inline.
- **3.2** Add groups and group membership. The `group` field has parsed and been
  validated as an `ItemId` since 0.6; this is the step that reads it, so its
  `expect(dead_code)` goes — CI will insist — along with the line in
  `docs/repoformat.md` saying nothing selects by group yet.
- **3.3** Port the atomic-write half of `tomlfile.rs`, `disabled.toml`, and the
  four enable/disable commands. Three things 0.4 left are finished here.
  `read_or_default` and `Error::is_not_found` are still at the tag: together
  they are what lets a state file treat a missing document as empty where the
  leaf manifest treats it as an error. The atomic whole-document rewrite rule in
  `future/state.md` is still unpromoted, because 0.4 had no writer to promote it
  against; it becomes `docs/state.md` here, linked from the reading rules in
  `docs/repoformat.md`.
- **3.4** Add `--skip` for suppressing an action or group for one run. The
  environment half belongs here too, or it is silently ignored:
  `BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS` union with the options.
  `Environment::list` at the tag is the comma-split, trim, drop-empties helper
  they share with 8.3's four bootstrap lists; 0.3 left it there for want of a
  caller.
- **3.5** Add default-disabled bootstrap entries. Like 5.1 and 6.1, adding the
  section is also un-rejecting it: the closed document turns `[default-disabled]`
  away today.
- **3.6** Add `apply-action` and `apply-group` over the same filtered plan.
- **3.7** Port `ItemAddress` and its parsing, which 3.6 is the first caller of.
- **3.8** Start managing the parts of your personal dotfiles that need only
  local actions, leaving the fetching parts to the existing script.

## Slice 4 — Fetching actions

The first slice with a real acceptance test. Individually the hardest work so
far, but it is what makes the tool usable, so it comes before the easier
variable and condition slices.

- **4.1** Add `fetch-url` for a single file, seeded only when missing — the same
  words as `copy`, and it should be the same code path. A download is the worst
  case rule 15 is about: it is slow, so the window in which the destination
  holds something unfinished is wide, and a network that drops mid-transfer is
  ordinary rather than exceptional. Publish through `install.rs`'s `install`, and
  take the digest check from `future/safety.md` with it — verifying content is a
  step between building and publishing, which is exactly the shape that path
  already has. Slice 1 spent four rounds of review getting this right for
  `copy`; none of it is worth deriving a second time.

  **This is the step that parameterizes `fill`.** 1.5 moved `install.rs` out
  whole but deliberately left it naming its one content producer directly,
  because `copy` was the only caller and rule 3 says wait. A download is the
  second, so `fill` becomes the argument — what to write into the staging node
  this run created — and `copy_file`, `copy_children`, and `mirror_permissions`
  go to `action/copy.rs` with it. What must **not** move is anything between
  the staging node and the destination: `create_staging`, `publish`, and
  `discard` are the rule-15 property itself, and a download reaches them by the
  same route a copy does.
- **4.2** Add archive extraction, rejecting absolute paths, `..` traversal, and
  symlinks escaping the destination root. A directory seed, so rule 15 again:
  extract into the staging tree and publish once, rather than into the
  destination. That also makes the entry rejections cheap to enforce — an entry
  that escapes is caught before anything reaches `$HOME`, and the whole
  extraction is abandoned by discarding one path.
- **4.3** Add `git-clone` for one repository, and the shared helper that shells
  out to `git`.
- **4.4** Add the `git-clone` list manifest format.
- **4.5** Add `git-clone-list`, cloning each entry and updating existing clones
  conservatively.
- **4.6** Make 4.1 and 4.5 return `Unknown` from `effects` — the first real
  exercise of the path slice 2 built.
- **4.7** Test against a local HTTP server and local bare git repositories; no
  step in the suite may reach the network.
- **4.8** **Acceptance: your personal dotfiles are fully managed by `sync`, and
  the personal shell script is retired.**

## Slice 5 — Variables and conditions

One flat scope only. See `guidance.md`, "Variables". Swap this with slice 4 if
the personal repository turns out to need OS conditionals before it needs
fetching.

- **5.1** Add static string values under `[vars]` in the leaf manifest. The
  document has been a closed record since 0.6, so `[vars]` is rejected outright
  until now: adding the section is also un-rejecting it, and
  `docs/repoformat.md`'s top-level schema names `[vars]` among the sections that
  fail. The same paragraph is edited at 3.5 and 6.1.
- **5.2** Port `vars.toml` and the `vars set` / `get` / `unset` commands.
  `vars get` is the first command that answers a question rather than reporting
  what it did, so it is the first caller of `Reporter::data`, left at the tag by
  0.3: standard output, no label, no color, and never gated by `--quiet`.
- **5.3** Add `BATFILES_VAR_*` and `--var`, warning on and dropping an invalid
  environment name. `Environment::one_shot_vars` and `VAR_PREFIX` are waiting at
  the tag — 0.3 ported only `capture`, `get`, and `location` — and three of their
  rules are the kind that get re-derived wrong: a bare `BATFILES_VAR_` is
  ignored, an empty value is significant, and name validity is deliberately
  deferred to the merge step so the diagnostic can name the whole environment
  variable rather than the suffix. `--var` was ported at 0.2 as a bare
  `Vec<String>` under rule 12, so this step puts its value parser back:
  `parse_var` in `src/cli/options.rs` at the tag splits at the first `=`,
  validates the key as a `VarName`, and checks the shape first so
  `--var profile` reports the missing `=` rather than a name-rule complaint. Take its five tests with it, and the
  two in `cli/actions.rs` asserting that an invalid key is a usage error raised
  before any root is resolved or any file opened — that ordering is what
  `future/cmdline.md` promises about `--var`, and it is not testable until now.
- **5.4** Merge those four sources in one function into one flat map, recording
  each value's origin for `vars list`.
- **5.5** Port the truthiness table and the `facts` / `env` / `vars` namespace
  binding. `Environment::entries` — the whole captured map, which is what the
  read-only `env` namespace exposes — is the last piece of the environment 0.3
  left at the tag.
- **5.6** Gate leaf actions and groups on `when` and `unless`, rejecting a
  record that sets both. The conditions are the third accessor to match over
  every `Action` variant, after `id` at 0.6 and `group` at 3.2, so this is the
  step 1.5 defers the collapse to: replace the per-field accessors with one `fn
  common(&self) -> Common<'_>` returning a borrowed view of the shared fields,
  so there is one exhaustive match rather than one per field.
- **5.7** Make an unevaluable condition close the gate and warn, in both
  spellings.
- **5.8** Add `vars list`.

## Slice 6 — Git remotes, materialization only

No inclusion of remote actions yet.

- **6.1** Add a `[remotes]` table with `type = "git"`. As at 5.1, this
  un-rejects a section the closed document turns away, and edits the same
  paragraph of `docs/repoformat.md`.
- **6.2** Materialize a declared remote into `remotes/<id>/`, reusing 4.3's git
  helper and 4.5's conservative update rules.
- **6.3** Add `@remote/path` as a parsed repository path resolved against
  exactly one repository, in the one resolver every action uses.
- **6.4** Let a leaf symlink or copy action take its source from a remote.
- **6.5** Gate remotes on `when` and `unless`.
- **6.6** Let `--dry-run` materialize into the tool-owned `remotes/` tree while
  leaving `$HOME` untouched, so an included remote's actions are knowable.
- **6.7** Fixture: a local bare git repository standing in for a remote, as a
  sibling of `leaf` under `tests/fixtures/` rather than as a growth of it.

## Slice 7 — `include-remote`

The hard slice. Everything it composes over is real by now.

- **7.1** Read an included remote's own `batfiles.toml`.
- **7.2** Splice its actions into the leaf's single ordered action list, in
  place, preserving order.
- **7.3** Add the action and group selection filters on the inclusion.
- **7.4** Add per-inclusion variable overrides.
- **7.5** Introduce one scope per inclusion — the first point at which layered
  precedence is real rather than a flat map.
- **7.6** Enforce the one-level rule: an included remote's own remotes and
  inclusions are not followed.
- **7.7** Give each inclusion a stable, unique display label.
- **7.8** Build a synthetic two-remote fixture with overlapping paths and
  per-inclusion variables, standing in for the corporate repository.
- **7.9** **Acceptance: the work repository assembles the personal and corporate
  repositories under `sync`, retiring the assembly scripts.**

## Slice 8 — Bootstrap

- **8.1** Port `init`, trimmed.
- **8.2** Add `clone`: a `git clone` followed by a `sync`, and little else.
- **8.3** Add bootstrap precedence and its interaction with default-disabled
  entries.
- **8.4** Add a Docker-based test that clones into a pristine machine image, as
  a `task test:docker` that `task ci` runs and `task test` does not.

## Slice 9 — Leaves

Independent of everything above, and — except for 9.2, which needs 9.1 —
independent of each other. Build on demand, in any order. `guidance.md`, "Seams
the late slices need", is the reason none of these require reworking what came
before.

- **9.1** Dynamic variables: the record, the command runner, timeouts, the
  cache, and `allow-dynamic-vars`.
- **9.2** `vars refresh`, including selective refresh by key. Requires 9.1;
  there is nothing to refresh without it.
- **9.3** File and archive remotes.
- **9.4** `--refresh-content` and the backup policy it depends on. It is also
  what makes 0.8's refusals obsolete: the "move it aside and run sync again"
  remedy and the error rows of `docs/repoformat.md`'s destination table are
  written for a tool that cannot give anything back, and `future/safety.md`
  parks `--no-overwrite` and `--interactive` here for the same reason.

  For the seed actions this is one function, not a sweep: `install.rs`'s `publish`
  is the single place that decides whether an occupied destination is kept, and
  `--refresh-content` is that decision inverted — back the destination up, then
  replace it. Everything the copy needed in order to be safe is already on the
  other side of it, since what `publish` moves into place is complete before it
  moves. Note what changes underneath, though: replacing means the destination
  is no longer required to be absent, so `hard_link` stops being the publication
  for files and the no-replace property goes with it. That is 9.5's problem
  arriving early, and the two steps should be read together.

- **9.5** Atomic no-replace publication, across every action or not at all. Each
  action decides whether a destination is free and then acts on the answer, so
  each has a window in which another process can take that path: `copy`'s is two
  adjacent calls, and `symlink`'s is three steps with a `remove` in the middle,
  which is both wider and the one every user runs. 1.3 closed the file half of
  `copy`'s with `hard_link` because that was free; the rest needs
  `renameat2` on Linux, `renamex_np` on macOS, `MoveFileExW` without
  `MOVEFILE_REPLACE_EXISTING` on Windows, nothing on the BSDs, and a fallback
  for the filesystems that refuse the flag anyway. That is unsafe FFI on three
  platforms, two of which CI never runs, to close a window nothing in the tool
  closes elsewhere. Do it as one piece of work over every action, or leave it
  documented — `docs/repoformat.md` says plainly that batfiles is not safe
  against a concurrent writer, which is the honest version of the status quo.
  Do not close it for one action and leave the wider ones open.

## Slice 10 — Distribution

Its own project. Do not start it before slice 8 is in real use.

- **10.1** Release binaries at a stable URL, with checksums and platform
  detection.
- **10.2** The `install.sh` template `init` writes, which finds `batfiles` on
  `PATH` or downloads it to `~/.local/bin`.
- **10.3** A Docker test that runs the one-liner end to end against a local
  release server, so the suite still never reaches the network.
