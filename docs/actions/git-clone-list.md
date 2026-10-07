# `git-clone-list`

**Unreleased:** selecting individual clone-list entries by address requires
a build newer than 0.1.0.

Declares every repository a list names, cloned under one directory.

```toml
[[actions]]
type = "git-clone-list"
id = "zsh-plugins"
source = "manifests/zsh-plugins.txt"
dest-dir = "~/.oh-my-zsh/custom/plugins"
```

| Field      | Type   | Required | Description                                                        |
|------------|--------|:--------:|--------------------------------------------------------------------|
| `source`   | repository path |   yes    | The list, within this repository or a remote it names. Never empty. |
| `dest-dir` | string |   yes    | The directory the clones are made in. Never empty; `~` is the home. |

The source uses [repository path syntax](../repoformat.md#sources-and-destinations) and the
[clone list format](../repoformat.md#the-clone-list-format). Each entry's repository is passed
to Git unchanged, regardless of where the list lives. Diagnostics identify the
list by its declared source, including any remote qualifier.

Selected lists must exist during [preparation](../cmdline.md#clone-list-preparation).
The destination container is created even for an empty list. Entries run in
list order with `git-clone` behavior and [entry-specific failures](../cmdline.md#clone-list-entry-failures).
An entry with an `id` can be [addressed](../cmdline.md#addresses) individually.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
