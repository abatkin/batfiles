# Batfiles Safety Model

## Purpose and scope

Batfiles manages user-owned configuration on the user's behalf. Its safety
model is intended to prevent accidental data loss while keeping explicit
configuration useful and predictable. It is not a sandbox or a security
boundary: a repository selected by the user can name destinations outside the
home directory, included repositories can contribute installation actions, and
allowed dynamic variables can execute arbitrary commands as the current user.

This document records guiding policy rather than an exhaustive implementation
specification. Implementations should preserve these principles when details
are not yet specified, and should fail with an actionable diagnostic when they
cannot determine a safe course of action.

## Trust model

Batfiles assumes that the user trusts the leaf repository and has deliberately
selected any included repositories. It also assumes that paths explicitly
written by the user express the user's intent. Batfiles does not attempt to
protect a user from a malicious repository that they have chosen to install.

That trust does not extend to every byte received from the network. Downloads
and archives must still be parsed defensively. An archive entry is data, not an
instruction to write outside the extraction root, create a device, or consume
unbounded resources. A configured SHA-256 digest verifies downloaded bytes;
without one, batfiles cannot promise content identity or authenticity beyond
the transport and source selected by the user.

Batfiles runs with the invoking user's permissions and does not elevate
privileges. It does not sandbox Git, dynamic-variable commands, or filesystem
access. In particular, a dry run may execute allowed dynamic-variable commands
and update their cache, as described by the command-line specification's
[dry-run behavior](cmdline.md#dry-run-behavior).

## Destination paths

As applied to `symlink`'s `dest`, these rules run and are specified in
[`docs/repoformat.md`](../repoformat.md#symlink). Step 0.9 promotes the general
statement here once there is more than one action type it governs.

Most destinations are expected to be in the selected home directory, but that
is a convention rather than a containment rule.

- A relative destination is resolved from the selected home directory.
- A destination beginning with `~` uses the selected home directory rather
  than an independently discovered shell home.
- An absolute destination is used as written.
- Lexical components such as `.` and `..` may be normalized, but normalization
  does not turn the selected home into a boundary.
- Batfiles does not canonicalize every path component to prove that the result
  remains beneath the home directory. If a parent component is a symlink,
  ordinary operating-system path resolution follows it.

Consequently, `--home-dir` selects the base for home-relative behavior; it does
not create a filesystem jail. This is deliberate. Users commonly symlink parts
of their home into other volumes, and explicit absolute or traversing paths
must continue to work.

This policy applies to action destination fields. A `git-clone-list` manifest's
per-entry `dest-name` is not a destination path: it is restricted to one
ordinary directory component beneath the action's `dest`, as defined by the
[Git clone manifest format](repoformat.md#git-clone-manifest-format).

Before mutation, batfiles should resolve and display the effective destination
in plans and diagnostics. It must never silently substitute the current
working directory when the selected home cannot be determined.

## Repository source paths

The lexical containment rule runs for `symlink`'s `source` and is specified in
[`docs/repoformat.md`](../repoformat.md#symlink). The `remotes/` tree and the
per-remote materializations below are not built.

Repository-backed sources have a narrower policy than destinations. A source
path is resolved from its owning leaf repository or remote materialization,
and the lexically resolved result must remain within the selected batfiles
directory. The tool-owned `remotes/` tree is inside that directory and is a
valid source location.

Absolute repository source paths are invalid. Relative paths that normalize
outside the selected batfiles directory are also invalid, even if the resulting
outside path happens to exist. Components such as `.` and an internal `..` are
acceptable when the normalized result remains inside the directory.

This is a lexical containment check, not canonicalization of the whole source
path. A symlink deliberately stored inside the batfiles directory may point
outside it and is followed according to ordinary operating-system behavior.

## Replacement and backups

Steps 1 through 3 run for `symlink`, minus the remote materializations, and are
specified in [`docs/repoformat.md`](../repoformat.md#symlink). Step 4 is not
built: until the backup policy exists at 9.4, an unmanaged destination is an
error rather than something to preserve and replace, and `--no-overwrite` and
`--interactive` are refused rather than honored.

Creating an absent path is normally safe. Replacing an existing filesystem
node is destructive and follows a stricter rule:

1. Determine what exists at the destination without treating the final
   symlink itself as its target.
2. If the requested result already exists, do nothing.
3. If the destination is a batfiles-owned symlink, replace it directly.
4. Otherwise, preserve the existing node in a recoverable backup before
   replacing it.

A symlink is considered batfiles-owned only when the destination itself is a
symlink whose target points into the selected leaf repository or one of its
tool-owned remote materializations. Replacing that link does not destroy the
target, and batfiles may repair it without producing a backup. A symlink to any
other location is unmanaged even when its name resembles a batfiles-managed
destination.

The backup is placed next to the node it replaces and uses a collision-resistant
name that never overwrites an earlier backup. Successful output tells the user
where recovery content was placed. Batfiles should not report a replacement as
successful until the backup has completed. A failure after the backup but
before installation may leave the destination absent; the error must identify
the backup rather than hiding that partial outcome.

A backup should preserve the original node type, contents, and access
permissions to the extent supported by the platform. Creating a backup must
not make private content more broadly readable. Renaming the existing node is
preferable to copying it when both recovery semantics and filesystem layout
allow that.

At each unmanaged conflict, interactive mode offers three choices: back up and
replace, replace without a backup, or skip. Back up and replace is the default.
The overwrite choice is an explicit waiver for that one conflict; selecting
interactive mode by itself does not weaken the backup guarantee. Non-interactive
operation remains predictable and backup-first by default, while
`--no-overwrite` skips unmanaged conflicts.

Tool-owned materializations, temporary files, and disposable caches may be
replaced without user-content backups. Machine-local configuration such as
`vars.toml` and `disabled.toml` is not disposable and follows the atomic write
rules in [Local state and cache files](state.md#shared-read-and-write-rules).

## Installed permissions

When a source file carries filesystem permission bits, a newly installed or
replaced destination file receives those permissions, including whether it is
executable. Replacing a destination does not preserve the old destination's
permissions; its adjacent backup preserves them, while the replacement follows
the source.

Ownership is not copied from the source. New content is owned according to the
invoking user and operating-system rules, and batfiles does not elevate
privileges to reproduce another owner. A source without meaningful permission
metadata uses the platform default subject to the process umask. Symlink
permission bits are not portable and carry no separate guarantee.

New destination directories corresponding to source directories follow the
source directory's permissions where the platform supports it. Synthetic
parent directories that exist only to reach a destination use the platform
default subject to the process umask.

## Seed actions and deletion

`copy` and `fetch-url` actions are missing-only seeds during `sync` and
`apply-*`. For a file-like copy or a fetch without extraction, the existence
check applies to the exact `dest`: if any filesystem node already occupies
that path, the action skips it.

For a directory-source copy or an extracting fetch, `dest` is instead a merge
root. Its existence does not skip the action. The existence check applies to
each selected entry at its mapped path below that root. A selected file or
symlink is installed only when its mapped path is absent; any existing node at
that path is left unchanged. A selected directory, including an empty one, is
created when absent; an existing directory at that path is traversed as a
merge point. If a non-directory node occupies a path where a selected
directory is required, batfiles leaves that node and the selected directory
subtree unchanged and reports the skip.

Thus an earlier `create-dir` for an extraction root does not disable a later
extracting fetch; the later action can still seed its missing selected entries.
Seed actions do not modify an existing entry merely because the source has
changed. The user must pass `--refresh-content` to force seed actions to run
again, following the replacement and backup rules above.

Batfiles does not implicitly mirror or prune destination trees. Files present
only at the destination remain there unless an explicitly documented operation
requires their replacement. This reduces the chance that changing an include
filter or upstream archive unexpectedly deletes local content.

## Git repositories

Git repositories are updated conservatively. Batfiles may fetch on every
normal synchronization, but it skips an update with a warning when the working
tree or index contains changes that the operation might disturb. Staged,
unstaged, and relevant untracked content all count; ignored build or cache
files need not block an update unless Git reports that they conflict.

A clean worktree allows batfiles to follow the declared configuration. It may
change the checked-out branch or ref, update the configured remote URL, and
fetch from that remote. Those transitions should be reported because they may
be surprising, but they do not by themselves require a backup.

Clean does not mean disposable. Batfiles must not discard local commits merely
to make a repository match its configured upstream. Fast-forward updates are
safe; divergent history, a non-fast-forward update, or another state requiring
a reset is skipped with a warning unless a future explicit operation defines a
stronger policy. Changing away from a branch does not delete that branch or
its commits.

Network and repository trust still apply. Batfiles does not guarantee signed
commits or immutable branch contents. Updating a declared Git remote can
change files and included actions on the next plan, so users should pin or
otherwise control sources where upstream mutability is unacceptable.

## Downloads and archives

Downloads should be written to a temporary file, verified when a digest is
configured, and only then moved into a tool-owned cache or destination. A
failed download or digest check must not replace existing content.

Archives require stricter handling than user-authored destination paths.
Batfiles should inspect and stage a complete selected archive tree before
merging it into the destination. It must reject entries that would escape the
staging root, including absolute names, lexical `..` traversal, and unsafe
symlink or hard-link targets. Device nodes and other special archive entries
should be rejected rather than created.

No metadata field supplied by an archive is trusted as a resource bound.
Extraction should stream data, count actual uncompressed bytes and entries,
and stop when it reaches an implementation-defined extraction budget. A limit
failure occurs while staging and therefore does not modify the destination.
The exact default limits and any explicit override remain an implementation
decision; no fixed limit can prevent every CPU, memory, disk, quota, or
concurrent-process exhaustion case on every supported system.

These archive rules do not contradict the destination path policy: the user
may explicitly choose an absolute destination, but the archive cannot choose a
different one.

## Action order and overlapping destinations

Enabled actions run in declaration order after `include-remote` actions have
been expanded in place. Each action observes the filesystem state left by all
earlier successful actions. If an action fails, later actions do not run.

Batfiles does not compare destinations across actions, maintain cross-action
ownership, or diagnose overlapping actions as conflicts. It does not freeze a
concrete create, skip, or replace decision for every action based on the
filesystem state at the beginning of the run.

Instead, execution has two levels:

1. Structural planning resolves variables, conditions, included actions,
   sources, and the final action order. A `git-clone-list` remains one opaque
   node whose entries are expanded at execution time, per the repository
   format's [deferred manifest expansion](repoformat.md#deferred-manifest-expansion).
2. Each enabled action inspects the filesystem and determines its concrete
   effects as the first phase of that action's execution, then performs those
   effects.

A later action may therefore skip, back up, replace, or otherwise act on output
from an earlier action according to its ordinary semantics. For example, if an
earlier action creates `f`:

- a later seed-only copy to `f` sees that it exists and skips;
- a later symlink action applies the normal owned-link or unmanaged-destination
  replacement policy;
- a later `create-dir` does nothing if `f` is already the required directory,
  and otherwise applies its normal type-mismatch behavior; and
- a later directory action sees and merges with the tree produced so far.

In particular, a `create-dir` followed by a seed-only directory copy or archive
extraction at the same path leaves the merge root in place and seeds missing
selected entries beneath it.

Batfiles does not warn merely because those actions overlap. Ordering and any
intentional or accidental overlap are the repository author's responsibility.

## Planning, errors, and recovery

The structural plan is an ordered program, not a transaction or a frozen list
of filesystem mutations. Inspection during the first phase of an action's
execution determines that action's creates, skips, Git updates, backups, and
replacements from the state that actually exists at that point. Every
destructive step still checks the filesystem immediately before mutation; the
earlier inspection is not an ownership claim and cannot eliminate races with
other processes.

Single-file writes should use a temporary sibling and atomic rename where
practical. Multi-file directory updates are neither atomic nor automatically
rolled back. As the first phase of a directory action's execution, batfiles
performs a best-effort inspection of the intended overlay. It then applies the
overlay in a deterministic order, leaving adjacent backups for every
destination node it replaces. On failure, it stops, reports what completed,
and leaves completed changes and recovery material in place.

Batfiles never deletes a backup in response to a later error. It may clean up
its own incomplete download or staging files when the destination was not yet
modified and they are not needed for recovery.

## Refreshing directory trees

Refreshing a whole directory is the most potentially destructive case. It uses
a recursive overlay based on the node at each selected relative path:

- If neither node exists at the destination, create the source node.
- If both nodes are directories, merge their selected children and retain
  destination-only children.
- If both are file-like nodes, leave identical content alone; otherwise back
  up and replace the destination node.
- If the node types differ, back up the entire conflicting destination node,
  then create the source node in its place.
- Treat a symlink encountered as a tree entry as a symlink, not as a directory
  to recursively merge through. Parent symlinks explicitly present in the
  configured destination path still receive ordinary OS path resolution.

This avoids making a backup copy of an entire matching destination directory,
which may be enormous and contain unrelated user data, while retaining the
core backup-before-replacement guarantee at each actual collision. It also
means content refresh is an overlay, not an exact synchronization: stale and
unrelated destination entries are not removed.

Exact backup filename formatting and behavior for local source special files
remain implementation details. When it cannot classify or safely copy a local
source node, batfiles refuses that node rather than guessing or recursively
overwriting destination content.
