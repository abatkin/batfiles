# What to Keep

Handoff for whoever moves code from the old crate into the rewrite. Line counts
are the old file's, given so you can tell at a glance how much of it is expected
to survive.

Nothing here is copied verbatim. Every file gets the `guidance.md` treatment on
the way in: comments cut to what a caller needs, error ceremony folded into the
crate error, speculative fields dropped, tests reduced to the ones testing
something tricky. If a port comes out near its old line count, it was not ported.

## Getting the old code

The old crate is tagged **`before-rewrite-20260819`** on `main`, taken before
`src/` was emptied. That tag is the reference this document points at; nothing
below should be reconstructed from memory. Port one file at a time, at the step
named, and never ahead of it.

## `tests/cli.rs` — the most valuable thing in the old crate

**Take all 47 tests, ported one step at a time.** This is the one place where the
old crate's test count is an asset rather than a liability: they drive the binary
from outside, so they survive a total rewrite of the internals. Several state
contracts that appear in no document — *quiet never suppresses requested data*,
*getting a variable prints the value on standard output alone*, *a no-op mutation
does not rewrite the document*, *an invalid address fails before anything is
written*. Losing those is losing specification, not losing tests.

Port each with the step whose behavior it covers: version, help, usage exit
codes, and the four `--color` cases at 0.2; the toggle suite at 3.3; the `vars`
suite at 5.2. A test whose command does not exist yet waits at the tag until it
does.

Do not port the helpers wholesale — `config_dir`, `write_disabled`, `stderr_of`
and friends were shaped by the old layout. Rebuild fixture helpers as the first
slice needs them and re-point the tests at those.

## Take early, cut lightly

Small, correct, already the right shape.

- **`src/tomlfile.rs`** (382) — step 0.4 (read) and 3.3 (write). The atomic
  whole-document rewrite is exactly right. Collapse the four error variants into
  the crate error, keeping the path in the message.
- **`src/output.rs`** (178) — step 0.3. `Reporter` and `Verbosity` port as-is.
  With `trace.rs` gone, this is the whole of progress reporting; commands write
  to it as they run.
- **`src/config/paths.rs`** (382) and **`src/config/env.rs`** (201) — step 0.3.
  Root resolution and the environment snapshot. Drop accessors for files that do
  not exist yet — `disabled()` returns at 3.3, `machine_vars()` at 5.2,
  `remotes_dir()` at 6.2, `dynamic_vars()` at 9.1 — and add each back at its
  step, spelled the way 0.6 split them: which root a document lives under is
  location policy and stays here next to the resolution, while the file name
  itself is a `FILE_NAME` on the document type, travelling with its parser.
- **`Cargo.toml`** — step 0.1. Take the dependency list, not the essays. Forty
  lines of prose currently justify seven dependencies, which is the doc-comment
  failure in another file; one line saying what uses it is enough. Slice 0 needs
  only `clap`, `serde`, and `toml`, with `assert_cmd` and `tempfile` as dev
  dependencies; of the rest, `simple-expressions` and `gethostname` arrive at
  5.5 and `jiff` at 9.1. Slice 4 adds an HTTP client and an archive reader.

  **`Cargo.lock` is not carried across.** The dependency set changes too much at
  0.1 to make the old one worth keeping — regenerate it and commit the result.
  `AGENTS.md`'s rule to commit the lockfile still holds; it is only this
  particular lockfile that is stale.
- **`src/cli/`** minus `trace.rs` (~700 across `mod`, `actions`, `options`,
  `toggles`, `vars`, `init`, `color`) — step 0.2. Port whole, including the
  `--color` pre-parse, which exists because clap needs a `ColorChoice` before it
  can render a usage error. This is the one place breadth is cheap: it makes the
  product visible on day one, and clap's derive keeps every field live.

## Keep untouched

Repository infrastructure is generic, working, and has nothing to do with the
rewrite. Do not recreate any of it: `.github/workflows/ci.yml`,
`.github/dependabot.yml`, `Taskfile.yml`, `rust-toolchain.toml`, `deny.toml`,
`clippy.toml`, `rustfmt.toml`, `.gitignore`, `LICENSE`.

Four additions land on top of it: the `allow(dead_code)` check at 0.11, the
cross-target build at 0.12, the `CARRY` marker check at 0.13, and
`task test:docker` at 8.4. CI runs `task ci`, so all four arrive through the
Taskfile or the test suite; the one workflow edit any of them needed is 0.12's
`rustup target add` for the cross-lint target, which is runner setup.

## Take, but cut hard

- **`src/item.rs`** (452) — `ItemId` at 0.6, `ItemAddress` at 3.7. Split them;
  the address is not needed for three slices. Budget ~40 lines for `ItemId`.
- **`src/var.rs`** (248) — step 5.1. `VarName` and its `RESERVED` list, same
  budget. The reserved list is load-bearing and must survive intact: it is what
  makes namespace dispatch unambiguous without a precedence rule.
- **`src/repo/action.rs`** (573) — variants arrive one step at a time: 0.6, 1.1,
  1.2, 1.3, 4.1, 4.3, 4.5, 6.1, 7.1. The `#[serde(tag = "type", rename_all =
  "kebab-case")]` plus `deny_unknown_fields` shape is correct; copy that pattern,
  not the whole enum. Note that the fetching variants are **records only** — see
  "Nothing exists yet" below.
