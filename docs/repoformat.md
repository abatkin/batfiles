# Batfiles repository format

Manifest syntax and action fields. Start with the [README example](../README.md#example).

[Layout](#repository-layout) · [Schema](#top-level-schema) · [Names](#names-and-ids) ·
[Remotes](#remotes) · [Paths](#sources-and-destinations) · [Filters](#entry-filters) ·
[Actions](#actions) · [Clone lists](#the-clone-list-format) ·
[Variables](#variables) · [Conditions](#conditions) · [Bootstrap defaults](#default-disabled-bootstrap-entries)

## Repository layout

A repository is an ordinary file tree with `batfiles.toml` at its root.
Other files matter only when an action references them. Batfiles owns generated
`remotes/`; add it to `.gitignore`.

The **leaf repository** is selected by [location precedence](environment.md#location-selection).
A remote's manifest is optional until an [`include-remote`](#include-remote)
asks to read it.

## Reading the manifest

Every document is read and validated in full before its contents are used,
including before its dynamic commands run. An included manifest is read only
when its inclusion opens, after the leaf's variables have resolved.

- Missing or unreadable manifests fail the command. Missing [state files](state.md)
  are instead empty documents.
- Malformed TOML or invalid schema fails with the file and position; the file
  is left untouched.
- Unknown record fields, invalid value shapes, and cross-record violations
  such as duplicate IDs are load errors. Action diagnostics use one-based
  positions in the manifest.
- An empty manifest is valid. Commands that do not read the repository are
  unaffected by its manifest.

## Top-level schema

All four sections are optional. There is no format-version field.

```toml
[remotes]                  # map<ID, Remote>

[[actions]]                # ordered list<Action>

[vars]                     # map<variable name, string | dynamic variable>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]
```

Records are closed: only documented fields and variants are accepted. Keys in
`[remotes]` and `[vars]` are user-chosen names, validated by the rules below.

## Names and IDs

| Kind | Syntax | Constraints |
| --- | --- | --- |
| Action ID, group, remote ID | `[A-Za-z0-9][A-Za-z0-9_-]*` | No whitespace, dots, or commas |
| Variable name | `[A-Za-z_][A-Za-z0-9_]*` | Cannot be `facts`, `env`, `vars`, `true`, or `false` |

Action IDs must be unique within a repository; a duplicate error names both
records. Remote IDs must also be distinct ignoring case on every platform,
because they name materialization paths. Group, action, and remote names occupy
separate namespaces.

Variable names are case-sensitive. `_hidden` is a variable name but not an ID;
`9front` and `oh-my-zsh` are IDs but not variable names. Dots join IDs into
[addresses](cmdline.md#addresses).

## Remotes

`[remotes]` is a map from a name to a record describing content this repository
does not hold: another repository, a file, or an archive. The key is the remote's
[ID](#names-and-ids):

```toml
[remotes.core]
type = "git"
url = "git@github.com:me/dotfiles-core.git"
ref = "main"
```

Every remote is a closed record selected by its required `type`:

| Field    | Type               | Required | Description                                                     |
|----------|--------------------|:--------:|-----------------------------------------------------------------|
| `type`   | remote-type string |   yes    | Selects the record variant: [`git`](#git), [`file`](#file), or [`archive`](#archive). |
| `when`   | condition          |    no    | See [conditions](#conditions).                                  |
| `unless` | condition          |    no    | At most one of the two.                                         |

A `type` the schema does not name is refused as the manifest is read, because
there is no record behind it to check, and so is a field the selected variant
does not have.

### `git`

A repository for git to clone.

| Field                | Type    | Required | Description                                                                 |
|----------------------|---------|:--------:|-----------------------------------------------------------------------------|
| `url`                | string  |   yes    | A repository for git to clone. Never empty.                                 |
| `ref`                | string  |    no    | A branch, tag, or commit to follow. Never empty.                            |
| `allow-dynamic-vars` | boolean |    no    | Whether an included manifest's [dynamic variables](#dynamic-variables) run. Defaults to `false`. |

`url` follows [`git-clone` source syntax](#git-clone), and `ref` follows its
[ref rules](#ref-following-one-branch-tag-or-commit). `branch` is not a field.

`allow-dynamic-vars` permits commands in an included manifest. Without it,
dynamic declarations there declare nothing, and `-v` names those skipped.
Leaf declarations need no such permission. See
[dynamic-command execution](environment.md#how-dynamic-commands-are-run).

A remote supplies content; an action decides where to install it. Only the
leaf's `[remotes]` is materialized. An included manifest's map is
[ignored](#an-included-manifests-own-remotes).

### `file`

One file, fetched from a URL.

```toml
[remotes.pathogen]
type = "file"
url = "https://raw.githubusercontent.com/tpope/vim-pathogen/master/autoload/pathogen.vim"
```

| Field    | Type   | Required | Description                                                     |
|----------|--------|:--------:|-----------------------------------------------------------------|
| `url`    | string |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL.       |
| `sha256` | string |    no    | 64 hexadecimal digits: the digest the fetched bytes must have.  |
| `executable` | boolean | no  | Whether the file lands executable. Defaults to `false`.         |
| `decompress` | boolean | no  | Whether the download is a gzip or bzip2 stream holding the file. Defaults to `false`. |

The materialization is the file itself, at `remotes/<id>`, so a
[repository path](#sources-and-destinations) names it whole, as `@pathogen` or
`{ remote = "pathogen" }`:

```toml
[[actions]]
type = "symlink"
source = "@pathogen"
dest = "~/.vim/autoload/pathogen.vim"
```

A path under a file remote, or a `source-dir` naming one, is a load error. It is
fetched by the [shared transfer](#the-transfer-both-fetching-actions-share), with
[fetched-file permissions](safety.md#installed-permissions); `executable` and
`decompress` mean what they do for [`fetch-file`](#fetch-file).

### `archive`

A zip or tarball fetched from a URL and unpacked.

```toml
[remotes.fzf]
type = "archive"
url = "https://example.com/fzf-0.65.2.tar.gz"
archive-root = "*"
```

| Field          | Type   | Required | Description                                                            |
|----------------|--------|:--------:|------------------------------------------------------------------------|
| `url`          | string |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL.              |
| `sha256`       | string |    no    | 64 hexadecimal digits: the digest the fetched archive must have.       |
| `archive-root` | string |    no    | A prefix every entry is written without, or `*` for the archive's single top-level directory. |
| `include`      | glob or list | no | Entries to unpack, by their path once the root is stripped; absent, every entry. See [entry filters](#entry-filters). |
| `exclude`      | glob or list | no | Entries not to unpack.                                                 |
| `executable`   | glob or list | no | Files to make executable, by their path once the root is stripped. See [`fetch-archive`](#fetch-archive). |

The archive is unpacked into `remotes/<id>/` as [`fetch-archive`](#fetch-archive)
unpacks into its `dest`, with the same formats, `archive-root`, filters,
`executable`, and [archive safety](safety.md#archive-extraction). Actions read
paths within it as within a Git clone, such as `@fzf/bin/fzf`.

File and archive remotes have no manifest, so [`include-remote`](#include-remote)
cannot name them.

### Materialization

`sync` materializes every admitted remote at `remotes/<id>` before any action,
even if no action uses it. Two IDs naming one URL have separate materializations.

Git remotes clone or update under the [Git policy](safety.md#git-updates),
following `ref` when supplied. An unusable checkout at a tool-owned path is
refused under every conflict policy; see [clone validation](safety.md#clone-validation).

File and archive remotes use a sibling stamp, `remotes/<id>.batfiles-source`,
to record their fetching declaration and detect changes:

| At `remotes/<id>` | Stamp | `sync` |
| --- | --- | --- |
| Nothing | Any, or none | Fetch |
| Node of the stamp's recorded kind | Matches declaration | Keep; report unchanged at `-v` |
| Node of the stamp's recorded kind | Differs from declaration | Refetch and replace |
| Anything else | Missing, unreadable, invalid, or wrong kind | Refuse before fetching |

The stamp covers the type, URL, digest, and type-specific options. Digests
compare case-insensitively; a single pattern and a one-element list compare
equally, but pattern order matters. [Replacement](safety.md#replacing-a-materialization)
is staged and verified; a failed fetch leaves the old materialization and stamp
in place and fails the run. [`sync --refresh-remotes`](cmdline.md#sync) forces
refetching when a declaration is unchanged.

An unclaimed node, including a clone left under an ID now declared as a file
or archive, must be moved aside or removed before retrying. A Git
materialization removes any obsolete stamp beside it.

Materialization failure stops the run before any action. Apply commands and
[dry runs](cmdline.md#dry-run-behavior) fetch nothing and use existing trees,
however stale. An action whose source remote is absent fails naming the remote;
a missing inclusion is handled by [plan completeness](cmdline.md#plan-completeness).

### A remote's condition

```toml
[remotes.corporate]
type = "git"
url = "git@git.example.com:it/dotfiles.git"
when = "work"
```

A remote's [condition](#conditions) decides whether it may be materialized or
read. Every execution command evaluates it, including apply commands. An
excluded remote is neither updated nor read, even if a previous materialization
remains; that tree is not deleted.

An action sourcing an excluded remote fails naming the remote and reason.
Usually it should carry the same condition. An inclusion of the excluded remote
instead contributes nothing. See [exclusion reporting](cmdline.md#exclusion-reporting).

## Sources and destinations

A **repository path** identifies content within the declaring repository or a
declared remote. `symlink`, `copy`, and `git-clone-list` use one as `source`;
`symlink-dir` and `copy-dir` use one as `source-dir`.
Fetching actions and `git-clone` have their own source syntax.

```toml
source = "files/zshrc"                             # declaring repository
source = "@core/files/zshrc"                       # declared remote
source = { remote = "core", path = "files/zshrc" } # same remote path
```

The structured form is closed and requires `remote`; omitting `path` means
`@<remote>`. Both remote spellings resolve and appear in diagnostics identically.

- A remote reference must name a remote in the same manifest. Undeclared
  references fail at load time; absent materializations fail at execution.
- A file remote is named whole (`@pathogen` or `{ remote = "pathogen" }`).
  A path within it or a `source-dir` naming it is invalid.
- Tree paths must be nonempty, relative, and remain strictly inside the tree.
  `.` and `..` are allowed only within that constraint. `.`, `./`, `shell/..`,
  and a tree remote with no path are invalid.
- Use `/` separators. Anchored paths and drive prefixes are rejected according
  to the host's path syntax. Symlinks within a repository may point outside it.
- An initial `@` is reserved with no escape, including inside a structured
  `path`. Elsewhere it is ordinary text, as in `files/@work/zshrc`.

Diagnostics identify the source tree. Sources are checked when an action runs,
including dry runs. A final broken symlink counts as present for linking; copies
must read its target. `source-dir` must resolve to a directory. Clone-list
sources are read during [preparation](cmdline.md#clone-list-preparation).

`dest` and `dest-dir` use the selected home as their relative base:

| Form | Resolution |
| --- | --- |
| `~` | Selected home |
| `~/path` or `path` | Relative to selected home |
| Absolute path | Used as written |
| Empty value or `~other` | Invalid |

Destinations may leave the selected home. Roots are anchored and paths normalized
lexically. [Location selection](environment.md#location-selection) defines roots;
[installation safety](safety.md) defines filesystem resolution, source
containment, conflicts, and staging.

## Entry filters

`include` and `exclude` select tree entries for `symlink-dir`, `copy`,
`copy-dir`, `fetch-archive`, and archive remotes. Each accepts one glob or a list;
archive `executable` patterns use the same matching rules.

```toml
include = "*rc"
exclude = ["private", "**/*.bak"]
```

Paths are relative to the root specified by the action, with `/` separators on
every platform. Patterns match the whole path, case-sensitively.

| Syntax | Matches |
| --- | --- |
| `*` | Any run of bytes within one segment, including a leading `.` |
| `?` | One byte within a segment |
| `[abc]`, `[a-z]`, `[!abc]` | One byte in, or outside, the class |
| `{a,b}` | Either alternative within one segment |
| `**` | Zero or more whole segments; must be a segment of its own |

Only `**` crosses separators: `*.bak` matches at the root, `**/*.bak` at any
depth. Classes and alternatives cannot contain `/`. Backslash is literal;
use `[*]` for a literal `*`. Byte matching means `?.txt` and `[é].txt` both
miss `é.txt`; use the literal character or `*`.

An entry is selected when it or an ancestor directory matches `include`, or
`include` is absent, and neither it nor an ancestor matches `exclude`.
Exclusion wins. `include = []` selects nothing. Unselected, nonexcluded
directories are created only when needed to hold selected descendants.
For example, including `bin` and excluding `bin/secret` installs the rest of
`bin`; including `scripts/lib` creates `scripts` holding only `lib`.

Empty patterns, leading `/`, empty/`.`/`..` segments (including a trailing `/`),
and malformed globs are load errors. Patterns matching nothing are not errors;
`-v` names the pattern and tree.

## Actions

[symlink](#symlink) · [symlink-dir](#symlink-dir) · [create-dir](#create-dir) ·
[copy](#copy) · [copy-dir](#copy-dir) · [fetch-file](#fetch-file) ·
[fetch-archive](#fetch-archive) · [git-clone](#git-clone) ·
[git-clone-list](#git-clone-list) · [include-remote](#include-remote)

`[[actions]]` is an ordered list. Each entry is a closed record selected by its
required `type` field.

| Field   | Type               | Required | Description                                                            |
|---------|--------------------|:--------:|------------------------------------------------------------------------|
| `type`  | action-type string |   yes    | Selects the action variant. `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, `fetch-archive`, `git-clone`, `git-clone-list`, and `include-remote` are the ones that exist. |
| `id`    | `ID`               |    no    | Makes the action addressable.                                          |
| `group` | `ID`               |    no    | Places the action in one group. See [groups](#groups).                 |
| `when`  | condition          |    no    | Runs the action only where the condition is true. See [conditions](#conditions). |
| `unless`| condition          |    no    | Runs it only where the condition is false. At most one of the two.     |

Each variant accepts only its documented fields. A field belonging to another
variant is an error when the manifest is read: for example, `symlink` takes
`source`, while `symlink-dir` takes `source-dir`. Only `symlink-dir` and
`copy-dir` accept `dot-prefix`, which adds a leading `.` to each installed name.

`symlink` and `copy` install one file or directory at one destination.
`symlink-dir` and `copy-dir` install each direct child into a destination
directory. Choose the type explicitly; it is not inferred from the source's
filesystem type. To download and unpack an archive, use
[`fetch-archive`](#fetch-archive); [`fetch-file`](#fetch-file) installs the
downloaded bytes as one file.

[`include-remote`](#include-remote) is the one record that installs nothing of
its own: it names a [remote](#remotes) and takes what that repository's manifest
declares, at its position in this list.

### Groups

A group consists of the actions whose `group` field names it. Each action has at
most one group; there is no `[groups]` declaration. Group values use the
[ID syntax](#names-and-ids), and may share names with actions.
Membership does not change declaration order or require adjacency.

Use groups to apply, disable, or skip members together. See
[selection](cmdline.md#selecting-what-a-run-does).

### `symlink`

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
[path rules](#sources-and-destinations), [destination policy](safety.md#replacing-what-is-already-there),
and [source-containment check](safety.md#installing-into-what-you-install-from).

Symlink actions require Unix. On unsupported platforms, the action fails by type
before inspecting its destination; it never substitutes a copy.

### `symlink-dir`

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
| `include`    | glob or list |  no | Children to link; absent, every child. See [entry filters](#entry-filters). |
| `exclude`    | glob or list |  no | Children not to link.                                                    |

Each direct child is linked, in sorted order, without descending into
directories: a `config/` child becomes one link to everything below it. Adding a
source child requires no manifest change.

`source-dir` and `dest-dir` follow the [path rules](#sources-and-destinations).
The destination is a [directory container](safety.md#directory-containers);
each child follows the [symlink policy](safety.md#replacing-what-is-already-there).
The [containment check](safety.md#installing-into-what-you-install-from) runs
before creating the container or enumerating children. Child failures follow
[execution failures](cmdline.md#execution-failures).

- `dot-prefix` fails on an already-dotted child, naming it.
- An empty source still creates `dest-dir`; `-v` reports no children.
- A source that is not a directory fails.
- Unsupported platforms fail before reading the source directory, as for `symlink`.

[Filters](#entry-filters) match direct child names before `dot-prefix`.
Patterns containing `/` are load errors. Unselected destinations are untouched;
selecting no children still creates `dest-dir` and reports that fact at `-v`.

### `create-dir`

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

`dest` follows [Sources and destinations](#sources-and-destinations). Missing
directories and parents are created; existing directory contents are preserved.
The [directory-container policy](safety.md#directory-containers) specifies
symlink handling and what happens to a non-directory in the way.

Every platform batfiles builds for creates directories, so unlike the two
symlink actions there is no platform on which this one is refused by name.

### `copy`

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
| `include` | glob or list | no | Entries of a directory source to copy; absent, every entry. See [entry filters](#entry-filters). |
| `exclude` | glob or list | no | Entries of a directory source not to copy.                     |

Installs an editable copy of a file or directory, under the
[seed policy](safety.md#seeds-do-not-replace-and-so-do-not-refuse): editing it
does not change the repository, and later runs keep it. A directory is installed
whole, never merged; use `copy-dir` to seed missing
children. [Refresh](safety.md#refreshing-seeds) also replaces the whole tree.
Paths, [permissions](safety.md#installed-permissions), and
[containment](safety.md#installing-into-what-you-install-from) follow shared rules.

Only regular files and directories may occur within the copied tree; nested
symlinks, sockets, FIFOs, and devices fail naming the path. The source itself
may resolve through a symlink.

[Filters](#entry-filters) match paths relative to `source` at any depth.
Unselected entries are not inspected, so an excluded symlink does not fail.
Selecting nothing seeds an empty directory. Filters on a file source fail,
regardless of the destination. Selection reads the source in both modes before
considering the destination; refresh compares against the filtered tree.

### `copy-dir`

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
| `include`    | glob or list | no  | Entries under `source-dir` to copy, at any depth; absent, every entry. See [entry filters](#entry-filters). |
| `exclude`    | glob or list | no  | Entries under `source-dir` not to copy, at any depth.                 |

Each direct child is seeded separately: an occupied child destination is kept,
and its siblings are still seeded. A directory child is one whole seed; nothing merges
into an existing child directory.

Paths, sorted order, `dot-prefix`, empty/non-directory sources, and destination
containers follow [`symlink-dir`](#symlink-dir). Permissions and allowed node
types follow [`copy`](#copy).

[Filters](#entry-filters) match paths relative to `source-dir` at any depth,
before `dot-prefix`. A child is seeded if it or something beneath it is
selected, and contains only selected entries. A wholly unselected child's
destination is not inspected. Other filter rules are `copy`'s.

### `fetch-file`

Declares one file downloaded to a destination where nothing is.

```toml
[[actions]]
type = "fetch-file"
id = "pathogen"
source = "https://raw.githubusercontent.com/tpope/vim-pathogen/master/autoload/pathogen.vim"
dest = "~/.vim/autoload/pathogen.vim"
```

| Field    | Type   | Required | Description                                                          |
|----------|--------|:--------:|----------------------------------------------------------------------|
| `source` | string |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL. Not a repository path. |
| `dest`   | string |   yes    | Where the file goes, exactly. Never empty; `~` is the home.          |
| `sha256` | string |    no    | 64 hexadecimal digits: the digest the fetched bytes must have.       |
| `executable` | boolean | no  | Whether the file lands executable. Defaults to `false`.              |
| `decompress` | boolean | no  | Whether the download is a gzip or bzip2 stream holding the file. Defaults to `false`. |

The shared [transfer](#the-transfer-both-fetching-actions-share),
[seed](safety.md#seeds-do-not-replace-and-so-do-not-refuse),
[staging](safety.md#staging-and-publication), and
[permission](safety.md#installed-permissions) rules apply. An occupied
destination costs no transfer unless [refresh](safety.md#refreshing-seeds) is
requested. On Unix, `executable` installs the file `0755` rather than `0644`.

Without `decompress`, downloaded bytes are installed unchanged, including any
archive. With it, gzip or bzip2 compression is detected from the leading bytes,
including concatenated streams, and the decompressed file is installed, as a
release publishing `tool-linux.gz` intends:

```toml
[[actions]]
type = "fetch-file"
source = "https://example.com/tool-linux.gz"
dest = "~/.local/bin/tool"
decompress = true
executable = true
```

Uncompressed data, a zip, or content that decompresses to a tar fails; use
`fetch-archive` for archives. `sha256` checks the downloaded bytes before
decompression.

### `fetch-archive`

Declares one archive downloaded and unpacked at a destination where nothing is.

```toml
[[actions]]
type = "fetch-archive"
id = "fzf"
source = "https://example.com/fzf-0.65.2.tar.gz"
dest = "~/.local/fzf"
archive-root = "*"
```

| Field          | Type   | Required | Description                                                            |
|----------------|--------|:--------:|------------------------------------------------------------------------|
| `source`       | string |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL. Not a repository path. |
| `dest`         | string |   yes    | Where the unpacked directory goes, exactly. Never empty; `~` is the home. |
| `sha256`       | string |    no    | 64 hexadecimal digits: the digest the fetched archive must have.       |
| `archive-root` | string |    no    | A prefix every entry is written without, spelled as an entry path is, or `*` for the archive's single top-level directory. |
| `include`      | glob or list | no | Entries to unpack, by their path once the root is stripped; absent, every entry. See [entry filters](#entry-filters). |
| `exclude`      | glob or list | no | Entries not to unpack.                                                 |
| `executable`   | glob or list | no | Files to make executable, by their path once the root is stripped.     |

The shared [transfer](#the-transfer-both-fetching-actions-share) supplies the
archive. The [seed policy](safety.md#seeds-do-not-replace-and-so-do-not-refuse)
installs one whole directory: an occupied `dest`, including one an earlier
`create-dir` made, causes no request. [Refresh](safety.md#refreshing-seeds)
replaces the whole tree. Extraction follows [archive safety](safety.md#archive-extraction),
[permissions](safety.md#installed-permissions), and staged publication.

Formats are detected from leading bytes:

| Format | Support |
| --- | --- |
| Tar | Plain, gzip, or bzip2; concatenated compressed streams; V7, ustar, GNU, and pax |
| Zip | Stored, deflated, or bzip2 entries; no encryption or other compression methods |

Plain tar is recognized by its first header's checksum. Zip must begin with an
entry or an empty central-directory ending; self-extracting prefixes are not
supported. Unsupported bodies fail with the detected format, such as xz or zstd.
Zip names use UTF-8 when flagged or valid, otherwise code page 437; backslashes
in names are refused. Unix modes determine zip symlink and permission metadata.

`archive-root` strips a prefix. `"*"` requires a single top-level directory and
strips it; multiple roots fail naming them. An explicit prefix installs only
entries beneath it. A prefix with nothing beneath it fails. Prefixes follow
entry-path syntax: absolute paths and `..` are load errors.

Filters and `executable` match paths after root stripping:

```toml
[[actions]]
type = "fetch-archive"
source = "https://example.com/tool.tar.gz"
dest = "~/.local/tool"
archive-root = "*"
include = ["bin", "lib"]
exclude = "**/*.md"
executable = ["bin", "lib/*.so"]
```

Directories needed to hold selected entries retain their archive modes.
Selecting nothing, or selecting a hard link whose target is excluded, fails.
Filters do not relax archive safety. Unmatched patterns are reported at `-v`
only when extraction runs, never during a dry run.

`executable` adds owner, group, and other execute bits to matched files and
files beneath matched directories, regardless of archive modes. It does not
change directories, symlinks, or excluded files; hard links share their target's
mode. A pattern marking no file is reported at `-v`. It has no effect on Windows.

### The transfer both fetching actions share

Both fetching actions use these rules:

- HTTP, HTTPS, and [`file://`](#file-urls) URLs; up to five redirects.
- Status `200 OK` is required. Partial, empty-status, and conditional responses
  such as 206, 204, and 304 fail.
- An optional `sha256` is checked against the downloaded bytes. A mismatch
  reports both digests and installs nothing. Archives are verified before
  extraction, and compressed files before decompression.
- No `Accept-Encoding` request header. Proxy environment variables are honored.
- TLS certificates use the operating system's trust store, including corporate
  CAs trusted by that machine.
- Thirty seconds to connect, thirty seconds to receive response headers, and
  ten minutes total for the body.

An incomplete or failed transfer is not published. See
[staging and publication](safety.md#staging-and-publication).

#### File URLs

A `file://` URL reads a whole local file and verifies `sha256` if supplied.
HTTP status, redirects, proxies, and timeouts do not apply.

Use an absolute path: `file:///home/you/tool.tar.gz`,
`file://localhost/home/you/tool.tar.gz`, or `file:///C:/tools/a.zip` on Windows.
Paths are percent-decoded; encode spaces as `%20`, `?` as `%3F`, and `#` as
`%23`. Queries, fragments, and hosts other than `localhost` are load errors.
Missing or unreadable files fail when the action runs, naming the path.

### `git-clone`

Declares one Git repository cloned at a destination, and kept up to date there.

```toml
[[actions]]
type = "git-clone"
id = "oh-my-zsh"
source = "https://github.com/ohmyzsh/ohmyzsh.git"
dest = "~/.oh-my-zsh"
```

| Field    | Type   | Required | Description                                                   |
|----------|--------|:--------:|---------------------------------------------------------------|
| `source` | string |   yes    | A repository for git to clone. Not a repository path.         |
| `dest`   | string |   yes    | The clone directory, exactly. Never empty; `~` is the home.   |
| `ref`    | string |    no    | A branch, tag, or commit to follow. Never empty.              |

`source` is passed unchanged to Git: HTTPS, SSH/scp-style, `git://`, or a local
path. Only empty sources are rejected by the manifest; repository path rules
do not apply. `dest` follows the shared [destination syntax](#sources-and-destinations).

Batfiles uses Git from `PATH` with the [documented environment](environment.md#variables-passed-on-to-git).
It does not initialize or update submodules; use
`git submodule update --init --recursive` if needed.

#### `ref`: following one branch, tag, or commit

Without `ref`, a clone follows its current branch and updates from its upstream.
With `ref`, every run fetches all remotes, then resolves the requested target:

1. For a valid branch name, search remote-tracking branches, preferring `origin`.
   Check out a local branch of that name, create tracking when needed, and
   fast-forward on later runs.
2. Otherwise resolve a tag, commit, full `refs/…` name, or expression such as
   `main~1`, and check it out detached. `HEAD` is resolved this way too.
3. An unresolved ref fails naming the request. A detached checkout already at
   the requested object is unchanged.

[Git updates](safety.md#git-updates) defines worktree protection and failure
handling. [Clone validation](safety.md#clone-validation) defines usable existing
checkouts; other destinations are [conflicts](safety.md#conflicts-and-backups).
Existing clones retain their configured remotes.

### `git-clone-list`

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

The source uses [repository path syntax](#sources-and-destinations) and the
[clone list format](#the-clone-list-format). Each entry's repository is passed
to Git unchanged, regardless of where the list lives. Diagnostics identify the
list by its declared source, including any remote qualifier.

Selected lists must exist during [preparation](cmdline.md#clone-list-preparation).
The destination container is created even for an empty list. Entries run in
list order with `git-clone` behavior and [entry-specific failures](cmdline.md#clone-list-entry-failures).
An entry with an `id` can be [addressed](cmdline.md#addresses) individually.

### `include-remote`

Takes the actions another repository declares into this one, at this position in
the list.

```toml
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
install-groups = ["shell"]
exclude-actions = ["p10k"]
```

| Field             | Type                  | Required | Description                                             |
|-------------------|-----------------------|:--------:|---------------------------------------------------------|
| `remote`          | ID                    |   yes    | A [`git` remote](#git) this same manifest declares.     |
| `install-actions` | ID or list of IDs     |    no    | Take only the actions named.                            |
| `install-groups`  | ID or list of IDs     |    no    | Take only the actions naming these groups.              |
| `exclude-actions` | ID or list of IDs     |    no    | Leave out the actions named.                            |
| `exclude-groups`  | ID or list of IDs     |    no    | Leave out the actions naming these groups.              |
| `vars`            | map of name to string |    no    | [Values](#variables-for-one-inclusion) for what it takes. |

`remote` must name a Git remote in the same manifest; undeclared, file, or
archive remotes are load errors. Multiple inclusions of one remote share its
materialization.

The inclusion reads `remotes/<remote>/batfiles.toml` and splices its actions
into this position, in their declaration order. Included repository paths, including clone-list
sources, resolve from that materialization. A present materialization with no
manifest fails. Expansion finishes before action execution; a malformed included
manifest therefore prevents installation. An absent materialization follows
[plan completeness](cmdline.md#plan-completeness).

Inclusion is one level deep, subject to the [restrictions below](#what-an-included-action-may-not-write).
A closed inclusion or remote condition contributes nothing, leaving the plan
complete. [Selection](cmdline.md#selecting-what-a-run-does) defines when an
inclusion is opened and how its group or exclusion reaches its contents.

An inclusion's `id` qualifies contributed [addresses](cmdline.md#addresses),
such as `corp.zshrc`; it need not match the remote ID. Without an ID, its
contents still run but cannot be addressed. Reports identify it by position
and remote, and attribute contributed records to it:

```text
include-remote action 2 of remote `corporate`
symlink zshrc (group shell, from include-remote action 2 of remote `corporate`)
```

Named inclusions use ``include-remote `corp` `` in diagnostics. The same name
identifies filter warnings and [variable blocks](cmdline.md#vars-list).

#### Selecting part of a remote

Filter values are unqualified IDs from the included manifest, as one string
or a list. With no filters, take all actions. An omitted allow-list differs
from an empty one: `install-actions = []` takes nothing.

At most one of `install-actions`, `install-groups`, and `exclude-groups` may be
present. `exclude-actions` may stand alone or accompany either group filter,
but not `install-actions`. Other combinations are load errors.

Allow-lists omit records lacking the relevant ID or group; deny-lists retain
them. Action and group filters use separate namespaces. A name matching nothing
warns, naming the inclusion, filter, and remote, but does not fail the run.

Filtered-out actions retain their addresses for selection diagnostics.
Filters are honored by every command; other exclusions can still apply to
selected records. See [selection](cmdline.md#selection-by-command).

#### Variables for one inclusion

```toml
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
```

`vars` maps [variable names](#names-and-ids) to strings, validated at load time.
Dynamic declarations are not accepted here. Omitted and empty maps both
provide no overrides. These values apply to contributed records, including
clone-list entries; see [scope and precedence](environment.md#variable-precedence).

#### Variables an included remote declares

An included manifest may declare its own static or dynamic `[vars]`, using the
[same schema](#variables) as the leaf. They are the lowest layer of
that inclusion's [scope](environment.md#variable-precedence), so the leaf and
this machine can override them without knowing they exist. They never decide the
inclusion's own condition or the remote's, which use the leaf scope.

Remote dynamic declarations require [`allow-dynamic-vars`](#git). They share
one capture and cache entry per remote variable across inclusions; see
[cache evaluation](state.md#when-declarations-are-evaluated).
Use `-vv` to inspect [inclusion variable blocks](cmdline.md#vars-list).

#### What an included action may not write

Included repository paths must refer to the declaring repository's own tree.
Remote references (`@core/files/zshrc` or the structured equivalent) are
refused even if that manifest declares the remote.

Nested `include-remote` records instead warn and are dropped; other actions
are contributed normally. The nested remote need not exist, since it will
never be resolved:

```text
warning: not included: include-remote corp.shared; an included repository does not reach further repositories
```

#### An included manifest's own `[remotes]`

The map is read for schema shape, then ignored. Unknown types and fields still
fail. Value validation is skipped: invalid URLs, digests, empty refs, and
case-colliding remote keys do not fail a run that includes the manifest.
Nothing materializes these remotes or allows an included action to source them.

A nonempty map warns once per inclusion, naming all ignored records; an empty
one is silent.

## The clone list format

A `git-clone-list` source is a text file with one repository per line:

```text
# vim plugins
https://github.com/tpope/vim-fugitive.git
https://github.com/romkatv/powerlevel10k.git dest-name=p10k id=p10k
https://github.com/company/internal-zsh-tools.git when="work" # work only
```

```text
<repository> [<key>=<value> ...] [# <comment>]
```

The repository is the first whitespace-delimited field, passed unchanged to
Git. Blank lines are ignored. The first `#` outside quoted values begins a
comment; encode a repository URL's literal `#` as `%23`.

| Key | Type | Description |
| --- | --- | --- |
| `id` | ID | Entry address suffix and diagnostic name |
| `ref` | string | Branch, tag, or commit; follows [ref rules](#ref-following-one-branch-tag-or-commit) |
| `dest-name` | string | Explicit clone directory name |
| `when` / `unless` | condition | Entry gate; at most one |

Unknown keys, repeated keys, empty `key=` values, and fields lacking `=` fail.
Values may be quoted with `'` or `"` to contain spaces or `#`. Inside quotes,
only `\\`, `\"`, and `\'` are escapes; other escapes and unclosed quotes fail.

Conditions use the enclosing action's scope and the shared
[condition rules](#conditions). See [preparation](cmdline.md#clone-list-preparation)
for evaluation timing. An explicit `id` permits [individual selection](cmdline.md#addresses)
and appears in diagnostics as `id=p10k, plugins.txt line 3`; otherwise entries
are named by file and line. IDs are never derived from directory names:
`ack.vim` is a valid directory name but not an ID.

### What a clone is called

Without `dest-name`, use everything after the repository's last `/` or `:`,
removing a trailing `.git`. This derivation is textual, independent of URL kind.

Derived or explicit names must be one directory component: nonempty, not `.`,
`..`, or `.git`, and containing no `/`, `\`, or `:`. An invalid derived name
asks for an explicit `dest-name`.

Destination names must be unique ignoring case on every platform; entry IDs
must also be unique. Duplicate errors name both lines. One repository under two
distinct destination names is valid, and destination spelling is preserved.
The first error stops list loading, identifying the file, line, and fault.

## Variables

`[vars]` maps [variable names](#names-and-ids) to strings or dynamic declarations:

```toml
[vars]
work = "false"
profile = "personal"
rank = "3"
email = { command = ["git", "config", "user.email"], cache = "1d" }
```

Variables feed `when` and `unless` only; no field interpolates them. Static
values must be strings, including empty strings: `work = true` and `rank = 3`
are load errors. A table must match the dynamic-variable schema.

See [variable precedence](environment.md#variable-precedence) for machine
values, environment and CLI overrides, and inclusion scopes. Dynamic commands
may run even when no condition uses them; see
[evaluation](state.md#when-declarations-are-evaluated).

### Dynamic variables

A table under `[vars]` declares a variable whose value a command produces. Both
TOML spellings are the same record:

```toml
[vars]
email = { command = ["git", "config", "user.email"], cache = "24h" }

[vars.has_op]
command = "command -v op >/dev/null"
capture = "status"
cache = "1h"
command-timeout = "5s"
```

| Field             | Type                               | Required | Default    | Description                                                                     |
|-------------------|------------------------------------|:--------:|------------|---------------------------------------------------------------------------------|
| `command`         | string or non-empty list of strings |   yes    |            | A shell command line, or a program and its arguments run without a shell.       |
| `capture`         | `"stdout"` or `"status"`           |    no    | `"stdout"` | The trimmed standard output, or `"true"`/`"false"` from the exit status.        |
| `cache`           | duration                           |    no    | `"1d"`     | How long a captured value is reused before the command runs again.              |
| `command-timeout` | duration                           |    no    | `"5s"`     | How long the command may run before it is killed. Greater than zero.            |

Unknown fields, empty command lists, invalid capture modes or durations, and
zero timeouts are load errors.

[Execution](environment.md#how-dynamic-commands-are-run) defines shell selection,
working directory, streams, limits, and capture semantics.
[Cache rules](state.md#dynamic-varstoml-dynamic-variable-cache) define reuse,
refresh, and failure fallback. Remote declarations require
[`allow-dynamic-vars`](#git).

A declaration with no captured or cached value still declares the variable;
it reads as the empty string and overrides lower layers. Consequently `when`
on it closes but `unless` opens. Use `when` when installation requires a
successful answer.

#### Duration values

`cache` and `command-timeout` are strings holding a number and a unit,
optionally repeated: `30s`, `5m`, `1h`, `1d`, `1w`, `1h 30m`. Units may be
abbreviated or spelled out (`2 hrs`, `90 minutes`), separated by whitespace or
commas, and sub-second units are accepted (`500ms`). A clock-style `HH:MM:SS`
form is accepted too (`01:30:00`). A fraction is allowed on the last unit
written, and only for hours or smaller: `1.5h` and `1m 30.5s` are durations,
`1.5d` and `1.5h 30m` are not. ISO 8601 durations such as `PT1H` are not.

A day is exactly 24 hours and a week exactly 7 days. Months and years have no
fixed length, so they are not durations. A negative duration, written `-1h` or
`1h ago`, is invalid. Zero is a valid `cache`, where it means a value is never
fresh, and an invalid `command-timeout`, which would kill every command before
it could answer.

## Conditions

Actions, clone-list entries, remotes, and default-disabled candidates may carry
one condition:

| Field | The record applies when |
| --- | --- |
| `when` | True |
| `unless` | False |

Writing both is a load error.

```toml
[[actions]]
type = "symlink"
id = "gitconfig-work"
source = "git/gitconfig.work"
dest = "~/.gitconfig"
when = "work && facts.os == 'macos'"
```

### The expression

Values use [Simple Expressions](https://github.com/abatkin/expressions-rs).
Every expression is parsed during document loading, even if never evaluated;
syntax errors identify the file, line, and position in the expression.

User variables are string-valued bare identifiers. Reserved namespaces
[`facts`](environment.md#host-facts-in-conditions),
[`env`](environment.md#host-environment-in-conditions), and `vars` also return
strings. Use member syntax for identifier-compatible keys (`facts.os`) or
index syntax for any key (`env["XDG_CURRENT_DESKTOP"]`).

### Identifiers

| Lookup | Declared/set | Missing |
| --- | --- | --- |
| Bare user variable | Its string value, including empty | Evaluation error |
| `vars.<name>` | Same user value | Empty string |
| `facts.<key>`, `env.<key>` | Host value | Empty string |

Use a bare identifier to catch undeclared names, or `vars` for optional ones:

```toml
when = "work"                     # error if undeclared
when = "vars.work"                # false if undeclared
when = "vars.work || vars.school"
```

All variable names allow member syntax; index syntax is also accepted.
[Precedence](environment.md#variable-precedence) chooses each user value.

### Truthiness

The condition result and every `&&`, `||`, and `!` operand use this table:

| Value | Boolean interpretation |
| --- | --- |
| Boolean | Itself |
| Number | False at zero, otherwise true |
| `"true"`, `"1"`, `"yes"`, `"on"` | True |
| `"false"`, `"0"`, `"no"`, `"off"`, `""` | False |
| Anything else | Evaluation error |

Comparison and `+` retain the expression language's rules. For example,
`profile = "personal"` needs a comparison, not `when = "profile"`.

### When a condition cannot be evaluated

Undeclared bare identifiers, invalid truthiness, arithmetic overflow, and other
evaluation failures close the gate for **both `when` and `unless`**. The record
is skipped, with [a warning](cmdline.md#exclusion-reporting) that permits
[continuation](cmdline.md#execution-failures). Diagnostics include the condition
and remedy but never the offending value.

A failed dynamic declaration with no cached value is still declared and reads
as the empty string; it does not cause an undeclared-name error.
[Selection](cmdline.md#selection-by-command) determines when action conditions
are evaluated or waived. Bootstrap conditions are evaluated only when their
candidates are offered.

## Default-disabled bootstrap entries

`[default-disabled]` proposes initial disables for a fresh machine:

```toml
[[default-disabled.actions]]
id = "p10k"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'macos'"
```

### Action entry

| Field | Type | Required | Description |
| --- | --- | :---: | --- |
| `id` | address | yes | Action or clone-list entry to disable |
| `when` / `unless` | condition | no | Offer the candidate only when its gate passes; at most one |

### Group entry

| Field | Type | Required | Description |
| --- | --- | :---: | --- |
| `group` | address | yes | Group to disable |
| `when` / `unless` | condition | no | Offer the candidate only when its gate passes; at most one |

Records are closed and [address syntax](cmdline.md#addresses) is validated, but
names are never resolved. Candidates may name included content or actions a
later repository revision introduces.

Only the leaf's candidates are considered, by
[`clone` or `sync --bootstrap`](cmdline.md#what-the-bootstrap-decides).
[Bootstrap adoption](state.md#bootstrap-adoption) defines eligibility and
persistence; [precedence](environment.md#bootstrap-adoption-precedence) defines
overrides. Conditions are always parsed at manifest load, and evaluated only
when candidates are offered during bootstrap. Other commands ignore the section.
