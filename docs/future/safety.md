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
[dry-run behavior](../cmdline.md#dry-run-behavior).

## Destination paths

Promoted at 0.10 to
[Sources and destinations](../repoformat.md#sources-and-destinations), where
these rules are stated for every action type rather than for `symlink` alone.
What remains here has no code yet.

A `git-clone-list` manifest's per-entry `dest-name` is not a destination path
and is not governed by those rules: it is restricted to one ordinary directory
component beneath the action's `dest`, as defined by the
[Git clone manifest format](repoformat.md#git-clone-manifest-format).

Before mutation, batfiles should resolve and display the effective destination
in plans. Both halves of that run: a diagnostic names the resolved destination,
and so does a [dry run](../cmdline.md#dry-run-behavior).

## Repository source paths

Promoted at 0.10 to
[Sources and destinations](../repoformat.md#sources-and-destinations), minus the
remote materializations below, which are not built.

A source path resolves from its owning leaf repository *or remote
materialization*, and the tool-owned `remotes/` tree is inside the selected
batfiles directory and is a valid source location. Until remotes exist, the
leaf repository is the only thing a source can be contained by, which is how the
promoted rule is written.

## Replacement and backups

Steps 1 through 3 and the definition of a batfiles-owned symlink were promoted
at 0.10 to
[Replacing what is already there](../repoformat.md#replacing-what-is-already-there),
minus the remote materializations, which are not built. Step 4 is what remains:
until the backup policy exists at 9.4, an unmanaged destination is an error
rather than something to preserve and replace, and `--no-overwrite` and
`--interactive` are refused rather than honored.

4. Otherwise, preserve the existing node in a recoverable backup before
   replacing it.

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

Promoted at 1.3 to [`copy`](../repoformat.md#copy), which is the only action
that installs content carrying permissions of its own. What is written there is
this section minus the replacement clause: a copied file receives the source's
permission bits including the executable one, a copied directory receives the
source directory's, synthetic parents that exist only to reach a destination
take the platform default subject to the umask, and ownership is not copied.

What remains here has no code yet. Replacing a destination does not preserve the
old destination's permissions; its adjacent backup preserves them, while the
replacement follows the source. Batfiles does not elevate privileges to
reproduce another owner. Symlink permission bits are not portable and carry no
separate guarantee.

## Seed actions and deletion

The copy half was promoted at 1.3 to
[Seeds do not replace, and so do not refuse](../repoformat.md#seeds-do-not-replace-and-so-do-not-refuse)
and the two `copy` action sections, and the build changed it in one way worth
recording. This section had one `copy` whose `dest` was an exact destination or
a merge root depending on the source type, with the missing-only check applied
recursively to every mapped entry below a merge root and existing directories
traversed as merge points. What was built is two actions and one level: `copy`
installs one thing at one name and does nothing at all if something is at that
name, `copy-dir` installs each direct child on its own, and a child directory
that already exists is kept whole rather than descended into. Deep merging is
what interleaves two configurations that were never written to combine, and it
can be added later without changing what a working manifest does; taking it away
afterwards could not.

What remains here has no code yet.

`fetch-url` actions are missing-only seeds during `sync` and `apply-*`. For a
fetch without extraction, the existence check applies to the exact `dest`: if
any filesystem node already occupies that path, the action skips it.

For an extracting fetch, `dest` is instead a merge root. Its existence does not
skip the action. The existence check applies to each selected entry at its
mapped path below that root. A selected file or symlink is installed only when
its mapped path is absent; any existing node at that path is left unchanged. A
selected directory, including an empty one, is created when absent; an existing
directory at that path is traversed as a merge point. If a non-directory node
occupies a path where a selected directory is required, batfiles leaves that
node and the selected directory subtree unchanged and reports the skip. Whether
that survives contact with an implementation the way the `copy` version did not
is for slice 4 to find out.

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
2. Each enabled action then runs in one pass: it inspects the filesystem as the
   previous action left it and acts on what it finds, rather than on anything
   decided for it earlier.

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

Single-file writes that carry content should use a temporary sibling and atomic
rename where practical. Content is what the rule is about: a node batfiles can
rebuild from the manifest — an owned symlink — is replaced in place and simply
redone if a run is interrupted, per
[Replacing what is already there](../repoformat.md#replacing-what-is-already-there).

The build went further than "where practical" and further than "single-file",
and the sentence that used to follow — that multi-file directory updates are
neither atomic nor automatically rolled back — is no longer true of the actions
that exist. `copy` and `copy-dir` build a whole directory beside its destination
and move it in with one rename, so a directory install is atomic and an
unfinished one leaves the destination untouched; see
[Seeds do not replace](../repoformat.md#seeds-do-not-replace-and-so-do-not-refuse).
It is stated as a rule rather than a preference because the failure it prevents
is a run that reports success over a half-installed destination forever. The
actions that do not exist yet inherit it — `fetch-url` and archive extraction
most of all, being seeds over the same destinations. As the first phase of a directory action's execution, batfiles
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
