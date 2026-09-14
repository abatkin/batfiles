# Batfiles Safety Model

## Purpose and scope

Batfiles manages user-owned configuration on the user's behalf. Its safety
model is intended to prevent accidental data loss while keeping explicit
configuration useful and predictable. It is not a sandbox or a security
boundary: a repository selected by the user can name destinations outside the
home directory, included repositories can contribute installation actions, and
allowed dynamic variables can execute arbitrary commands as the current user.

These are unbuilt proposals. Implemented safety behavior is specified in
[installation safety](../safety.md).

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
[proposed dry-run behavior](cmdline.md#dry-run-behavior).

## Destination paths

Current [path and destination safety](../safety.md) applies to new action types.

## Repository source paths

Remote source resolution is implemented. New remote types must follow the
current [source syntax](../repoformat.md#sources-and-destinations) and
[path safety rules](../safety.md#path-resolution).

## Replacement and backups

The current [replacement rules](../safety.md#replacing-what-is-already-there)
refuse unmanaged destinations. Step 9.4 adds backups and conflict options.
Preserve an unmanaged node in a recoverable backup before replacing it.

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
rules in [`docs/state.md`](../state.md#writing).

## Installed permissions

A refreshed replacement follows its source permissions; its adjacent backup
preserves the old destination's permissions. Do not elevate privileges to
reproduce another owner. Current [installed permissions](../safety.md#installed-permissions)
remain the default for newly created content.

## Seed actions and deletion

Extend the current [seed policy](../safety.md#seeds-do-not-replace-and-so-do-not-refuse)
with explicit refresh and backups at 9.4.

Seed actions do not modify an existing entry merely because the source has
changed. The user must pass `--refresh-content` to force seed actions to run
again, following the replacement and backup rules above.

Batfiles does not implicitly mirror or prune destination trees. Files present
only at the destination remain there unless an explicitly documented operation
requires their replacement. This reduces the chance that changing an include
filter or upstream archive unexpectedly deletes local content.

## Git repositories

That a Git remote's materialization follows the current [Git update
policy](../safety.md#git-updates) is built and specified there.

Network and repository trust still apply. Batfiles does not guarantee signed
commits or immutable branch contents. Updating a declared Git remote can
change files and included actions on the next plan, so users should pin or
otherwise control sources where upstream mutability is unacceptable.

## Downloads and archives

Remote file and archive materialization must follow the current
[staging](../safety.md#staging-and-publication) and
[archive validation](../safety.md#archive-extraction) rules.

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

Expand included actions in place before capturing selection and preparing clone
lists. Preserve the current [execution order](../cmdline.md#sync): each action
inspects the filesystem left by earlier successful actions. Inclusion does not
add cross-action destination conflict detection.

## Planning, errors, and recovery

Refresh and backup operations must report completed changes and recovery paths
when they fail. They do not make the entire action list transactional. Apply
refresh overlays in deterministic order and retain adjacent backups for every
replaced destination node.

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
