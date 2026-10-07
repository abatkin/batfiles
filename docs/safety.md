# Installation safety

How batfiles treats what is already at a destination, builds and publishes new
content, and updates Git checkouts.

## What is not sandboxed

Batfiles protects what is already at a destination; it is not a security
boundary. It runs with the invoking user's permissions, and a [dynamic
variable](repoformat.md#dynamic-variables)'s command is an arbitrary program run
as that user, with no sandbox, in dry runs as well as real ones. A leaf
repository's commands always run; an included remote's run only where the leaf
sets [`allow-dynamic-vars`](repoformat.md#git) on it. What such a command does
besides printing its value is outside every rule below.

Batfiles assumes the user trusts the leaf repository and deliberately selected
its included repositories. Those repositories can name destinations outside the
home directory and contribute installation actions. Git and filesystem access
are not sandboxed, and batfiles does not elevate privileges.

A configured SHA-256 digest verifies downloaded bytes. Without one, content
identity and authenticity depend on the transport and source the user selected.
Batfiles does not require signed Git commits or make branches immutable; pin a
commit or otherwise control the source when upstream changes are unacceptable.
Updating a remote can change both files and included actions on the next run.

Archive paths and entry types are validated under the [extraction
rules](#archive-extraction), but extraction and compressed-file expansion have
no overall byte or entry budget. Those limits are a
[potential enhancement](enhancements.md#extraction-budgets).

## Path resolution

Written paths are validated when the manifest is read. Repository and home roots
are anchored to absolute paths for execution. Path construction removes `.` and
cancels `..` lexically; existing parent symlinks are followed by the operating
system. Source containment is lexical, so a symlink stored in a repository may
point outside it.

Existing symlink targets are resolved from the link's physical parent directory
and compared with the resolved repository root. New symlinks store the anchored
source path using the selected repository spelling. See
[source and destination syntax](repoformat.md#sources-and-destinations).

A path names a location; a *node* is whatever filesystem entry occupies it: a
file, a directory, a symlink, or anything else. The rules below classify and
settle nodes, not paths.

### Installing into what you install from

Local installation actions refuse destinations that resolve inside their source
directory. Missing destination components are resolved through their nearest
existing ancestor. Directory-wide actions check before creating their container.
A single symlink checks before creating or replacing its destination, including
before removing a replaceable link or setting aside an unmanaged node, but leaves
an already-correct link unchanged. A seed checks containment when its individual
destination is vacant or is being [refreshed](#refreshing-seeds); a
directory-wide copy also checks its container before enumerating children.

The reverse is refused too wherever a destination would be set aside: a
symlink or copy whose source is at or inside the node in its way. Setting that
node aside would take the source with it, so the action fails naming both
before anything is asked or moved. The source counts as inside when resolving
its path, one component at a time and following each link as the operating
system does, passes through the destination node — located by its resolved
parent and its own name, so a destination that is a link is that link. That
covers a source reached through a repository alias as well as one written
under the destination.

## Replacing what is already there

A destination is inspected without following its final symlink:

| Existing node | Symlink installation | Git clone installation |
| --- | --- | --- |
| Absent | Create the link | Clone |
| Link already pointing at the requested source | Leave unchanged | Apply link classification below |
| Link resolving inside the batfiles repository | Replace the link | Remove the link and clone |
| Broken link | Replace the link | Remove the link and clone |
| Directory | [Conflict](#conflicts-and-backups) | Validate and update as a clone; a directory that is [not a usable clone](#clone-validation) is a conflict |
| Other node, including a link reaching content outside the repository | Conflict | Conflict |

Broken means resolution ends with NotFound or NotADirectory, including a target
such as `<regular-file>/child`. Unresolvable link loops are refused. A requested
symlink already pointing at its source is unchanged even if that source is itself
a broken link.

Conflicts name the destination and the kind of node found. Symlink diagnostics
include the written target and its resolved form when different. Removals are
reported at normal verbosity; unchanged nodes at `-v`.

### Conflicts and backups

An unmanaged node in a destination's way — one the table above calls a
conflict, a non-directory where a [container](#directory-containers) must be,
or a seed being [refreshed](#refreshing-seeds) to different content — is settled
by the run's conflict policy:

| Policy | Selected by | What happens at each conflict |
| --- | --- | --- |
| Back up and replace | Default | The node is renamed to a backup beside it, then the action installs |
| Skip | `--no-overwrite` | The node is left, nothing is installed there, and the run goes on |
| Ask | `--interactive` | The user chooses back up and replace (the default answer), overwrite, or skip |

A backup is the node itself, renamed to
`<name>.batfiles-backup-<YYYYMMDDTHHMMSSZ>` in the same directory. The stamp is
the run's start in UTC, shared by every backup the run makes; where that name is
taken, `-2`, `-3`, and so on are appended, so a backup never replaces an earlier
one. Renaming keeps the node's type, contents, permissions, and ownership, and
makes nothing more readable than it was. A directory is backed up whole.
Batfiles never removes a backup.

Overwriting, which only `--interactive` offers, is a waiver for that one
conflict: the node is renamed to `<name>.batfiles-old`, which must be vacant,
and removed once the replacement is in place. A removal that fails warns and
leaves the path.

Setting a node aside is reported at normal verbosity, before the action
installs: `backed up <dest> to <backup>`, or `discarded <dest>`. If the install
then fails and nothing is at the destination, the node is renamed back and
`restored <dest>` is reported before the error. Where it cannot be renamed back,
the error names where it is. A skip is reported as `skipped <dest>: it is <what
is there>`, at normal verbosity, and the run exits 0.

An interactive question is written to standard error, even under `--quiet`,
and one line is read from standard input for each, which need not be a terminal:
`b` or an empty line backs up, `o` overwrites, `s` skips, and anything else asks
again. Standard input ending before an answer fails the run with nothing done at
that destination. [Option rules](cmdline.md#shared-action-execution-options)
say which policies combine with each other and with `--dry-run`.

A dry run settles each conflict as the default would, or skips it under
`--no-overwrite`, and reports the rename it would make, naming the backup path
this run would use.

Tool-owned destinations are not conflicts: an unrecognized node where a
[remote is materialized](#replacing-a-materialization), including one in the way
of the `remotes/` directory, is refused under every policy, and the refusal says
to move it aside. Staging, download, and `.batfiles-old` paths that are already
taken are refused too.

### Directory containers

`create-dir` and the destination directories of directory-wide actions preserve
existing directories and their contents. They follow symlinks to directories.
Missing parents are created; broken links along the required directory path are
removed and replaced with directories. A non-directory at any required component
is a [conflict](#conflicts-and-backups) at that path, followed if it is a link:
backing it up renames the node itself and makes the directory in its place, and
skipping it skips the action, or for a missing parent of one destination, that
destination. Batfiles does not create the target of a broken link.

### Seeds do not replace, and so do not refuse

`copy`, `copy-dir`, `fetch-file`, and `fetch-archive` install only at vacant
individual destinations, unless the run [refreshes content](#refreshing-seeds).
Any existing node, including a broken symlink, is kept and reported at `-v`.
They do not inspect or merge existing directory contents. `copy-dir` applies
this rule separately to each direct child.

Local source validation still happens before destination occupancy is checked.
The directory container and containment rules also apply, so an occupied child
does not waive errors in its source or parent setup.

A fetched archive is one seed directory. Declaring `create-dir` for that same
path makes it occupied and prevents extraction.

### Refreshing seeds

`--refresh-content` installs every seed again over what is at its destination.
The complete content is built at the staging path first, exactly as a new seed
is, and compared with what is there:

- Content that matches — the same node types, bytes, permissions, and link
  targets, and for a directory the same children throughout — is left alone
  and reported `unchanged` at `-v`. Refreshing twice makes one backup, not two.
- A symlink that holds no content of its own, as the table above classifies
  one, is replaced without a backup.
- Anything else is a [conflict](#conflicts-and-backups), asked about only once
  the new content is complete, and replaced by renaming the staged content in.

A directory seed is replaced whole: files only the destination held go with the
backup rather than staying in the refreshed tree. Refresh is not a merge.

A dry run builds and fetches nothing, so it cannot compare; it reports the
replacement a difference would make. Under `--no-overwrite` an occupied seed is
kept without being built, since nothing there could be replaced.

## Staging and publication

Seeds and fetched [remote materializations](repoformat.md#materialization) are
built at `<destination>.batfiles-incomplete`, beside their destination.
[Backups](#conflicts-and-backups) and nodes waiting to be discarded sit beside
it too.
Archive downloads, and downloads declared `decompress`, first use
`<destination>.batfiles-download`; verification finishes before extraction or
decompression into the staging node. Downloads and extraction use the same open
file.

Temporary nodes are created exclusively. An occupied staging or download path
fails and is left untouched. On Unix, staging roots are private while content is built;
final permissions are applied after content is complete. A failed or interrupted
build leaves the destination uninstalled, allowing a later run to retry.

Cleanup removes only paths created by the current operation and is best-effort.
A read-only copied tree, for example, may prevent cleanup. A surviving staging
path blocks another attempt and is named in diagnostics. Inspect it before
removing it manually; the filename alone does not establish ownership.

Completed files are published using a hard link where supported. A destination
that appeared during the build is kept. Directory publication and the file
fallback check occupancy and then rename.

### Replacing a materialization

A file or archive remote's materialization is tool-owned: batfiles
[replaces one](repoformat.md#materialization) that its stamp says it fetched,
without a backup, and refuses one that no stamp claims. Replacement never
exposes a partial materialization:

1. The new content is built and verified at the staging path, as a seed is.
2. The earlier materialization is renamed to `<destination>.batfiles-old`.
3. The new content is renamed into place.
4. The renamed-aside materialization is removed.
5. The stamp is rewritten.

A failure before step 2 leaves the earlier materialization and its stamp in
place. If step 3's rename fails, the earlier materialization is renamed back. An
occupied `.batfiles-old` path fails before anything moves, as an occupied
staging path does. Removal in step 4 is best-effort: one that fails warns and
leaves the path, which then blocks the next replacement until it is removed.

The stamp is not written with the content, so a failure at step 5 leaves the new
materialization in place with a stamp that describes an earlier fetch, or with
none after a first fetch. The error names the materialization and says to delete
it and run `sync` again, which fetches it afresh.

### Concurrent writers

Two batfiles runs sharing a cache directory cannot run at once: the second is
refused by the [run lock](state.md#run-lock). Beyond that, batfiles does not
lock destinations or guard against a concurrent writer, including a batfiles
run with a different cache directory. Several operations check and then act in
separate steps, and another process can change the path between them: the
publication fallback's occupancy check and rename; symlink inspection, removal,
and creation; materialization replacement; the vacancy check before setting a
node aside; and refresh's comparison before replacement. Do not run simultaneous
installs against the same destinations through different cache directories, or
alongside another program writing them.

Git clones write directly to their destination. Subsequent runs reject incomplete
or damaged checkouts; see [Git updates](#git-updates). State documents follow the
separate [atomic rewrite policy](state.md#writing).

## Installed permissions

- Copies preserve source permissions, including executable bits, and so does a
  refreshed copy; its backup keeps the old node's own. Directory modes
  are applied after their children are copied. Symlinks nested inside copied
  trees are refused. Ownership belongs to the user running batfiles. Parents
  created to reach a destination use platform defaults subject to the umask.
- On Unix, fetched files receive mode `0644`, or `0755` where declared
  `executable`; the process umask does not alter that final mode.
- Archive entries keep permission bits `0777` only: setuid, setgid, and sticky
  bits are removed. Missing or unreadable modes default to `0644` for files and
  `0755` for directories. A zip entry has a mode only where the zip records a
  Unix one, as a zip made on Unix does; one made on Windows records none, and
  its MS-DOS attributes are not read as a mode. Directory modes are applied
  after their contents. The destination root uses the stripped root directory's
  mode when available, otherwise `0755`. A file
  [`executable`](repoformat.md#fetch-archive) marks gains `0111` after these
  rules apply.
- Unix mode handling does not apply on Windows. Symlink actions and archive
  symlink entries are unsupported there.

## Archive extraction

Archive paths are validated before selected entries are unpacked. An unsafe
installed entry fails the action rather than being skipped.

- Entry paths must be relative, without platform prefixes or any `..` component.
  `.` components are removed. These entry-path checks also apply outside a
  selected archive root.
- Nothing may be installed beneath a symlink declared by the archive.
- Hardlink targets must remain inside the selected tree and must not traverse an
  archive symlink. The target must already exist when its entry is unpacked, and
  [entry filters](repoformat.md#entry-filters) must not leave it out.
- Symlink targets must remain within the extraction tree. They may use `..` to
  climb past a directory, but may not cancel a component declared as a symlink.
- Files, directories, symlinks, and hardlinks are supported. Device nodes and
  FIFOs are refused. Archive metadata records are not installed as content.
- Entry filters never relax these checks. An entry they leave out is treated as
  one outside the archive root: its path is still checked, and its link target is
  not, since nothing is installed for it. A symlink they leave out still forbids
  installing anything beneath its path.

For example, `bin/tool -> ../lib/tool` is allowed. A target of
`a/b/../../outside` is refused when `a/b` is a symlink, even if lexical
cancellation appears to keep it inside the tree.

The destination is published only after extraction succeeds. See
[fetch-archive](repoformat.md#fetch-archive) for formats and root selection and
[HTTP transfer rules](repoformat.md#the-transfer-both-fetching-actions-share)
for digest verification.

## Git updates

Both Git action types and a declared remote's
[materialization](repoformat.md#materialization) use the same clone and update
rules. Existing clones keep their configured remotes: changing a manifest's
`source` or a remote's `url` does not repoint one. Move the clone aside if it
needs to be cloned from a different source.

| Checkout state | Result |
| --- | --- |
| Already at the target commit and checkout state | Unchanged, reported at `-v` |
| Target is ahead with no local commits | Fast-forward |
| Declared ref requires another branch or detached commit | Check out the target conservatively |
| Staged, unstaged, or untracked changes | Warn and skip the update |
| Current branch has commits absent from the target | Warn and skip advancement |
| Fast-forward would add a path occupied by a local file | Warn and skip advancement |
| No declared ref, and detached HEAD or no configured upstream | Warn and skip the update |
| Configured upstream cannot resolve | Fail with Git diagnostics |
| Git cannot run, fetch fails, or declared ref cannot resolve | Fail |

Ignored files do not make a checkout dirty, but are protected against being
overwritten by an update or ref switch. Untracked files count regardless of
`status.showUntrackedFiles`. A successful branch switch may be reported even
when the subsequent fast-forward is skipped; local branches and commits remain.

The [ref field](repoformat.md#ref-following-one-branch-tag-or-commit) controls
branch versus detached checkout. Submodules are not initialized or updated.
The command reference defines [clone-list failure handling](cmdline.md#clone-list-entry-failures).

A clone whose declared `ref` cannot be resolved stays at its destination in
the checkout state reached before the failure; later runs retry the ref.
Removing an action, list entry, or remote from the manifest does not remove its
clone or materialization. Neither does a
[remote's condition](repoformat.md#a-remotes-condition) that stops passing; the
materialization stays in place but is not read.

### Clone validation

An existing destination must be a directory with the following, or it is a
[conflict](#conflicts-and-backups) for an action and refused for a remote's
materialization:

1. A real `.git` directory Git reads as a repository, as
   `git rev-parse --resolve-git-dir` judges one: its `HEAD`, object store, refs,
   and any `commondir`, following links inside it as Git does. A `.git` file or
   symlink does not qualify, including linked worktrees and submodules, and a
   directory Git answers "not a gitdir" for is a damaged clone. Any other
   failure of that check — Git cannot run, or fails for a reason of its own —
   fails the run rather than calling the checkout damaged.
2. A worktree root that Git resolves to the destination itself. A directory
   inside another checkout or configured with another worktree does not.
3. A resolvable HEAD. An incomplete or damaged clone is named with Git's
   diagnostics.

Such a directory is never updated: backing it up renames it whole and clones
afresh in its place.

Destination symlinks use the replacement rules above and are never followed to
update another checkout. Git inherits configuration and credentials subject to
[the environment policy](environment.md#variables-passed-on-to-git).
