# Rewrite Steps

Eleven vertical slices. Each ends with a working binary and a `tests/cli/`
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

- **2.1** ✅ Add `RunMode { Perform, DryRun }` to `action::RunContext` and read it
  at the three helpers that carry out an action's work. Under `DryRun`
  `install::seed` creates no staging node.
- **2.2** ✅ Report in the tense the mode dictates, through one `Verb` shared by
  every action.
- **2.3** ✅ Make 2.1 mechanical: `tests/hygiene.rs` rejects `std::fs`,
  `std::os::*::fs`, and `std::process::Command` named at all outside an
  allowlist whose entries each say which kind of owner they are.
- **2.4** ✅ Un-stub `--dry-run` and promote the docs, creating
  `docs/cmdline.md`'s "Dry-Run Behavior".
- **2.5** ✅ CLI test: a dry run over the whole `leaf` fixture leaves the home
  root byte-for-byte as it found it, snapshotted whole, and its lines match what
  the real run then reports, word for word but for the tense.

## Slice 3 — Selection and ordering

- **3.1** ✅ Make declaration order the execution order, explicitly and tested.
- **3.2** ✅ Add groups and group membership, read by the `-v` heading that names
  the record each action's lines come from.
- **3.3** ✅ Port the atomic-write half of `tomlfile.rs`, `disabled.toml`, and
  the four enable/disable commands, promoting the write path and the document
  into a new `docs/state.md`.
- **3.4** ✅ Add `--skip` and its `BATFILES_SKIP_*` half, and make `sync` read
  the `disabled.toml` 3.3 only wrote, through one filter in `src/selection.rs`.
- **3.5** ✅ Add default-disabled bootstrap entries, which — like 5.1 and 6.1 —
  is also un-rejecting the section the closed document turned away. Parsed and
  checked as the manifest is read; adoption is 8.3's.
- **3.6** ✅ Add `apply-action` and `apply-group` over the same filtered plan,
  each waiving the exclusions that name what it asked for.
- **3.7** ✅ Port `ItemAddress`, widening both apply targets, `disabled.toml`'s
  two lists, the run-only skips, and `[default-disabled]`'s entry fields, so that
  a name reaching into a not-yet-included remote is recorded rather than refused.
- **3.8** Start managing the parts of your personal dotfiles that need only
  local actions, leaving the fetching parts to the existing script.

## Slice 4 — Fetching actions

The first slice with a real acceptance test. Individually the hardest work so
far, but it is what makes the tool usable, so it comes before the easier
variable and condition slices.

Dry-run needs one addition here and no rework. Both fetching actions publish
through `install::seed`, which reads the mode, so they are dry-run correct as
written; the git helper at 4.3 is the exception and reads the mode itself. The
step that made them report unknown effects went with the effect type it was an
artifact of (`guidance.md`, "Why there is no effect type"), so no bullet defines
4.6.

- **4.1** Add `fetch-url` for a single file, seeded only when missing — the same
  words as `copy`, and it should be the same code path. A download is the worst
  case rule 15 is about: it is slow, so the window in which the destination
  holds something unfinished is wide, and a network that drops mid-transfer is
  ordinary rather than exceptional. Publish through `install.rs`'s `build_and_publish`, and
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

  Moving the fillers puts `std::fs` in `action/copy.rs`, so add it to 2.3's
  allowlist here, as a downstream entry rather than a mode reader — nothing it
  contains is reachable under `DryRun`, because `install::seed` creates no
  staging node to fill. The fetcher is the same kind of entry for the same
  reason.
- **4.2** Add archive extraction, rejecting absolute paths, `..` traversal, and
  symlinks escaping the destination root. A directory seed, so rule 15 again:
  extract into the staging tree and publish once, rather than into the
  destination. That also makes the entry rejections cheap to enforce — an entry
  that escapes is caught before anything reaches `$HOME`, and the whole
  extraction is abandoned by discarding one path.
