# Roadmap

Remaining work, numbered so that carry markers and withheld options can name the
step that will clear them. `tests/hygiene.rs` reads this file: a step is open
until its entry is marked ✅, and an annotation naming an undefined or completed
step is rejected. Slices 0 to 8 are done and their history is in the commit log.

[`docs/architecture.md`](../architecture.md) owns the rules this work is built
to, and [`AGENTS.md`](../../AGENTS.md#carrying-work-forward) owns how material is
routed here.

## Slice 9 — Leaves

Complete.

- **9.1** ✅ Dynamic variables, their cache, `allow-dynamic-vars`,
  `--refresh-vars`, and `vars list --no-refresh`.
- **9.2** ✅ `vars refresh`, selective refresh by key, and reachability.
- **9.3** ✅ File and archive remotes, `file://` URLs, and `--refresh-remotes`.
- **9.4** ✅ `--refresh-content`, backups, `--no-overwrite`, and
  `--interactive`.

## Slice 10 — Distribution

Complete. [`docs/distribution.md`](../distribution.md) specifies it.

- **10.1** ✅ The [release tree](../distribution.md), the `dist:` and `release:`
  tasks, and the tag-triggered release workflow, accepted by `v0.1.0-rc.1`.
- **10.2** ✅ The POSIX hosted installer, `dist:mirror`, and `shellcheck` in
  `task lint`, accepted in the pristine-machine container.
- **10.3** ✅ The leaf stub `init` writes, the compiled-in release base, and
  `sync --bootstrap`, accepted in a second, fresh container.
- **10.4** ✅ `batfiles update`, the compiled-in target triple and the asset a
  build without a release of its own takes, and `update --check` in
  `dist:smoke`.
- **10.5** ✅ The Windows installer and stub, `init --stubs`, `update` replacing a
  running `batfiles.exe`, `task test` on a Windows runner, and a Windows runner
  in `dist:smoke`.
- **10.6** ✅ The [GitHub Pages](../distribution.md#github-pages) copies of the
  hosted installers, accepted by `v0.1.0` deploying `https://batfiles.dev`.

## Enhancements

Unscheduled proposals, separate from slice acceptance. Keep these unnumbered so
carry markers and withheld options cannot refer to them as implementation steps.

- **Continue after independent failures.** Allow local actions to proceed after
  network failures or other independent errors. Define which errors permit
  continuation, avoid repeated diagnostics for one failed parent, and specify
  final status and partial-success output. Destination conflicts already
  continue under `--no-overwrite`.
- **Run lock.** Hold an advisory lock under the state directory for the whole
  run, so a second batfiles invocation against the same state waits or refuses.
  This covers the realistic [concurrent writer](../safety.md#concurrent-writers):
  two runs started at once. It would also close the lost update that [state
  rewrites](../state.md#writing) permit. Per-operation no-replace renames are
  out of scope; the lock does not guard against other programs.
- **Additional archive formats.** Consider ZIP and other compressed tar formats.
  The existing scratch file permits random access; assess dependencies and
  cross-platform builds for each reader.
- **Personal installation adoption.** Verify the complete personal manifest on
  the real home when deployment is requested; scratch-home validation alone
  does not establish live adoption.
- **Fully qualified Windows host name.** `facts.hostname` comes from
  `gethostname`, which calls `GetComputerNameExW` with
  `ComputerNamePhysicalDnsHostname` on Windows and so drops the DNS suffix a
  domain-joined machine is configured with. Reporting the qualified name means
  calling the same API with `ComputerNameDnsFullyQualified` directly, which adds
  a Windows dependency and unsafe FFI to a crate that has neither, and cannot be
  tested on a runner that only cross-compiles for Windows. Weigh a second fact
  against changing this one, since a manifest comparing the short form would
  break. [`environment.md`](environment.md) documents the current difference.
- **Git command-local overrides.** Review support for `GIT_CONFIG_COUNT` values
  such as one-off proxy/header settings. Current behavior clears these values;
  any change must preserve destination isolation and update environment docs.
