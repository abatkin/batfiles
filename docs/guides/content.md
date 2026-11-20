# Manage files and downloads

Choose an action based on how the installed content should behave after the
first run. All actions run in declaration order. Give actions an `id` when you
want to select them individually.

## Choose an action

| Need | Action | On later runs |
| --- | --- | --- |
| Use the repository's live file or directory | [symlink](../actions/symlink.md) | Keep or correct the link; Unix only |
| Link each direct child into an existing directory | [symlink-dir](../actions/symlink-dir.md) | Manage child links; Unix only |
| Ensure a directory exists | [create-dir](../actions/create-dir.md) | Keep an existing directory |
| Install an editable starting file or tree | [copy](../actions/copy.md) | Keep an occupied destination |
| Seed each direct child into a directory | [copy-dir](../actions/copy-dir.md) | Keep occupied child destinations |
| Download one file | [fetch-file](../actions/fetch-file.md) | Keep an occupied destination |
| Download and unpack a tree | [fetch-archive](../actions/fetch-archive.md) | Keep an occupied destination |
| Maintain a Git checkout | [git-clone](../actions/git-clone.md) | Update conservatively |
| Maintain a list of Git checkouts | [git-clone-list](../actions/git-clone-list.md) | Process each entry |
| Run another repository's actions | [include-remote](../actions/include-remote.md) | See [composition](composition.md) |
| Run a command | [run](../actions/run.md) | As its trigger says |

Copies and downloads are *seeds*: after installation, the local content is
allowed to diverge. A seed's ordinary run does not compare or overwrite it.

## Seed an editable configuration

```toml
[[actions]]
type = "copy"
id = "editor"
source = "files/editor.toml"
dest = "~/.config/my-editor/settings.toml"
```

Add the source file to your repository, then preview and apply:

```sh
batfiles apply-action --id editor --dry-run
batfiles apply-action --id editor
```

For a whole directory tree, `copy` uses that directory as `source`. For separate
children merged into a destination directory, use `copy-dir` with `source-dir`.
The [entry filters](../repoformat.md#entry-filters) select children or archive entries.

## Download content

Replace this illustrative URL with a source you trust:

```toml
[[actions]]
type = "fetch-file"
id = "theme"
source = "https://downloads.example.com/editor/theme.toml"
dest = "~/.config/my-editor/theme.toml"
```

A [template](../repoformat.md#templates) can fetch each machine's build:

```toml
[[actions]]
type = "fetch-file"
id = "tool"
source = "https://downloads.example.com/tool/tool-${facts.os}-${facts.arch}.gz"
dest = "~/.local/bin/tool"
decompress = true
executable = true
```

To follow a project's GitHub releases instead of a fixed URL, see
[install from GitHub releases](releases.md).

Add `sha256` with the expected 64-digit digest to verify the downloaded bytes.
Without a digest, identity depends on the chosen source and transport. Use
`fetch-archive` to unpack an archive, and `decompress = true` on `fetch-file`
to expand a gzip or bzip2 stream into a single file. The action references list
supported formats and options; [archive safety](../safety.md#archive-extraction)
defines permitted entries.

## Refresh deliberately

```sh
batfiles apply-action --id editor --refresh-content
```

This rebuilds the seed and compares it with the installed content. If different,
the normal conflict policy applies: back up and replace by default, ask with
`--interactive`, or leave it with `--no-overwrite`. A dry-run refresh does not
build or download the content, so it cannot tell whether the result would differ.

The three refresh options affect different things:

| Option | Refreshes |
| --- | --- |
| `--refresh-content` | Installed copy and download seeds |
| `sync --refresh-remotes` | File and archive sources under `remotes/` |
| `--refresh-vars` | Dynamic variables evaluated for that run |

See [seed refresh](../safety.md#refreshing-seeds), [remotes](../repoformat.md#materialization),
and [dynamic captures](../state.md#when-declarations-are-evaluated).

## Maintain a Git checkout

```toml
[[actions]]
type = "git-clone"
id = "tools"
source = "https://git.example.com/me/tools.git"
dest = "~/src/tools"
ref = "main"
```

Batfiles uses your Git executable, credentials, and SSH configuration. Existing
checkouts with a suitable branch update by fast-forward; local modifications
and divergence are preserved rather than reset. A warning can therefore mean
the checkout was kept. See [Git updates](../safety.md#git-updates).

For several repositories, use a [clone list](../repoformat.md#the-clone-list-format).
Some per-entry failures warn and continue, so inspect warnings even when a
clone-list command exits successfully.

## Run a command after an install

A program often needs a command run once it is installed, and again whenever it
is upgraded. A `run` action following the install does that:

```toml
[[actions]]
type = "fetch-file"
id = "mise"
source = "https://downloads.example.com/mise/mise-${facts.os}-${facts.arch}"
dest = "~/.local/bin/mise"
executable = true

[[actions]]
type = "run"
id = "mise-install"
command = ["~/.local/bin/mise", "install"]
trigger = { after = "mise" }
```

The command runs on the first sync, and after that only when the download is
installed or replaced, including by `--refresh-content`. A leading `~/` in the
program reaches it even before `~/.local/bin` is on `PATH`. Other
[triggers](../actions/run.md#triggers) run a command every time, only at
bootstrap, once, or on an interval. Batfiles records each success in
[`runs.toml`](../state.md#runstoml-run-action-history); use
`batfiles apply-action --id mise-install` to run it again regardless.
