# Rewrite Steps

Eleven vertical slices, each ending with usable behavior and a CLI test.
[guidance.md](guidance.md) owns implementation rules. Completed identifiers stay
available to hygiene checks; open steps contain instructions for future work.
See [retirement](docs.md#retirement-at-slice-8) for the slice 8 transition.

## Slice 0 — Walking skeleton

- **0.1** ✅ Establish the crate and documentation split.
- **0.2** ✅ Parse the CLI surface and reject unsupported commands and options.
- **0.3** ✅ Resolve presentation and the four location roots.
- **0.4** ✅ Read the leaf TOML manifest.
- **0.5** ✅ Define the minimal action schema.
- **0.6** ✅ Parse closed action records and validate names and paths.
- **0.7** ✅ Execute symlink actions.
- **0.8** ✅ Diagnose occupied destinations by node type.
- **0.10** ✅ Specify shared destination and symlink safety.
- **0.11** ✅ Check dead-code annotation policy.
- **0.12** ✅ Include Windows compilation in lint checks.
- **0.13** ✅ Check carry-marker syntax and step status.
- **0.14** ✅ Reject unimplemented options before root resolution.
- **0.15** ✅ Exercise a realistic leaf fixture through the CLI.
- **0.16** ✅ Document supported behavior in the project README.

## Slice 1 — The rest of the local actions

- **1.1** ✅ Execute `symlink-dir`, including `dot-prefix`.
- **1.2** ✅ Execute `create-dir` and create empty directory-action containers.
- **1.3** ✅ Execute `copy` and `copy-dir` as missing-only seeds.
- **1.4** ✅ Cover local actions with portable and symlink-specific fixture assertions.
- **1.5** ✅ Share installation, path resolution, and action-pair helpers.

## Slice 2 — Dry-run

- **2.1** ✅ Gate action writes through helpers using `RunMode`.
- **2.2** ✅ Share reporting tense through `Verb`.
- **2.3** ✅ Check filesystem/process imports against the owner inventory.
- **2.4** ✅ Support and document `--dry-run`.
- **2.5** ✅ Verify whole-tree preservation and reporting parity for distinct destinations.

## Slice 3 — Selection and ordering

- **3.1** ✅ Execute in declaration order.
- **3.2** ✅ Support groups and per-action verbose headings.
- **3.3** ✅ Persist `disabled.toml` with atomic writes and enable/disable commands.
- **3.4** ✅ Combine persistent exclusions with CLI and environment skips.
- **3.5** ✅ Parse default-disabled bootstrap entries; adoption remains at 8.3.
- **3.6** ✅ Execute `apply-action` and `apply-group` using shared selection.
- **3.7** ✅ Parse dotted addresses in targets and exclusion lists.
- **3.8** ✅ Verify the personal repository's local actions against a scratch home.

## Slice 4 — Fetching actions

- **4.1** ✅ Execute `fetch-file` through seed installation.
- **4.2** ✅ Extract plain and gzipped tar with archive-root selection and safety checks.
- **4.3** ✅ Clone and conservatively update Git repositories.
- **4.4** ✅ Parse executable clone lists before action writes.
- **4.5** ✅ Execute clone lists and follow declared Git refs.
- **4.6** ✅ Reject dead-code expectations naming undefined or completed steps.
- **4.7** ✅ Cover clone-list failures, existing clones, and dry-run behavior.
- **4.8** ✅ Declare the complete personal repository installation with batfiles actions.

## Slice 5 — Variables and conditions

Use one flat scope, as specified under [Variables](guidance.md#variables).
Reference paths and reusable parsers are listed in [keep.md](keep.md).

- **5.1** ✅ Add static string `[vars]` values and `VarName`.
- **5.2** ✅ Add `vars.toml` and the machine-local variable commands.
- **5.3** Add `BATFILES_VAR_*` and `--var`. Ignore a bare `BATFILES_VAR_`, preserve
  empty values, and warn with the full environment name for invalid suffixes.
  Parse CLI values at the first `=`, checking that delimiter before key validity.
  Invalid CLI keys must fail as usage errors before root resolution or file reads.
  Adapt the reference parser and its CLI tests.
- **5.4** Merge manifest, state file, environment, and CLI values in one function,
  in that precedence order. Record origins for `vars list`.
- **5.5** Add truthiness, `facts` / `env` / `vars` namespace binding, and captured
  environment enumeration. Use the reference coercion policy.
- **5.6** Gate actions and groups on `when` and `unless`; reject both on one
  record. Extend `Action::common`, default-disabled entries, and clone-list
  entries. Validate the default-disabled lists and remove the expectations
  naming this step; entry IDs remain unread until adoption at 8.3.
- **5.7** Make unevaluable conditions close the gate and warn, in both spellings.
- **5.8** Add `vars list`. Separate repository-required roots from state-only
  roots so a state-only resolution cannot carry an unselected batfiles
  directory; normal listing reads the leaf repository while `--machine-only`
  does not.

## Slice 6 — Git remotes, materialization only

No inclusion of remote actions yet.

- **6.1** Add `[remotes]` with Git records and update the supported schema.
- **6.2** Materialize declared remotes under `remotes/<id>/` through
  `git::clone_or_update`. Pass None when there is no declared ref. Keep dry-run
  behavior uniform for every caller; review reporting for materializations.
- **6.3** Add parsed `@remote/path` values to the shared repository-path resolver.
- **6.4** Allow leaf symlink and copy actions to source content from remotes.
- **6.5** Gate remotes on `when` and `unless`.
- **6.6** Document and test that dry runs neither clone nor update materializations.
  Test absent and existing materializations against the local bare fixture,
  snapshot the whole remotes tree, and check direct evidence that no fetch ran.
  Stale materializations remain usable for inspection; 7.1 handles missing ones.
- **6.7** Add a separate local bare repository fixture for remotes.

## Slice 7 — `include-remote`

- **7.1** Read an included remote's manifest. A dry run uses its existing
  materialization, possibly stale. A missing materialization produces a partial
  action list. Add complete/partial reporting here and promote both the staleness
  and partiality rules from `docs/future/cmdline.md`.
- **7.2** Splice included actions into declaration order before capturing
  selection or preparing clone lists. Resolve included list sources from their
  materialization. Give included actions qualified addresses and extend
  `ItemAddress::names` so stored exclusions can match them. Preserve the
  distinction between an unread list and a validated empty list.
- **7.3** Add inclusion action/group selection filters.
- **7.4** Add per-inclusion variable overrides.
- **7.5** Add per-inclusion scopes and layered precedence.
- **7.6** Enforce one-level inclusion: ignore an included remote's own remotes
  and inclusions.
- **7.7** Give every inclusion a stable, unique display label.
- **7.8** Add a synthetic two-remote fixture with overlapping paths and overrides.
- **7.9** Acceptance: assemble the personal and corporate repositories under `sync`.

## Slice 8 — Bootstrap

- **8.1** Add `init`, adapting the reference implementation to current helpers.
- **8.2** Add `clone`: clone a repository and synchronize it.
- **8.3** Add bootstrap precedence and default-disabled adoption. Remove the
  dead-code expectations for adopted entry fields. Update the CLI assertions
  that candidates currently disable nothing and create no `disabled.toml`.
- **8.4** Add a pristine-machine Docker test through `task test:docker`, included
  in `task ci` but excluded from `task test`. Complete the documentation and
  roadmap migration in [docs.md](docs.md#retirement-at-slice-8).

## Slice 9 — Leaves

Build on demand, in any order except that 9.2 requires 9.1. Retain these numbers
when moving the remaining work to the roadmap at slice 8.

- **9.1** Add dynamic-variable records, execution, timeouts, caching, and
  `allow-dynamic-vars`. Register the runner as bookkeeping in the filesystem-owner
  inventory, explicitly recording arbitrary unsandboxed subprocess execution.
  It does not consult `RunMode`. Promote the dry-run cache/execution caveat and
  relevant state and environment sections when implemented.
- **9.2** Add `vars refresh`, including selective refresh by key.
- **9.3** Add file and archive remotes.
- **9.4** Add `--refresh-content`, backups, `--no-overwrite`, and interactive
  conflict handling. Update current destination refusals and remedies. Seed
  refresh must change both the early occupancy decision in `install::seed` and
  publication in `install::publish`; complete content must exist before backup
  and replacement. Review this together with 9.5.
- **9.5** Address concurrent-writer safety across all actions, including the
  inspection/removal/create sequence for symlinks and rename publication for
  directories. Evaluate platform no-replace operations and filesystem fallbacks;
  preserve the current limitation in `docs/safety.md` until all action paths
  meet the stronger contract. Include refresh and backup operations from 9.4.
- **9.6** Report prospective directory creations and link removals once per run.
  Cover repeated parent inspection under a broken `dest-dir` and multiple
  actions sharing a destination container, such as linking and cloning plugins
  into `~/.oh-my-zsh/custom/plugins`. Track reported paths in `RunContext` for
  dry-run output only; do not simulate filesystem changes.

## Slice 10 — Distribution

Begin after slice 8 is in real use.

- **10.1** Release binaries at stable URLs with checksums and platform detection.
- **10.2** Add the `init` installer template, locating batfiles on PATH or
  downloading it into `~/.local/bin`.
- **10.3** Test installation end to end in Docker against a local release server.

## Enhancements

Unscheduled proposals, separate from slice acceptance. Keep these unnumbered so
carry markers and withheld options cannot refer to them as implementation steps.

- **Continue after independent failures.** Allow local actions to proceed after
  network failures or unrelated destination conflicts. Define which errors permit
  continuation, avoid repeated diagnostics for one failed parent, and specify
  final status and partial-success output. Review alongside backup behavior.
- **Entry filters.** Add include/exclude filters to archives and local directory
  actions. Archive filters match paths after root stripping. Consider sharing the
  filter implementation introduced for inclusions at 7.3.
- **Additional archive formats.** Consider ZIP and other compressed tar formats.
  The existing scratch file permits random access; assess dependencies and
  cross-platform builds for each reader.
- **Individual clone-list addresses.** Resolve `<action-id>.<entry-id>` for
  apply, disable, and run-only skips. Dotted addresses already parse; add lookup
  against prepared list entries. Accept per-machine plugin selection, such as
  `disable-action vim-bundles.YouCompleteMe`.
- **Personal installation adoption.** Verify the complete personal manifest on
  the real home when deployment is requested; scratch-home validation alone
  does not establish live adoption.
- **Git command-local overrides.** Review support for `GIT_CONFIG_COUNT` values
  such as one-off proxy/header settings. Current behavior clears these values;
  any change must preserve destination isolation and update environment docs.