- **4.3** Add `git-clone` for one repository, and the shared helper that shells
  out to `git`. **That helper is the fourth thing that reads `RunMode`, and the only
  one slice 4 adds** (`guidance.md`, "Where the mode is read"). It is not covered
  by anything slice 2 built: a clone destination that already exists is an
  occupied destination, which `install::seed` declines by design, so the update
  path reaches the worktree — and the network — through neither `seed` nor
  `paths`. A dry run that ran it would contact the network and modify a checkout
  it was asked only to describe, which is the invariant this slice is most able
  to break. Add the helper to 2.3's allowlist in the same change, since it names
  `std::process::Command`; it is the one addition of the mode-reader kind.

  **Under `DryRun` the helper runs no git, for any caller.** It needs no write
  scope and no exception argument, and 6.2 must not add one: a dry run does not
  materialize remotes either, and that limit is deliberate (`guidance.md`, "Where
  the mode is read"). **Do not infer the answer from the destination path.** A
  containment test against the home is wrong in both directions — the batfiles
  directory is ordinarily inside the home, and an action's `dest` may be an
  absolute path outside it — and with one rule for every caller there is nothing
  for such a test to decide anyway.
- **4.4** Add the `git-clone` list manifest format.
- **4.5** Add `git-clone-list`, cloning each entry and updating existing clones
  conservatively. Say what a dry run reports for each of the two cases, because
  they are not the same sentence: a destination that is absent is `would clone
  <url> into <dest>`, and one holding a clone already is `would update the clone
  at <dest>`. **Neither runs `git`**, so neither reaches the network and neither
  can say what the update would bring — which is the "intent, not success"
  boundary rather than a partial plan (`guidance.md`, "Why there is no effect
  type"). The conservative update rules 6.2 reuses are about what a real run
  does; a dry run stops before all of them.
- **4.7** Test against a local HTTP server and local bare git repositories; no
  step in the suite may reach the network. A dry run is part of what is tested
  here: `fetch-url` says what it would fetch and where, and `git-clone-list`
  reads its manifest — a repository file, readable at the moment the action runs
  — and says one line per entry. Neither reaches the network in that mode, and an
  existing clone is left exactly as it was, unfetched.

  Promote `docs/future/cmdline.md`'s "Remote content is described, not retrieved"
  paragraph into the section 2.4 created, minus its second half about inclusions,
  which waits for 7.1.
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
  so there is one exhaustive match rather than one per field. The same two fields
  also go on 3.5's two `[default-disabled]` entry records, whose closedness is
  what refuses them today — and since the both-set rule is the kind serde cannot
  express, checking it walks those two lists, which is the first thing to read
  them. That is why `Manifest::default_disabled` and the two `Vec` fields under
  it expect their dead code until *this* step and not until 8.3; only each
  entry's `id` and `group` stay unread until adoption.
- **5.7** Make an unevaluable condition close the gate and warn, in both
  spellings.
- **5.8** Add `vars list`.

## Slice 6 — Git remotes, materialization only

No inclusion of remote actions yet.

- **6.1** Add a `[remotes]` table with `type = "git"`. As at 5.1, this
  un-rejects a section the closed document turns away, and edits the same
  paragraph of `docs/repoformat.md`.
- **6.2** Materialize a declared remote into `remotes/<id>/`, reusing 4.3's git
  helper and 4.5's conservative update rules. It calls that helper the same way
  `git-clone` does and asks it for no exception, so under `--dry-run` nothing is
  cloned or fetched here either; 6.6 is where that is stated and tested.
- **6.3** Add `@remote/path` as a parsed repository path resolved against
  exactly one repository, in the one resolver every action uses.
- **6.4** Let a leaf symlink or copy action take its source from a remote.
- **6.5** Gate remotes on `when` and `unless`.
- **6.6** State and test what `--dry-run` does about materialization: nothing.
  It reports that it would clone or update `remotes/<id>/` and leaves the tree
  exactly as it found it, materialized or not. If 4.3 gave the helper one rule
  for every caller, this step is a report line and a test; if it grew a write
  scope instead, this is where that surfaces, and the fix belongs in the helper.

  **Test it against 6.7's bare repository**, both from an unmaterialized start
  and over an existing materialization, snapshotting `remotes/` whole the way 2.5
  snapshots the home root. The second case is the one worth writing carefully: an
  implementation that clones only when absent passes the first and fails the
  second, and quietly re-fetching a remote is exactly what this decision rules
  out.

  What that costs is a dry run describing a remote from a materialization that
  may be old, and it is accepted rather than mitigated (`guidance.md`, "Where the
  mode is read"). 7.1 owns what to do when there is no materialization at all.
- **6.7** Fixture: a local bare git repository standing in for a remote, as a
  sibling of `leaf` under `tests/fixtures/` rather than as a growth of it.

## Slice 7 — `include-remote`

The hard slice. Everything it composes over is real by now.

- **7.1** Read an included remote's own `batfiles.toml`. **This slice owns the
  complete-versus-partial plan, and it is the only thing that does.** A dry run
  does not materialize (6.6), so it reads whatever `remotes/<id>/` already holds
  and lists that remote's actions as its last materialization declares them —
  possibly out of date, which is the accepted cost. An inclusion with nothing
  materialized leaves actions the run cannot name at all: an incomplete *action
  list* rather than an action with unknowable effects, and nothing else in the
  tool can produce one, which is why slice 2 builds none of this vocabulary
  (`guidance.md`, "Why there is no effect type"). Promote the remote-inclusion
  paragraph of `docs/future/cmdline.md`'s dry-run section here — both halves,
  staleness and partiality — into the `docs/cmdline.md` section 2.4 created.
- **7.2** Splice its actions into the leaf's single ordered action list, in
  place, preserving order. **This is where `ItemAddress::names` stops being
  string equality.** 3.7 widened every list that holds a name to an address, so
  a qualified one already parses, is already recorded, and already reports that
  it matched nothing; what that one method says is the whole of why. A spliced
  action answers to `<inclusion>.<id>`, so giving it one is what makes the
  entries `disabled.toml` and `--skip-action` have been accepting since 3.7 do
  anything — and it is one function rather than a sweep, which is what widening
  them early bought.
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
  entries. 3.5 landed the section parsed and unread, so this is the step that
  first reads it, and the list of fields waiting on it is held by the compiler
  rather than by prose: every one carries `#[expect(dead_code, reason = "adopted
  at 8.3, …")]`, which becomes an unfulfilled expectation — and under `-D
  warnings` a failure — the moment adoption reads it (rule 1). Two assertions in
  `tests/cli/manifest.rs` are this step's to change on purpose: that a candidate
  disables nothing, and that a `sync` over a manifest declaring candidates
  creates no `disabled.toml`.
- **8.4** Add a Docker-based test that clones into a pristine machine image, as
  a `task test:docker` that `task ci` runs and `task test` does not.

## Slice 9 — Leaves

Independent of everything above, and — except for 9.2, which needs 9.1 —
independent of each other. Build on demand, in any order. `guidance.md`, "Seams
the late slices need", is the reason none of these require reworking what came
before.

- **9.1** Dynamic variables: the record, the command runner, timeouts, the
  cache, and `allow-dynamic-vars`. The command runner names
  `std::process::Command`, so it joins 2.3's allowlist as bookkeeping, and its
  reason should say that what it runs is arbitrary unsandboxed programs. It reads
  no `RunMode`: a dry run resolves dynamic variables normally and may write
  `dynamic-vars.toml`. Promote the dynamic-variable paragraph of
  `docs/future/cmdline.md`'s dry-run section into the `docs/cmdline.md` section
  2.4 created — it is the one caveat on "a dry run does not do the work", and it
  has no business being promoted before there is a command to run.
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

- **9.6** Report a prospective removal once per run. A dry run does not remove a
  broken symlink, so anything that inspects the same path again finds it again
  and says again that it would go. The reachable case is a `-dir` action whose
  `dest-dir` is a broken link: `for_each_child` reports it, then every child's
  `create_parents` rediscovers it, so twelve children produce thirteen lines
  where a real run produces one.

  The fix is a set of already-reported paths on `action::RunContext`, consulted in
  `DryRun` only — about ten lines, and not a simulated filesystem, since it
  changes what is *said* rather than what is *found*. It waits here because the
  cost is duplicated output in a narrow case, and because a `Context` carrying
  mutable state is a real change to a value slice 2 wants to keep boring. Do it
  when the noise is worth ten lines, or when a second reporter needs the same
  set.

## Slice 10 — Distribution

Its own project. Do not start it before slice 8 is in real use.

- **10.1** Release binaries at a stable URL, with checksums and platform
  detection.
- **10.2** The `install.sh` template `init` writes, which finds `batfiles` on
  `PATH` or downloads it to `~/.local/bin`.
- **10.3** A Docker test that runs the one-liner end to end against a local
  release server, so the suite still never reaches the network.
