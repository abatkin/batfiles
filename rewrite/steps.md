# Rewrite Steps

Eleven vertical slices. Each ends with a working binary and a `tests/cli/`
test. Steps are numbered so other documents can reference them. Read
`guidance.md` first — several steps below are short because the reasoning lives
there.

The order is driven by one thing: **how soon the tool can manage real
dotfiles.** Slice 4 exists because the personal repository needs `fetch-file` and
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
- **3.8** ✅ Write the manifest for the local half of the personal repository —
  every installer step but the four that reach the network — and verify it
  against a scratch home. Landed on a branch rather than in use: running it
  beside the script it half-replaces buys nothing, so adoption waits for 4.8,
  which retires that script. What it found is the two Enhancements below.

## Slice 4 — Fetching actions

The first slice with a real acceptance test. Individually the hardest work so
far, but it is what makes the tool usable, so it comes before the easier
variable and condition slices.

Dry-run needs one addition here and no rework. Both fetching actions publish
through `install::seed`, which reads the mode, so they are dry-run correct as
written; the git helper at 4.3 is the exception and reads the mode itself. The
step that made them report unknown effects went with the effect type it was an
artifact of (`guidance.md`, "Why there is no effect type"); 4.6 below is the
number it left free, put back to use rather than kept as a gap.

- **4.1** ✅ Add `fetch-file` for a single file, seeded only when missing, through
  the same `install.rs` path `copy` uses. `fill` is now a parameter carried on an
  `install::Seed` descriptor, and the fetcher joined 2.3's allowlist beside
  `action/copy.rs` as a downstream entry. Landed as `fetch-url` and renamed
  before 4.2: the transport is what the two fetching actions share, so naming the
  built one after it left the ambiguous name on the specific member of the pair.
- **4.2** ✅ Add `fetch-archive`, refusing every entry that would be written
  outside its destination. Gzipped and plain tar, sniffed from the archive's own
  bytes; `archive-root` built and the entry filters left in `docs/future/`, which
  now holds only them. The archive is a third sibling of the destination —
  downloaded whole, verified, then unpacked into the staging tree — so
  `install.rs` grew `with_scratch` beside `seed`, and that is where the rule-15
  discipline for it lives.
- **4.3** ✅ Add `git-clone` for one repository, and the shared `src/git.rs` that
  shells out to `git` — the fourth reader of `RunMode`, and the only one slice 4
  adds. It clones where nothing is and fast-forwards where a clone is, which is
  the whole of the conservative update policy that does not need a `ref`.
- **4.4** ✅ Add the clone list format, and read it as the repository is loaded.
  The record and `src/clone_list.rs` landed together, there being no artifact
  separable from the parser that enforces it, and the load-time read is a change
  of mind about `docs/future/`'s deferred manifest expansion that the future
  document now records. Reaching the action warns that it cloned nothing and the
  run carries on, on `git.rs`'s terms for a clone it declines to update rather
  than rule 12's for an option — deliberately, the window being one step wide,
  and the cost being that a successful `sync` does not mean the whole manifest
  is installed. `action/git_clone_list.rs` holds the warning and the `CARRY`
  marker; 4.5 replaces both.
- **4.5** ✅ Add the cloning half of `git-clone-list`, and `ref` on `git-clone`
  with it. The entries the read pass checks now travel on the record —
  `Option<Vec<Entry>>` under `#[serde(skip)]`, `None` meaning no pass read this
  one, which **7.2 keeps true by splicing included records ahead of that pass**
  rather than after it. A `ref` is resolved after the fetch and **against the
  remote-tracking namespace first**, `origin` deciding where several remotes
  match: resolving the bare string finds the local branch a fetch never moves, so
  `ref=main` would pin a plugin to the commit it was first cloned at and report
  success forever. That is the one way to get this wrong, and
  `docs/repoformat.md` now says so where a reader of the format will find it. A
  clone at a ref reports `cloned <dest> from <url> at <ref>` rather than a clone
  line and a switch; `Verb::SwitchRef` belongs to the clone that was already
  there and moves. The git failures nest as `Error::Git(git::Failure)` —
  `Failure`, not `Error`, because `git.rs` names the crate's `Error` throughout
  and one module cannot have both — and `action/git_clone_list.rs` holds the
  private predicate deciding whether a failure costs one entry or the run.
