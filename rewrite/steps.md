# Rewrite Steps

Eleven vertical slices. Each ends with a working binary and a `tests/cli.rs`
test. Steps are numbered so other documents can reference them. Read
`guidance.md` first — several steps below are short because the reasoning lives
there.

The order is driven by one thing: **how soon the tool can manage real
dotfiles.** Slice 4 exists because the personal repository needs `fetch-url` and
`git-clone-list` and nothing else exotic, so those come before variables,
conditions, and remotes even though they are individually harder.

A step marked **✅** is done. Nothing else is. A step may also have grown since
it was written: when a slice leaves work for a later step, it records that on
the step, so the instruction is waiting when you get there.

Note that check the `rewrite/README.md` for guidance after Slice 8 is completed, as much of the "rewrite" infrastructure will need to be replaced at that point.

## Slice 0 — Walking skeleton

`batfiles sync` turns one `[[actions]]` symlink record into a symlink on disk.

- **0.1** ✅ Tag the old crate as named in `keep.md`, empty `src/`, and make the
  initial `docs/` cut described in `docs.md`. Work on `main`: the tag is what
  preserves the old crate, nothing merges back, and CI runs only on `main` and
  pull requests. Branch per slice if you want one reviewed.
- **0.2** ✅ Port `cli/` whole, with every command parsed and every unimplemented
  one exiting 2. Promote the command overview, global options, output streams,
  and exit statuses from `future/cmdline.md`. Color resolution is built here
  too, so `future/environment.md`'s color section is promoted alongside them.
- **0.3** ✅ Port the `Reporter` and the four-root resolution so diagnostics and
  paths work from the first commit. Promote location selection and its
  precedence from `future/environment.md`. Two pieces of 0.2 are waiting here
  for their first caller: `ColorResolution::enabled`, which resolves `auto`
  against the terminal for batfiles' own diagnostics, and `Environment`, which
  replaces `app`'s direct reads of `BATFILES_COLOR` and `NO_COLOR`. Every
  command but `version` and `init` now resolves its roots before reporting that
  it is unimplemented, and `-v` prints the four it resolved, which is what makes
  resolution observable from `tests/cli.rs` before anything reads a root.
- **0.4** ✅ Port the read half of `tomlfile.rs`; leave the atomic-write half out
  until 3.3 needs it. `sync` parses the leaf `batfiles.toml` before reporting
  that it is unimplemented — that is what gives the reader a caller, and it
  makes a missing or malformed manifest fail with the file named. The reading
  rules were promoted into a new `docs/repoformat.md`; the atomic
  whole-document rewrite rule was **not**, because nothing writes yet and
  promoting it would have put unbuilt behavior in `docs/` — it waits at 3.3 with
  the writer, along with `read_or_default` and `Error::is_not_found`, which are
  still at the tag for want of a caller. `Roots::batfiles_config()` came back
  here rather than at 0.6, this step being the first caller. The `tests/cli.rs`
  helper is a temporary tree from here on: the repository is the first root
  anything opens, so fixed absolute paths stopped being safe.
- **0.5** ✅ Define the smallest useful `batfiles.toml`: a list of actions, each
  with an `id`, a `type`, a `source`, and a `dest`. Landed with 0.6, which is
  the parser that enforces the definition; there was no artifact separable from
  it. The record also takes `group`, which costs nothing — it is an `ItemId`
  like `id` — and rejecting it until 3.2 would be rejecting a spelling the
  format already settled.
- **0.6** ✅ Parse it as an internally-tagged enum with one variant, rejecting
  unknown fields. Promote the top-level schema, names and IDs, and the `symlink`
  variant from `future/repoformat.md` into the `docs/repoformat.md` 0.4 started,
  which already holds the layout and the reading rules. This is where the
  manifest's file name finds its owner: `Roots::batfiles_config()` exists but
  spells `batfiles.toml` inline, because 0.4 had no parser to hold the name.
  Give the document type a `FILE_NAME` and point the accessor at it, which is
  the split the tag used — which root a document lives under is location policy
  and stays next to the resolution, while each file name travels with its
  parser. The remaining four accessors are still at the tag: `disabled()`
  follows at 3.3, `machine_vars()` at 5.2, `remotes_dir()` at 6.2, and
  `dynamic_vars()` at 9.1.

  Two things it left, both mechanically enforced rather than remembered:
  `source` and `dest` carry `expect(dead_code)` until **0.7** reads them, and
  `group` until **3.2** does, and each annotation fails CI at the step that
  gives its field a reader (`guidance.md`, rule 1). `validate()` carries one
  rule, duplicate action IDs, that being the only cross-field rule expressible
  with these fields; **5.6** adds `when` excluding `unless`, and directory mode
  adds the two-mode rule when it lands. `Action::id()` is the first of the
  accessors that match over every variant — at the third, collapse them into one
  `fn common(&self)` as the note there says.