- **`src/repo/batfiles_config.rs`** (501) — from 0.6, growing. The document
  struct plus a single `validate()` for cross-field rules is the right answer to
  "serde cannot express `when`/`unless` exclusivity", and it is why no parallel
  raw/validated type family is needed. Port the mechanism at 0.6 with one rule in
  it; add rules as their fields land.
- **`src/init.rs`** (693) — step 8.1. Working and correct, and its `fn git(dir,
  args) -> Output` helper is the pattern for rule 6 — though step 4.3 needs that
  helper first, so lift it out ahead of the rest of this file. Large for what it
  does; expect to lose a third. The `install.sh` template it writes is slice 10,
  not slice 8.
- **`src/toggle.rs`** (297) and **`src/state/disabled.rs`** (207) — step 3.3. One
  implementation over four commands is right.
- **`src/state/vars.rs`** (158) — step 5.2.

## Take later, shelve until then

Genuinely good work, which is exactly what makes it tempting to land ahead of a
caller. Do not.

- **`src/repo/value.rs`** (735) — `RepoPath` at 6.3, `Condition` at 5.6,
  `GlobFilter` and `ItemIdList` at 7.3. **Protect the hand-written visitors.**
  Serde's `untagged` collapses every mistake inside either form of a short/long
  union into "data did not match any variant"; these visitors decide the form by
  TOML type and let the real error survive with its location. Tedious to redo,
  easy to lose. Split the file so each type lands at its own step.
- **`src/condition.rs`** (801) — step 5.5. The `Coercions` policy and the
  `facts`/`env`/`vars` namespace binding are subtle and correct; shelve, do not
  rewrite. Cut the 249 comment lines to the two rules a caller needs: an
  undeclared bare identifier is an error, and `vars.x` is total.
- **`src/repo/remote.rs`** (210) — the git variant at 6.1, file and archive at
  9.3.
- **`src/repo/default_disabled.rs`** (143) — step 3.5.
- **`src/repo/duration.rs`** (264) — step 9.1. `FriendlyDuration` has no other
  caller; `cache` and `command-timeout` are its only two fields.
- **`src/repo/var_decl.rs`** (305) and **`src/state/dynamic_vars.rs`** (237) —
  step 9.1.

## Do not take

- **`src/cli/trace.rs`** (276) — cut. It was built so the CLI shape could be seen
  before any command existed, and then read as a feature. It holds the one
  exhaustive match over every command shape, so it breaks on every CLI change.
  Commands report their own progress through the `Reporter` as they run.
- **`src/reach.rs`** (1,448) — a six-phase staged pipeline built around an
  insertion point for a materialization step that was never written. What it does
  becomes a single function once slice 7 has a real caller; what it knows is
  captured in `docs.md`.
- **`src/scope.rs`** (1,054) — the layered-precedence type family. Step 5.4 is a
  flat four-source map; 7.5 is where per-inclusion scopes first exist, and by
  then the shape should follow from two real callers rather than from this. The
  `Source` enum's ordering is worth reading once and re-deriving.
- **`src/dynamic/`** (1,945) — resolve, run, cache policy, shadowing. Step 9.1 at
  the earliest.
- **`src/repo/model.rs`** (112) — `Leaf`, `IncludedRemote`, `Inclusion`,
  `RemoteState`. Shaped for consumers never written; slices 6 and 7 should define
  their own. One idea in it was earned: `RemoteState` distinguishes materialized,
  present but manifest-less, and not materialized at all, which is exactly the
  distinction dry-run needs at 6.6. Re-derive it there.
- **`src/repo/load.rs`** (826) — the two-phase walk, entangled with `reach`.
  Slices 0, 6, and 7 each grow their own loading, and it should stay smaller than
  this.
- **The 17 error types and their 24 `Display` impls**, wherever they appear. See
  `guidance.md`, rule 5.

## Nothing exists yet for slice 4

The old crate has serde records for `fetch-url`, `git-clone`, and
`git-clone-list`, and **no implementation of any of them**. `Cargo.toml` has no
HTTP client and no archive reader. Slice 4 is therefore the first genuinely new
code in the rewrite and the first new dependencies — expect to add one HTTP
client and one archive reader, and to justify each in the manifest the way the
existing entries are. Budget accordingly: it is a bigger slice than its position
suggests.

## Structure of the new crate

Start flat and let slices add depth:

```
main.rs  app.rs  cli/  output.rs  location.rs  tomlfile.rs
item.rs  var.rs  manifest/  action/  state/
```

Modules are added when implemented behavior needs them. `manifest/` earns its
directory at 0.6 because the manifest genuinely has sections; `action/` earns one
when the third action type lands at 1.3. Nothing gets a directory to hold a
single file, and nothing gets one in anticipation.

Two of these are named for what they hold rather than for what the old crate
called them, because the old names collided. `config` meant three things at once
— the location resolution ported from `src/config/`, the `config_dir` root that
holds `vars.toml` and `disabled.toml`, and `BatfilesConfig`, which is the
manifest — so the resolution is `location.rs` (the term `environment.md` uses)
and the document type is `Manifest`. `repo/` is only ever the manifest schema,
so it is `manifest/`, which leaves "repository" free for the materialization
work that arrives at 6.2. The file lists above still name the old crate's paths,
which is where the salvageable code is.
