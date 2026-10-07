# `copy-dir`

Declares one copy per direct child of a directory, all of them in one
destination directory.

```toml
[[actions]]
type = "copy-dir"
id = "seeds"
source-dir = "seed"
dest-dir = "~"
dot-prefix = true
```

| Field        | Type    | Required | Description                                                          |
|--------------|---------|:--------:|------------------------------------------------------------------------|
| `source-dir` | repository path  |   yes    | The directory whose direct children are copied. |
| `dest-dir`   | string  |   yes    | The directory the copies are made in. Created if it is missing.       |
| `dot-prefix` | boolean |    no    | Prefix each installed name with `.`. Defaults to `false`.             |
| `include`    | glob or list | no  | Entries under `source-dir` to copy, at any depth; absent, every entry. See [entry filters](../repoformat.md#entry-filters). |
| `exclude`    | glob or list | no  | Entries under `source-dir` not to copy, at any depth.                 |

Each direct child is seeded separately: an occupied child destination is kept,
and its siblings are still seeded. A directory child is one whole seed; nothing merges
into an existing child directory.

Paths, sorted order, `dot-prefix`, empty/non-directory sources, and destination
containers follow [`symlink-dir`](symlink-dir.md#symlink-dir). Permissions and allowed node
types follow [`copy`](copy.md#copy).

[Filters](../repoformat.md#entry-filters) match paths relative to `source-dir` at any depth,
before `dot-prefix`. A child is seeded if it or something beneath it is
selected, and contains only selected entries. A wholly unselected child's
destination is not inspected. Other filter rules are `copy`'s.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