- **4.6** Make rule 1's within-slice reading mechanical: `tests/hygiene.rs`
  rejects an `expect(dead_code)` whose `reason` names a step `steps.md` marks ✅,
  names a step it does not define, or names no step at all. Today the reason is
  prose the check only requires to be non-empty, and the compiler covers only the
  half where the caller lands; an item whose step is done and whose caller never
  arrived is the other half, and it is the one rot path that letting code sit
  unused between two steps opens. Reuse `recorded_steps`, `is_step`, and the
  three-case shape the `CARRY` check already has — a step-shaped token anywhere
  in the reason is the step, since these read `"cloned at 4.5"` rather than a bare
  number (`guidance.md`, rule 1).
- **4.7** Finish the git fixtures for a *list* of repositories; no step in the
  suite may reach the network. 4.5 took the minimum — `BareRepo` making more than
  one origin, and `tests/fixtures/clonelist` pointing at local bare repositories
  the way `cloning` does — because a step may leave a behavior unfinished and
  never leave one untested. What is left here is the strong-claim pass, and a dry
  run is most of it: `git-clone-list` says one line per entry without reaching
  the network, and an existing clone is left exactly as it was, unfetched.

  Most of the apparatus is already there. 4.1 gave `tests/cli/support.rs` a
  `tiny_http` `Server` that counts requests, and 4.3 gave it `BareRepo`, a bare
  repository with a working clone beside it for publishing a second commit, plus
  a `git` helper that forces an identity and no signing. 4.3's `cloning` tests
  are the pattern for asserting the strong claim rather than the weak one: an
  existing clone's `.git/FETCH_HEAD` staying absent is what says no git ran, the
  way `Server::requests` says nothing was asked of the server. 4.5 wrote one list
  test that way — a dry run over clones that are already there — and landed the
  case one repository could not have: an entry that fails leaves the entries after
  it cloned. What is left is the rest of that pass, and the shapes only a list can
  be in: an entry that fails on its `ref`, one whose clone is already there beside
  one that is not, and a `dest-dir` that is somebody else's directory.

  Promote what is left of `docs/future/cmdline.md`'s "Remote content is
  described, not retrieved" paragraph into the section 2.4 created — 4.1 took
  `fetch-file`'s share and the digest sentence beside it, and 4.3 took
  `git-clone`'s along with the no-git-under-dry-run rule the list inherits —
  minus its second half about inclusions, which waits for 7.1.
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
  record that sets both. The collapse this step was once going to perform has
  already happened: `Action::common` is the one exhaustive match over the shared
  fields, so the two conditions are two fields added to `Common` rather than a
  fourth accessor with a match of its own. The same two fields
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
- **6.2** Materialize a declared remote into `remotes/<id>/` by calling
  `git::clone_or_update`, which already does both halves of this. Pass `None` for
  the ref a remote does not declare: 4.5 left that a plain parameter rather than a
  descriptor struct, and this is the third caller, the one entitled to decide
  otherwise (rule 3). It takes no exception argument and must not grow one, so under `--dry-run` nothing is
  cloned or fetched here either; 6.6 is where that is stated and tested. Note
  that a remote's warnings arrive already written for a destination in the home
  — "not updating `<path>`: it has uncommitted changes" is an odd thing to read
  about `remotes/<id>/`, and whether that wants different words is this step's to
  decide.
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

## Enhancements

Wanted, but not scheduled and not part of any slice's acceptance. An entry here
is an idea with a reason attached, which is the least that stops it from being
re-derived; a slice claims one by moving it into a numbered step, and until then
nothing above depends on it.

**Deliberately unnumbered.** `tests/hygiene.rs` reads this file for `- **x.y**`
step numbers, so an entry written that way would become an open step that a
`CARRY` marker or a withheld option could name. These are not that.

