# `create-dir`

Declares one directory, created where nothing is.

```toml
[[actions]]
type = "create-dir"
dest = "~/.local/share/zsh-plugins"
```

| Field  | Type   | Required | Description                                             |
|--------|--------|:--------:|-----------------------------------------------------------|
| `dest` | string |   yes    | The directory to create. Never empty; `~` is the home.    |

The only action with no `source`, because it installs nothing. It is for a
directory whose contents come from somewhere else — a plugin root another tool
clones into, a cache a program expects to find already there — which a manifest
would otherwise have no way to ask for.

`dest` follows [Sources and destinations](../repoformat.md#sources-and-destinations). Missing
directories and parents are created; existing directory contents are preserved.
The [directory-container policy](../safety.md#directory-containers) specifies
symlink handling and what happens to a non-directory in the way.

Every platform batfiles builds for creates directories, so unlike the two
symlink actions there is no platform on which this one is refused by name.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
