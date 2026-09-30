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

Begin after slice 8 is in real use.

[Distribution](distribution.md) specifies all of it.

- **10.1** The [release tree](distribution.md#release-tree): targets, assets,
  the [release base parameter](distribution.md#the-release-base-parameter)
  compiled into the binary, the `dist:binary` and `dist:assemble` tasks, and the
  tag-triggered workflow. Accepted when a tagged release publishes a complete,
  verifiable asset set. Its installers may be unstamped sentinels until 10.2.
- **10.2** The POSIX [hosted installer](distribution.md#hosted-installer), its
  GitHub Pages copy, and [`dist:mirror`](distribution.md#self-hosting), with
  `shellcheck` in `task lint`. Accepted by extending the [pristine-machine
  acceptance](../architecture.md#the-pristine-machine) against a local release
  tree built by `dist:assemble`: the `clone` one-liner on a machine with no
  batfiles, an existing binary used without a download, and a checksum mismatch
  refused.
- **10.3** The [leaf stub](distribution.md#leaf-stub), added to the skeleton
  [`init`](../cmdline.md#init) lays down. Accepted in the same container: the
  stub with no binary, with one present and the base unreachable, piped with no
  checkout, and a pin passing over an older binary.
- **10.4** [`batfiles update`](distribution.md#batfiles-update), tested against
  a loopback release tree.
- **10.5** The [Windows](distribution.md#windows) installer and stub.

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
- **Entry filters.** Add include/exclude filters to archives and local directory
  actions. Archive filters match paths after root stripping. There is nothing to
  share with the inclusion filters 7.3 built beyond the one-or-many spelling:
  those compare IDs for equality against what a manifest declared, and these
  match globs against paths. Building `GlobFilter` is this proposal's own work.
- **Additional archive formats.** Consider ZIP and other compressed tar formats.
  The existing scratch file permits random access; assess dependencies and
  cross-platform builds for each reader.
- **Individual clone-list addresses.** Resolve `<action-id>.<entry-id>` for
  apply, disable, and run-only skips. Dotted addresses already parse; add lookup
  against prepared list entries. Accept per-machine plugin selection, such as
  `disable-action vim-bundles.YouCompleteMe`. Needs lists read before
  unmatched-skip warnings and target resolution, which today precede
  preparation; a way for a qualified target to reach into a clone list as
  `Selection::reaches_into` does for inclusions; and a per-entry "not
  requested" state beside `PreparedEntry::exclusion`.
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