- **0.7** ✅ Execute a single symlink action against the resolved home directory,
  creating the link or repairing one that points somewhere else. `sync` runs and
  exits 0; `src/sync.rs` holds the one action-execution loop, and the source and
  destination resolvers next to it, until `action/` earns its directory at 1.2.
  `Reporter::info` and `Verbosity::shows_info` came from the tag, so `--quiet`
  finally suppresses something.

  Three things it decided that the step did not say, each of which moved work
  across a step boundary:

  - **A destination is only repaired when the link already there points into
    the repository.** Rule 13 is a standing rule rather than a step, and
    repairing an arbitrary link would suspend it for as long as 0.8 took. So
    0.8 is no longer "add the refusal" — the refusal is here, as a single
    `already exists` naming the path. What 0.8 owes is the diagnostic that says
    *which* of a file, a directory, or a link elsewhere it found, which is worth
    doing deliberately rather than as a side effect.
  - **The rule-12 check for `sync`'s nine dead options landed here rather than
    at 0.11**, because 0.7 is the step that makes `sync --dry-run` write to
    `$HOME`. It lives beside `unimplemented` in `app.rs` and is a flat list;
    0.11 lifts it out for the commands that are still stubs.
  - **A source must exist and must resolve inside its repository.** The
    containment half is `safety.md`'s repository-source rule, checked lexically,
    and it is one place, which is the seam 6.3 extends rather than replaces.

  Only the unix symlink call is built. The binary compiles everywhere and a
  `symlink` action fails on sight where batfiles cannot make one, checked before
  anything is inspected or removed. `tests/cli.rs` compiles everywhere too: the
  execution tests are one `#[cfg(unix)] mod linking`, since several build their
  fixtures with `symlink` and none of the rest has a meaningful non-unix form,
  and a `#[cfg(not(unix))]` test covers the refusal. An action type that is not
  platform-specific — `create-dir` at 1.1, `copy` at 1.2 — does not belong in
  that module.

  **Nothing enforces this.** CI is ubuntu-only, so the gating is checked by
  whoever remembers to run `cargo clippy --all-targets --target
  x86_64-pc-windows-msvc -- -D warnings`, which needs `rustup target add` first
  and no linker. Adding it to `task ci` is one line and a target install on the
  runner; until someone decides that is worth the time, expect the gates to rot,
  and re-run that command after touching `tests/cli.rs`.

  Every path here is lexical, and review found the two places where that has to
  be done deliberately rather than by accident: the roots are anchored to
  absolute paths before a target is written into a link, and an existing link's
  target is resolved from the link's own directory before it is compared or
  judged. Both are needed for rule 13 to mean anything — a spelling that starts
  with the repository can leave it, and one that does not can be inside it. Any
  later action that stores a path, or classifies one already stored, needs the
  same treatment.

  What it left: the repair is a remove followed by a create, not a temporary
  sibling and an atomic rename — see 0.9. `tests/cli.rs` grew `repo_file` and
  `one_symlink`; 0.12 still builds the fixture repository. The tests that used
  `sync` as their unimplemented-command specimen now use `clone`, through the
  `a_stub` helper, which each later slice re-points as commands land.
- **0.8** Refuse any destination occupied by something that is not a repairable
  symlink — a regular file, a directory, a link outside the repository — failing
  with the path named and nothing written (`guidance.md`, rule 13). 0.7 already
  refuses all three; what is missing is the diagnostic saying which one it
  found, and the row of `docs/repoformat.md`'s destination table that currently
  reads "anything else" saying so too.
