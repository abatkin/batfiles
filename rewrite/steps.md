# Rewrite Steps

Eleven vertical slices. Each ends with a working binary and a `tests/cli.rs`
test. Steps are numbered so other documents can reference them. Read
`guidance.md` first — several steps below are short because the reasoning lives
there.

The order is driven by one thing: **how soon the tool can manage real
dotfiles.** Slice 4 exists because the personal repository needs `fetch-url` and
`git-clone-list` and nothing else exotic, so those come before variables,
conditions, and remotes even though they are individually harder.

A step marked **✅** is done. Nothing else is.

## Slice 0 — Walking skeleton

`batfiles sync` turns one `[[actions]]` symlink record into a symlink on disk.

- **0.1** ✅ Tag the old crate as named in `keep.md`, start a branch with `src/`
  emptied, and make the initial `docs/` cut described in `docs.md`.
- **0.2** Port `cli/` whole, with every command parsed and every unimplemented
  one exiting 2. Promote the command overview, global options, output streams,
  and exit statuses from `future/cmdline.md`.
- **0.3** Port the `Reporter` and the four-root resolution so diagnostics and
  paths work from the first commit. Promote location selection and its
  precedence from `future/environment.md`.
- **0.4** Port the read half of `tomlfile.rs`; leave the atomic-write half out
  until 3.3 needs it. Promote the atomic whole-document rewrite rule from
  `future/state.md`, which is cross-cutting and belongs with the reader.
- **0.5** Define the smallest useful `batfiles.toml`: a list of actions, each
  with an `id`, a `type`, a `source`, and a `dest`.
- **0.6** Parse it as an internally-tagged enum with one variant, rejecting
  unknown fields. Promote the layout, the top-level schema, names and IDs, and
  the `symlink` variant from `future/repoformat.md`.
- **0.7** Execute a single symlink action against the resolved home directory,
  creating the link or repairing one that points somewhere else. Add `symlink`
  to `goals.md`'s implemented-actions line, and keep that line current at every
  action thereafter.
- **0.8** Refuse any destination occupied by something that is not a repairable
  symlink — a regular file, a directory, a link outside the repository — failing
  with the path named and nothing written (`guidance.md`, rule 13).
- **0.9** Promote `safety.md`'s destination resolution and symlink traversal
  rules into `docs/`, since 0.7 and 0.8 are the first code they govern.
- **0.10** Add the `allow(dead_code)` check to `task ci`, so rule 1 is enforced
  from the first commit rather than remembered.
- **0.11** Add the unimplemented-option check every command calls at entry, and
  populate it from the options ported at 0.2 (`guidance.md`, rule 12).
- **0.12** Add a real leaf repository under `tests/fixtures/` and CLI tests that
  sync it, assert the symlink, assert an occupied destination fails without
  writing, and assert an unimplemented option fails.
- **0.13** Rewrite the project `README.md` to describe what the binary does
  today, and keep it honest at every slice thereafter.

## Slice 1 — The rest of the local actions

- **1.1** Add `create-dir`.
- **1.2** Add `copy` with its missing-only seed semantics, preserved once
  created.
- **1.3** Extract only what all three variants genuinely share, and not before
  all three exist.
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
- **3.2** Add groups and group membership.
- **3.3** Port the atomic-write half of `tomlfile.rs`, `disabled.toml`, and the
  four enable/disable commands.
- **3.4** Add `--skip` for suppressing an action or group for one run.
- **3.5** Add default-disabled bootstrap entries.
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

- **5.1** Add static string values under `[vars]` in the leaf manifest.
- **5.2** Port `vars.toml` and the `vars set` / `get` / `unset` commands.
- **5.3** Add `BATFILES_VAR_*` and `--var`, warning on and dropping an invalid
  environment name.
- **5.4** Merge those four sources in one function into one flat map, recording
  each value's origin for `vars list`.
- **5.5** Port the truthiness table and the `facts` / `env` / `vars` namespace
  binding.
- **5.6** Gate leaf actions and groups on `when` and `unless`, rejecting a
  record that sets both.
- **5.7** Make an unevaluable condition close the gate and warn, in both
  spellings.
- **5.8** Add `vars list`.

## Slice 6 — Git remotes, materialization only

No inclusion of remote actions yet.

- **6.1** Add a `[remotes]` table with `type = "git"`.
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
