# `symlink-dir`

**Unreleased:** `include` and `exclude` require a build newer than 0.1.0.

Declares one symlink per direct child of a directory in the repository, all of
them in one destination directory.

```toml
[[actions]]
type = "symlink-dir"
id = "rcfiles"
source-dir = "files"
dest-dir = "~"
dot-prefix = true
```

| Field        | Type    | Required | Description                                                             |
|--------------|---------|:--------:|---------------------------------------------------------------------------|
| `source-dir` | repository path  |   yes    | The directory whose direct children are linked. |
| `dest-dir`   | string  |   yes    | The directory the links are made in. Created if it is missing.            |
| `dot-prefix` | boolean |    no    | Prefix each installed name with `.`. Defaults to `false`.                 |
| `include`    | glob or list |  no | Children to link; absent, every child. See [entry filters](../repoformat.md#entry-filters). |
| `exclude`    | glob or list |  no | Children not to link.                                                    |

Each direct child is linked, in sorted order, without descending into
directories: a `config/` child becomes one link to everything below it. Adding a
source child requires no manifest change.

`source-dir` and `dest-dir` follow the [path rules](../repoformat.md#sources-and-destinations).
The destination is a [directory container](../safety.md#directory-containers);
each child follows the [symlink policy](../safety.md#replacing-what-is-already-there).
The [containment check](../safety.md#installing-into-what-you-install-from) runs
before creating the container or enumerating children. Child failures follow
[execution failures](../cmdline.md#execution-failures).

- `dot-prefix` fails on an already-dotted child, naming it.
- An empty source still creates `dest-dir`; `-v` reports no children.
- A source that is not a directory fails.
- Unsupported platforms fail before reading the source directory, as for `symlink`.

[Filters](../repoformat.md#entry-filters) match direct child names before `dot-prefix`.
Patterns containing `/` are load errors. Unselected destinations are untouched;
selecting no children still creates `dest-dir` and reports that fact at `-v`.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