- **0.9** Promote `safety.md`'s destination resolution and symlink traversal
  rules into `docs/`, since 0.7 and 0.8 are the first code they govern. Three
  sections carry a pointer paragraph naming what already runs — destination
  paths, repository source paths, and the first three steps of replacement and
  backups — and the rules themselves are in `docs/repoformat.md` under
  `symlink`, written for one action type. Promoting means lifting them to
  general statements now that they are about to govern `create-dir` and `copy`
  as well, and leaving the `symlink` section pointing at them.

  One thing to promote honestly or fix first: "single-file writes should use a
  temporary sibling and atomic rename where practical" is **not** what 0.7 does.
  Repairing a link removes it and creates the replacement, so there is a window
  in which the destination does not exist. Either make the repair atomic or
  promote the sentence with the exception stated.
- **0.10** Add the `allow(dead_code)` check to `task ci`, so rule 1 is enforced
  from the first commit rather than remembered. It has two halves: no
  `allow(dead_code)` under `src/` at all, and every `expect(dead_code)` carrying
  a `reason`. The second is what keeps rule 1's record-field exception honest,
  0.6 being the first step to use it — an `expect` without a reason is an
  `allow` that gets past the grep. Close the other half of the hole
  while you are here: `task lint` runs `clippy --all-targets`, which compiles
  with `cfg(test)`, so an item reachable only from a `#[cfg(test)]` block is
  never reported dead and no annotation exists for the grep to find. Linting the
  default targets as well is what makes rule 1 actually mechanical.
- **0.11** Add the unimplemented-option check every command calls at entry, and
  populate it from the options ported at 0.2 (`guidance.md`, rule 12). The four
  location options went live at 0.3 and are off the list, and `--quiet` went
  live at 0.7, so what remains is the shared action-execution, selection, and
  bootstrap options. `sync`'s nine are already done — 0.7 could not wait,
  because it is the step that gave `--dry-run` a filesystem to silently write
  to — so this step generalizes `app::unsupported` to the commands that are
  still stubs and moves it out of `app.rs`. Keep 0.7's ordering: the check runs
  before root resolution, because an unsupported option is a status-2 "nothing
  was attempted", and resolving first would let a status-1 missing-home failure
  preempt it on the machines least able to explain why. Note that a stub command
  reports its own unimplemented status anyway, so for those the check only
  changes *which* message they get — it earns its place as each command lands.
- **0.12** Add a real leaf repository under `tests/fixtures/` and CLI tests that
  sync it, assert the symlink, assert an occupied destination fails without
  writing, and assert an unimplemented option fails. All three assertions exist
  as of 0.7, against manifests written inline into a temporary tree; what is
  missing is a repository shaped like a real one, which is a different test —
  several actions over a directory tree that someone might actually keep.
- **0.13** Rewrite the project `README.md` to describe what the binary does
  today, and keep it honest at every slice thereafter.

## Slice 1 — The rest of the local actions

- **1.1** Add `create-dir`.
- **1.2** Add `copy` with its missing-only seed semantics, preserved once
  created.
- **1.3** Extract only what all three variants genuinely share, and not before
  all three exist. The candidates are already visible in `src/sync.rs`:
  destination resolution, source resolution, and creating a missing parent are
  each written for `symlink` alone. This is also where `action/` earns its
  directory and `sync.rs` stops holding both the loop and one action's work.
- **1.4** Extend the fixture and add one CLI test per action type.

## Slice 2 — Dry-run

Do this before a fourth action type exists. See `guidance.md`, "Dry-run".

- **2.1** Split every action into an `effects` phase that inspects and a
  separate `apply` phase that performs.
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
  0.7's loop already runs the list in order and stops at the first failure; what
  is owed here is the test that pins it, which needs two actions whose order is
  observable — one creating what the next depends on — rather than two that
  merely both happen.
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

- **4.1** Add `fetch-url` for a single file, seeded only when missing.
- **4.2** Add archive extraction, rejecting absolute paths, `..` traversal, and
  symlinks escaping the destination root.
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
  record that sets both.
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
- **6.7** Fixture: a local bare git repository standing in for a remote.

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
- **9.4** `--refresh-content` and the backup policy it depends on.

## Slice 10 — Distribution

Its own project. Do not start it before slice 8 is in real use.

- **10.1** Release binaries at a stable URL, with checksums and platform
  detection.
- **10.2** The `install.sh` template `init` writes, which finds `batfiles` on
  `PATH` or downloads it to `~/.local/bin`.
- **10.3** A Docker test that runs the one-liner end to end against a local
  release server, so the suite still never reaches the network.
