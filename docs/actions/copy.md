# `copy`

Declares one file or one directory, copied to a destination where nothing is.

```toml
[[actions]]
type = "copy"
id = "gitconfig-local"
source = "seed/gitconfig.local"
dest = "~/.gitconfig.local"
```

| Field    | Type   | Required | Description                                                     |
|----------|--------|:--------:|-------------------------------------------------------------------|
| `source` | repository path |   yes    | The file or directory to copy, within this repository or a remote it names. |
| `dest`   | string |   yes    | Where the copy goes, exactly. Never empty; `~` is the home.     |
| `include` | glob or list | no | Entries of a directory source to copy; absent, every entry. See [entry filters](../repoformat.md#entry-filters). |
| `exclude` | glob or list | no | Entries of a directory source not to copy.                     |

Installs an editable copy of a file or directory, under the
[seed policy](../safety.md#seeds-do-not-replace-and-so-do-not-refuse): editing it
does not change the repository, and later runs keep it. A directory is installed
whole, never merged; use `copy-dir` to seed missing
children. [Refresh](../safety.md#refreshing-seeds) also replaces the whole tree.
Paths, [permissions](../safety.md#installed-permissions), and
[containment](../safety.md#installing-into-what-you-install-from) follow shared rules.

Only regular files and directories may occur within the copied tree; nested
symlinks, sockets, FIFOs, and devices fail naming the path. The source itself
may resolve through a symlink.

[Filters](../repoformat.md#entry-filters) match paths relative to `source` at any depth.
Unselected entries are not inspected, so an excluded symlink does not fail.
Selecting nothing seeds an empty directory. Filters on a file source fail,
regardless of the destination. Selection reads the source in both modes before
considering the destination; refresh compares against the filtered tree.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