- **Keep going past the first failure.** `sync` stops at the first action that
  fails (`docs/cmdline.md`, "Exit Statuses"), and `symlink-dir` stops at the
  first child (`docs/repoformat.md`, `symlink-dir`). Either as an option or as
  how it works, a run would attempt what it can and report every failure at the
  end.

  Step 3.8 is where this stopped being hypothetical. Adopting batfiles on a
  machine that already has hand-written dotfiles means one refusal, one `mv`,
  and another whole run, for as many conflicts as there are — and the run
  reports one of them per attempt while knowing about none of the rest, because
  it never got there. Ten conflicts is ten runs.

  Two things it has to settle. **Which failures are worth continuing after**: an
  occupied destination is independent of everything else in the manifest, but a
  `dest-dir` that could not be created makes each of its children fail for the
  same reason, and thirteen lines restating one cause is worse than stopping.
  **What the exit status and the report look like** when a run both did work and
  failed, which is a state nothing in the tool produces today.

  Note that 9.4's backup policy removes the most common reason to want this,
  since an unmanaged destination stops being a failure at all. That makes the
  two worth reading together, and it is an argument for order rather than
  against the enhancement: what is left afterwards is a genuinely broken run
  wanting to say everything it found.

- **Archive entry filters.** `include` and `exclude` on `fetch-archive`, matched
  against an entry's path with `archive-root` already stripped. Specified in
  `docs/future/repoformat.md`, refused by the closed record today, and left
  unbuilt at 4.2 for two reasons worth keeping: a named `archive-root` already
  installs one directory out of an archive and nothing beside it, which covers
  what either repository driving this project would have wanted a filter for,
  and `GlobFilter` is not ported until 7.3. Whichever of those changes first is
  when this is worth revisiting.

  The same fields are specified and unbuilt on `copy`, `copy-dir`, and
  `symlink-dir`, so 7.3's port is likely to make all four cheap at once. That is
  an argument for doing them together rather than for doing this one early.

- **More archive formats.** 4.2 accepts gzipped and plain tar, decided by
  sniffing the archive's leading bytes, and names what it saw when it refuses
  anything else. Zip is the one worth adding first — it is what a Windows-facing
  release publishes — and it is the one that would change the shape of the
  extraction: zip needs random access, where a tar is read straight through. The
  scratch file 4.2 downloads to already gives it that, so the cost is a
  dependency and a second reader rather than a rework. `bzip2`, `xz`, and `zstd`
  are each one more decompressor in `format_of`, and each brings a C toolchain to
  a build that has none.

  It has no step because nothing wants one yet: neither target repository
  fetches an archive at all, and the refusal names the format, so a repository
  that needs one finds out immediately rather than getting a corrupt install.

- **Address individual `git-clone-list` entries.** `<action-id>.<entry-id>`, the
  same shape a remote-qualified address has, resolving against a list's manifest
  rather than an inclusion. **This one is specified and unbuilt rather than
  undecided**: the form is in `docs/future/cmdline.md`'s address table, what
  `apply-action` does with it is in the bullet above that table, and how the
  lookup reaches execution time is `docs/future/repoformat.md`'s "Deferred
  manifest expansion" — the plan carries the requested, disabled, and skipped
  entry addresses into an opaque node, and the manifest is read when the action
  runs.

  It has no step because nothing in slice 4 or slice 7 gives an entry a name.
  4.4 and 4.5 build the list format and the action; 7.2 makes a qualified
  address resolve to a *spliced action*, which is the other row of the same
  table. 3.7 already did the part that is easy to forget: a dotted address
  parses today, `disabled.toml` and `--skip-action` already record one, and both
  already report that it matched nothing. What is missing is only the lookup.

  The personal repository is the acceptance. `BLACKLIST_VIM_MODULES`, read from
  a sourced `~/.dotfiles-local`, is per-machine skipping of individual vim
  bundles — which is `disable-action vim-bundles.YouCompleteMe` and nothing
  else. Until then the shell variable has no equivalent, and 4.8 retires the
  script that reads it.
