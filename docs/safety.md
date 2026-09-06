# Installation safety

These rules describe implemented behavior. Backup and refresh proposals are in
[future/safety.md](future/safety.md).

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

### Installing into what you install from

Local installation actions refuse destinations that resolve inside their source
directory. Missing destination components are resolved through their nearest
existing ancestor. Directory-wide actions check before creating their container.
A single symlink checks before creating or replacing its destination, but leaves
an already-correct link unchanged. A seed checks containment only when its
individual destination is vacant; a directory-wide copy also checks its container
before enumerating children.

## Replacing what is already there

Inspect an installation destination without following its final symlink:

| Existing node | Symlink installation | Git clone installation |
| --- | --- | --- |
| Absent | Create the link | Clone |
| Link already pointing at the requested source | Leave unchanged | Apply link classification below |
| Link resolving inside the batfiles repository | Replace the link | Remove the link and clone |
| Broken link | Replace the link | Remove the link and clone |
| Directory | Refuse | Validate and update as a clone |
| Other node, including a link reaching content outside the repository | Refuse | Refuse |

Broken means resolution ends with NotFound or NotADirectory, including a target
such as `<regular-file>/child`. Unresolvable link loops are refused. A requested
symlink already pointing at its source is unchanged even if that source is itself
a broken link.

Refusals name the destination and the kind of node found. Symlink diagnostics
include the written target and its resolved form when different. There is no
replacement override yet; move conflicting content aside before rerunning.
Removals are reported at normal verbosity; unchanged nodes at `-v`.

### Directory containers

`create-dir` and the destination directories of directory-wide actions preserve
existing directories and their contents. They follow symlinks to directories.
Missing parents are created; broken links along the required directory path are
removed and replaced with directories. A non-directory at any required component
fails with that path named. Batfiles does not create the target of a broken link.

### Seeds do not replace, and so do not refuse

`copy`, `copy-dir`, `fetch-file`, and `fetch-archive` install only at vacant
individual destinations. Any existing node, including a broken symlink, is kept
and reported at `-v`. They do not inspect or merge existing directory contents.
`copy-dir` applies this rule separately to each direct child.

Local source validation still happens before destination occupancy is checked.
The directory container and containment rules also apply, so an occupied child
does not waive errors in its source or parent setup.

A fetched archive is one seed directory. Declaring `create-dir` for that same
path makes it occupied and prevents extraction. Re-seeding existing content is
an unbuilt [refresh proposal](future/safety.md#seed-actions-and-deletion).

## Staging and publication

Seeds are built at `<destination>.batfiles-incomplete`, beside their destination.
Archive downloads first use `<destination>.batfiles-download`; verification
finishes before extraction into the staging directory. Downloads and extraction
use the same open archive file.

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

### Concurrent writers

Batfiles does not lock installation destinations or guarantee safety against a
concurrent writer. The check and rename fallback are separate operations; another
process can create a node between them. Symlink replacement also has separate
inspection, removal, and creation operations. Do not run simultaneous installs
against the same destinations.

Git clones write directly to their destination. Subsequent runs reject incomplete
or damaged checkouts; see [Git updates](#git-updates). State documents follow the
separate [atomic rewrite policy](state.md#writing).

## Installed permissions

- Copies preserve source permissions, including executable bits. Directory modes
  are applied after their children are copied. Symlinks nested inside copied
  trees are refused. Ownership belongs to the user running batfiles. Parents
  created to reach a destination use platform defaults subject to the umask.
- On Unix, fetched files receive mode `0644`; the process umask does not alter
  that final mode.
- Archive entries keep permission bits `0777` only: setuid, setgid, and sticky
  bits are removed. Missing or unreadable modes default to `0644` for files and
  `0755` for directories. Directory modes are applied after their contents.
  The destination root uses the stripped root directory's mode when available,
  otherwise `0755`.
- Unix mode handling does not apply on Windows. Symlink actions and archive
  symlink entries are unsupported there.

## Archive extraction

Validate archive paths before unpacking selected entries. Any unsafe installed
entry fails the action; it is not silently skipped.

- Entry paths must be relative, without platform prefixes or any `..` component.
  `.` components are removed. These entry-path checks also apply outside a
  selected archive root.
- Nothing may be installed beneath a symlink declared by the archive.
- Hardlink targets must remain inside the selected tree and must not traverse an
  archive symlink. The target must already exist when its entry is unpacked.
- Symlink targets must remain within the extraction tree. They may use `..` to
  climb past a directory, but may not cancel a component declared as a symlink.
- Files, directories, symlinks, and hardlinks are supported. Device nodes and
  FIFOs are refused. Archive metadata records are not installed as content.

For example, `bin/tool -> ../lib/tool` is allowed. A target of
`a/b/../../outside` is refused when `a/b` is a symlink, even if lexical
cancellation appears to keep it inside the tree.

The destination is published only after extraction succeeds. See
[fetch-archive](repoformat.md#fetch-archive) for formats and root selection and
[HTTP transfer rules](repoformat.md#the-transfer-both-fetching-actions-share)
for digest verification.

## Git updates

Both Git action types use the same clone and update rules. Existing clones keep
their configured remotes: changing a manifest's `source` does not repoint one.
Move the clone aside if it needs to be cloned from a different source.

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
[Clone lists](repoformat.md#one-entry-that-fails-costs-that-entry) may continue
past an entry-specific failure; standalone clone actions propagate it.

### Clone validation

An existing destination must be a directory with:

1. A real `.git` directory. A `.git` file or symlink is refused, including linked
   worktrees and submodules.
2. A worktree root that Git resolves to the destination itself. A directory
   inside another checkout or configured with another worktree is refused.
3. A resolvable HEAD. Incomplete or damaged clones are refused with diagnostics.

Destination symlinks use the replacement rules above and are never followed to
update another checkout. Git inherits configuration and credentials subject to
[the environment policy](environment.md#variables-passed-on-to-git).
