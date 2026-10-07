# `symlink`

Declares one symlink, from a path in the repository to a destination.

```toml
[[actions]]
type = "symlink"
id = "zshrc"
source = "shell/zshrc"
dest = "~/.zshrc"
```

| Field    | Type   | Required | Description                                                        |
|----------|--------|:--------:|--------------------------------------------------------------------|
| `source` | repository path |   yes    | The source, within this repository or a remote it names. Never empty. |
| `dest`   | string |   yes    | The destination path, as written. Never empty; `~` is the home.    |

`source` is the target; `dest` is the link. Follow the shared
[path rules](../repoformat.md#sources-and-destinations), [destination policy](../safety.md#replacing-what-is-already-there),
and [source-containment check](../safety.md#installing-into-what-you-install-from).

Symlink actions require Unix. On unsupported platforms, the action fails by type
before inspecting its destination; it never substitutes a copy.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
