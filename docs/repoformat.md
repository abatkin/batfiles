# Batfiles Repository Format

This document is a schema-oriented summary of the repository formats necessary
for the batfiles dotfiles manager. It intentionally gives only enough behavior
to explain the data model; the other [specification documents](README.md) define
planning, precedence, state, and command execution policy.

## Repository Layout

A batfiles repository is an ordinary file tree with a `batfiles.toml` at its
root.

```text
dotfiles/
├── batfiles.toml
├── install.sh              # bootstrap entry point in a leaf repo
├── remotes/                # generated and owned by batfiles
├── bin/                    # conventional directory, not magic
├── files/                  # conventional directory, not magic
├── local-files/            # conventional directory, not magic
└── ...
```

Only `batfiles.toml` has intrinsic meaning. Names such as `bin/`, `files/`, and
`local-files/` become meaningful only when actions reference them.

The user-selected **leaf repository** owns the configuration being applied. A
Git remote may also have a `batfiles.toml`; it is optional and is read only when
a leaf `include-remote` action selects it. The `remotes/` directory is generated
materialization data and should normally be ignored by Git.

## Top-Level `batfiles.toml` Schema

All top-level sections are optional:

```toml
[remotes]                  # map<string, Remote>

[vars]                     # map<string, string | DynamicVariable>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]

[[actions]]                # ordered list<Action>
```

There is no format-version field in the current schema.

Known records are closed: unknown fields in the top-level document, a remote,
an action, a dynamic variable, a structured path reference, or a
default-disabled entry are invalid. Map keys under `[remotes]` and `[vars]` are
user-defined data and therefore are not treated as schema fields; their values
must still match one of the known value shapes.

## Shared Value Types

The schema uses these reusable value shapes.

### String-valued variables

Every variable value exposed by batfiles remains a string. This includes static
repository values, per-inclusion overrides, persisted and one-shot overrides,
dynamic-command results, facts, and host environment values. Batfiles does not
infer types from their contents.

A static repository value is therefore a TOML string:

```text
VariableValue = string
```

Booleans, integers, floats, dates, arrays, and arbitrary tables are not static
variable values. A table under `[vars]` is interpreted as a dynamic-variable
declaration rather than a value.

### Condition

```text
Condition = string
```

The string contains an expression, for example:

```toml
when = "work && facts.os == 'darwin'"
```

Expressions use the
[Simple Expressions](https://github.com/abatkin/expressions-rs) language.
Batfiles makes user variables available as bare string-valued identifiers and
provides reserved, string-valued `facts` and `env` resolvers. The resolvers
accept member syntax for identifier-compatible keys, such as `facts.os` and
`env.HOME`, and index syntax for any key, such as
`env["XDG_CURRENT_DESKTOP"]`. A missing fact or environment key resolves to the
empty string instead of producing an evaluation error.

Condition inputs follow the shared [string-valued variable
model](#string-valued-variables); any coercion while evaluating an expression
is defined by Simple Expressions itself.

For concision, schema tables below list only `when`. Every TOML record or
manifest entry that accepts `when` also accepts `unless` as its negated alias.
The two fields are mutually exclusive. This convention applies even though
`unless` is not repeated in each closed-record table.

### Repository path

Fields that read a path from a repository accept either a string or a closed
structured record:

```text
RepoPath = string | { remote: string, path: string }
```

For an action declared by the leaf repository, all three TOML forms below are
valid:

```toml
source = "files/zshrc"
source = "@core/files/zshrc"
source = { remote = "core", path = "files/zshrc" }
```

A plain string is relative to the action's own repository. A string beginning
with `@` is shorthand for the structured remote reference.

Remote references are available only to actions declared by the leaf
repository. An action included from a Git remote may use only an ordinary
repository-relative string, which resolves within that Git remote's
materialization; `@remote/path` and
`{ remote = "remote", path = "path" }` are invalid in an included action. An
included repository does not get its own remotes, and its actions do not
inherit access to the leaf repository's remotes. This keeps inclusion limited
to one level and prevents an included action from reinterpreting a remote name
from the leaf repository.

### Glob filter

`include` and `exclude` accept either one glob or a list:

```text
GlobFilter = string | list<string>
```

```toml
include = "*.toml"
exclude = ["private/*", "*.bak"]
```

### Names and IDs

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

- User variable names match `[A-Za-z_][A-Za-z0-9_]*` and cannot be `facts`,
  `env`, `true`, or `false`.
- IDs and group names match `[A-Za-z0-9][A-Za-z0-9_-]*`. This rule applies to
  action IDs, `include-remote` IDs, manifest-entry IDs, remote IDs, and group
  names. In particular, an ID cannot contain whitespace, `.`, or `,`; dots are
  reserved for composing qualified addresses and commas delimit environment
  lists.
- Action IDs must be unique within a repository. Manifest-entry IDs must be
  unique within their manifest.
- Group names and action IDs occupy distinct namespaces.
- A remote map key is also that remote's ID.

## Remotes

`[remotes]` is a map from a user-selected name to a tagged remote record. Every
remote has these fields:

| Field  | Type                           | Required | Default | Description                       |
|--------|--------------------------------|:--------:|---------|-----------------------------------|
| `type` | `"git" \| "file" \| "archive"` |   yes    | —       | Selects the record variant.       |
| `when` | `Condition`                    |    no    | enabled | Conditionally enables the remote. |

### Git remote

```toml
[remotes.core]
type = "git"
url = "git@github.com:me/dotfiles-core.git"
branch = "main"
when = "facts.os != 'windows'"
allow-dynamic-vars = true
```

| Field                | Type    | Required | Default | Description                                                               |
|----------------------|---------|:--------:|---------|---------------------------------------------------------------------------|
| `url`                | string  |   yes    | —       | Git repository URL.                                                       |
| `branch`             | string  |    no    | —       | Branch name; tags and commit pins are not part of the remote schema.      |
| `allow-dynamic-vars` | boolean |    no    | `false` | Whether this remote may execute included dynamic variable declarations.   |

### File remote

```toml
[remotes.pathogen]
type = "file"
url = "https://example.com/pathogen.vim"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
```

| Field    | Type   | Required | Description                                               |
|----------|--------|:--------:|-----------------------------------------------------------|
| `url`    | string |   yes    | `https://`, `http://`, or `file://` source URL.           |
| `sha256` | string |    no    | 64-digit hexadecimal SHA-256 digest of the fetched bytes. |

### Archive remote

```toml
[remotes.fzf]
type = "archive"
url = "https://example.com/fzf.tar.gz"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
archive-root = "*"
include = ["bin/*"]
exclude = ["*.md"]
```

| Field          | Type         | Required | Description                                                                 |
|----------------|--------------|:--------:|-----------------------------------------------------------------------------|
| `url`          | string       |   yes    | `https://`, `http://`, or `file://` archive URL.                            |
| `sha256`       | string       |    no    | 64-digit hexadecimal SHA-256 digest of the archive bytes.                   |
| `archive-root` | string       |    no    | Archive path prefix to strip, or `"*"` for automatic single-root detection. |
| `include`      | `GlobFilter` |    no    | Archive entries to include.                                                 |
| `exclude`      | `GlobFilter` |    no    | Archive entries to exclude.                                                 |

Declaring a remote only names and materializes a source. Actions decide whether
and where its content is installed.

## Variables

`[vars]` is a map from variable name to either a static string or a dynamic
variable record.

```toml
[vars]
work = "false"
profile = "personal"
rank = "3"

[vars.has_op]
command = ["sh", "-c", "command -v op >/dev/null"]
capture = "status"
cache = "1h"
command-timeout = "5s"
```

Dynamic declarations may also use inline-table syntax:

```toml
[vars]
email = { command = ["git", "config", "user.email"], cache = "24h" }
```

### Dynamic variable record

| Field             | Type                               | Required | Default    | Description                                                                     |
|-------------------|------------------------------------|:--------:|------------|---------------------------------------------------------------------------------|
| `command`         | string or non-empty `list<string>` |   yes    | —          | Shell command string or direct argument vector.                                 |
| `capture`         | `"stdout" \| "status"`             |    no    | `"stdout"` | Produce a string from trimmed stdout or `"true"`/`"false"` from command status. |
| `cache`           | duration string                    |    no    | `"1d"`     | Cache lifetime, using a friendly duration such as `"1h"`.                       |
| `command-timeout` | duration string                    |    no    | `"5s"`     | Maximum command runtime.                                                        |

Arbitrary table-shaped variable values are not supported; every table value in
`[vars]` must match this closed dynamic-variable record.

#### Duration values

`cache` and `command-timeout` accept a friendly duration: a number and a unit,
optionally repeated, such as `30s`, `5m`, `1h`, `1d`, `1w`, or `1h 30m`. Units
may be abbreviated or spelled out (`2 hrs`, `90 minutes`), separated by
whitespace or commas, and sub-second units are accepted (`500ms`). A clock-style
`HH:MM:SS` form is also accepted (`01:30:00`). A fractional quantity is allowed
on the last unit written and only for hours or smaller: `1.5h` and `1m 30.5s`
are durations, `1.5d` and `1.5h 30m` are not. ISO 8601 durations such as `PT1H`
are not accepted.

A day is exactly 24 hours and a week is exactly 7 days. Months and years have no
fixed length, so they are not durations; freshness compares two instants rather
than two calendar dates. A negative duration — written `-1h` or `1h ago` — is
invalid. Zero is valid: as a `cache` it means the value is never fresh.

## Default-Disabled Bootstrap Entries

The leaf repository may provide initial disabled action and group addresses.
These are arrays of closed records nested below `[default-disabled]`.

```toml
[[default-disabled.actions]]
id = "p10k"

[[default-disabled.actions]]
id = "core.work-tools"
when = "work"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'darwin'"
```

### Action entry

| Field    | Type        | Required | Description                                                        |
|----------|-------------|:--------:|--------------------------------------------------------------------|
| `id`     | string      |   yes    | Addressable remote, action, included action, or manifest-entry ID. |
| `when`   | `Condition` |    no    | Conditional bootstrap adoption.                                    |

### Group entry

| Field    | Type        | Required | Description                                   |
|----------|-------------|:--------:|-----------------------------------------------|
| `group`  | string      |   yes    | Leaf or qualified included group address.     |
| `when`   | `Condition` |    no    | Conditional bootstrap adoption.               |

`[default-disabled]` in an included remote is structurally valid but ignored;
bootstrap policy belongs to the leaf repository.

## Actions

`[[actions]]` is an ordered heterogeneous array. Each action is a tagged record
selected by its required `type` field.

All action variants share these fields:

| Field   | Type               | Required | Description                                                                |
|---------|--------------------|:--------:|----------------------------------------------------------------------------|
| `type`  | action-type string |   yes    | Selects the action variant.                                                |
| `id`    | `ID`               |    no    | Makes the action addressable when its surrounding context also permits it. |
| `when`  | `Condition`        |    no    | Conditionally enables the action.                                          |
| `group` | `ID`               |    no    | Places the action in one group.                                            |

### `symlink`

Creates one symlink or a shallow set of symlinks. Exactly one of the two modes
is valid.

Single-item mode:

```toml
[[actions]]
type = "symlink"
source = "shell/zshrc"
dest = "~/.zshrc"
```

Directory mode:

```toml
[[actions]]
type = "symlink"
source-dir = "files"
dest-dir = "~"
include = ["zshrc", "config"]
exclude = "private"
dot-prefix = true
```

| Field        | Type         | Mode                | Description                                                         |
|--------------|--------------|---------------------|---------------------------------------------------------------------|
| `source`     | `RepoPath`   | single, required    | Source file, symlink, or directory.                                 |
| `dest`       | string       | single, required    | Exact destination path.                                             |
| `source-dir` | `RepoPath`   | directory, required | Directory whose direct children are selected.                       |
| `dest-dir`   | string       | directory, required | Destination directory for selected children.                        |
| `include`    | `GlobFilter` | directory, optional | Direct child names to include.                                      |
| `exclude`    | `GlobFilter` | directory, optional | Direct child names to exclude.                                      |
| `dot-prefix` | boolean      | directory, optional | Prefix the first destination segment with `.`; defaults to `false`. |

### `copy`

Missing-only seed action that copies a file-like item or recursively seeds the
contents of a directory. A file-like source is skipped when its exact `dest`
exists. For a directory source, an existing `dest` is a merge root rather than
a reason to skip the action; the missing-only check applies separately to each
selected mapped entry. Thus the example below can seed missing dotfiles even
though `~` already exists. See
[Seed actions and deletion](safety.md#seed-actions-and-deletion).

```toml
[[actions]]
type = "copy"
source = "local-files"
dest = "~"
include = ["*"]
exclude = ["private/*"]
dot-prefix = true
```

| Field        | Type         | Required | Description                                                                    |
|--------------|--------------|:--------:|--------------------------------------------------------------------------------|
| `source`     | `RepoPath`   |   yes    | File, symlink, or directory source.                                            |
| `dest`       | string       |   yes    | Exact destination for a file-like source, or destination root for a directory. |
| `include`    | `GlobFilter` |    no    | Recursive selection when the source is a directory.                            |
| `exclude`    | `GlobFilter` |    no    | Recursive exclusion when the source is a directory.                            |
| `dot-prefix` | boolean      |    no    | Dot-prefix top-level mapped paths for a directory source; defaults to `false`. |

### `create-dir`

```toml
[[actions]]
type = "create-dir"
dest = "~/.config"
```

| Field  | Type   | Required | Description                     |
|--------|--------|:--------:|---------------------------------|
| `dest` | string |   yes    | Exact directory path to create. |

### `git-clone-list`

Reads a line-oriented manifest and maps each repository URL below a destination
directory.

```toml
[[actions]]
id = "zsh-plugins"
type = "git-clone-list"
source = "manifests/zsh-plugins.txt"
dest = "~/.local/share/zsh-plugins"
```

| Field    | Type       | Required | Description                                      |
|----------|------------|:--------:|--------------------------------------------------|
| `source` | `RepoPath` |   yes    | Manifest file.                                   |
| `dest`   | string     |   yes    | Parent directory for derived clone destinations. |

An action `id` is required only if its individual manifest entries need
qualified addresses.

#### Deferred manifest expansion

The manifest is not read during structural planning. A selected `git-clone-list`
stays one opaque node in the plan, carrying its resolved configuration, where its
manifest source comes from, the variable, fact, and environment context needed to
evaluate entry conditions, and any requested, disabled, or skipped entry
addresses that execution must apply. The plan does not claim to know or validate
the entries.

When the action executes, batfiles materializes or refreshes the manifest's
source remote if required, reads and validates the manifest, expands its entries
into a nested execution-time plan, evaluates each entry's condition, and performs
the selected clones. This holds even when the manifest is already readable at
planning time, because a remote refresh or an earlier ordered action may change
it before this action runs.

Entry existence is therefore checked during execution, not during planning. A
targeted `apply-action --id <action>.<entry>` produces a deferred manifest node
carrying the requested entry ID; an entry address that no manifest entry matches
fails when the action executes.

### `git-clone`

```toml
[[actions]]
type = "git-clone"
source = "https://github.com/ohmyzsh/ohmyzsh.git"
dest = "~/.oh-my-zsh"
ref = "refs/heads/master"
```

| Field    | Type   | Required | Description                                   |
|----------|--------|:--------:|-----------------------------------------------|
| `source` | string |   yes    | Literal Git repository URL, not a `RepoPath`. |
| `dest`   | string |   yes    | Exact clone directory.                        |
| `ref`    | string |    no    | Branch, tag, or commit selector.              |

### `fetch-url`

Missing-only seed action that fetches a file to one destination or extracts an
archive into one destination directory. A non-extracting fetch is skipped when
its exact `dest` exists. For extraction, an existing `dest` is a merge root
rather than a reason to skip the action; the missing-only check applies
separately to each selected mapped archive entry. A prior `create-dir` for that
`dest` therefore does not suppress extraction. See
[Seed actions and deletion](safety.md#seed-actions-and-deletion).

```toml
[[actions]]
id = "pathogen"
type = "fetch-url"
source = "https://example.com/pathogen.vim"
sha256 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
dest = "~/.vim/autoload/pathogen.vim"
```

```toml
[[actions]]
type = "fetch-url"
source = "https://example.com/tool.tar.gz"
dest = "~/.local/tool"
extract = true
archive-root = "*"
include = ["bin/*"]
exclude = ["*.md"]
```

| Field          | Type         | Required | Default     | Description                                                            |
|----------------|--------------|:--------:|-------------|------------------------------------------------------------------------|
| `source`       | string       |   yes    | —           | `https://`, `http://`, or `file://` URL.                               |
| `dest`         | string       |   yes    | —           | File destination, or directory destination when extracting.            |
| `extract`      | boolean      |    no    | `false`     | Whether the response is an archive to extract.                         |
| `sha256`       | string       |    no    | —           | 64-digit hexadecimal digest of the response/archive bytes.             |
| `archive-root` | string       |    no    | archive top | Archive prefix to strip, or `"*"` for automatic single-root detection. |
| `include`      | `GlobFilter` |    no    | all entries | Entries to include while extracting.                                   |
| `exclude`      | `GlobFilter` |    no    | none        | Entries to exclude while extracting.                                   |

The archive-selection fields are meaningful only when `extract = true`.

### `include-remote`

Selects actions from a declared Git remote and inserts them at this position in
the ordered action list.

```toml
[[actions]]
type = "include-remote"
id = "core"
remote = "core"
install-groups = ["editor"]
exclude-actions = ["p10k"]
vars = { profile = "personal" }
```

| Field             | Type                        | Required | Description                                   |
|-------------------|-----------------------------|:--------:|-----------------------------------------------|
| `remote`          | `ID`                        |   yes    | Name of a declared Git remote.                |
| `install-actions` | `ID` or `list<ID>`          |    no    | Allow-list of unqualified remote action IDs.  |
| `install-groups`  | `ID` or `list<ID>`          |    no    | Allow-list of unqualified remote group names. |
| `exclude-actions` | `ID` or `list<ID>`          |    no    | Deny-list of unqualified remote action IDs.   |
| `exclude-groups`  | `ID` or `list<ID>`          |    no    | Deny-list of unqualified remote group names.  |
| `vars`            | `map<string, string>`       |    no    | Per-inclusion variable overrides.             |

The common `id` is optional, but it is what makes included actions, groups, and
manifest entries externally addressable. It becomes the `<remote>` prefix in a
qualified address such as `core.zshrc`; it need not match the declared remote
name in the `remote` field. Each selection field accepts either a single string
or a list of strings; a string is equivalent to a one-item list.

The inclusion `id` is not used to namespace dynamic-variable cache entries.
Allowed dynamic declarations are cached by the declared remote's map key, so
multiple inclusions of the same remote share their declaration captures even
when the inclusions have different IDs or `vars` overrides. The state
specification defines the [remote cache-key
format](state.md#dynamic-varstoml-dynamic-variable-cache).

If none of the `install-*` or `exclude-*` fields is specified, all actions in
the remote are selected. Selection does not force an action to run: the
included actions remain subject to their usual conditions, repository-wide
disabled and run-only skip lists, and all other normal planning and application
rules.

At most one of `install-actions`, `install-groups`, and `exclude-groups` may be
specified. `exclude-actions` may be used by itself or together with either
`install-groups` or `exclude-groups`, allowing specific actions to be removed
from the group-selected set. It cannot be combined with `install-actions`. No
other combinations are valid.

## Git Clone Manifest Format

The `git-clone-list` source is a line-oriented text file rather than TOML.
Blank lines and full-line comments are ignored.

```text
https://github.com/zsh-users/zsh-autosuggestions.git
https://github.com/romkatv/powerlevel10k.git id=p10k when="use_p10k" ref=master dest-name=p10k # fancy prompt; example=x
# plain comment
```

An entry has this conceptual shape:

```text
<git-url> [<key>=<value> ...] [# <comment text>]
```

The URL is the first whitespace-delimited field. It may be followed by
whitespace-separated `key=value` metadata. The first `#` outside a quoted
metadata value begins an opaque comment; nothing after it is parsed as
metadata. A literal hash in a URL must therefore be percent-encoded as `%23`.
This separation allows comments to contain arbitrary text, including `=`,
quotes, and text that resembles a supported metadata key.

Supported metadata keys are:

| Key         | Type             | Description                                                       |
|-------------|------------------|-------------------------------------------------------------------|
| `id`        | `ID`             | Optional entry ID.                                                |
| `when`      | condition string | Optional enablement expression. Quote it when it contains spaces. |
| `unless`    | condition string | Negated alias for `when`; mutually exclusive with it.             |
| `ref`       | string           | Optional branch, tag, or commit selector.                         |
| `dest-name` | string           | Optional one-component override for the destination directory name. |

Without `dest-name`, the destination name is the final component after the last
slash in the repository URL with a trailing `.git` removed. Whether derived or
specified by `dest-name`, it must be a single ordinary directory component: it
cannot be `.`, `..`, an absolute path, or a path containing directory
separators. The entry is cloned to `<action dest>/<destination name>`;
`dest-name` is only a name and cannot select another directory.

Values may be bare, single-quoted, or double-quoted. Bare values end at
whitespace or `#`. Quoted values end at the matching quote and may contain `#`.
Within quoted values the only escapes are `\\`, `\"`, and `\'`. Every
non-comment field after the URL must be metadata. Unknown keys, duplicate keys,
unterminated quotes, unsupported escapes, an empty `key=` value, a field without
`=`, or specifying both `when` and `unless` make that entry's metadata invalid.
An `id` value that does not match the [shared ID syntax](#names-and-ids) is also
invalid.

## Serializer-Oriented Summary

A serializer or deserializer needs to account for four unions:

1. `Remote` is tagged by `type` as `git`, `file`, or `archive`.
2. `Action` is tagged by `type` as one of seven action variants.
3. A `[vars]` value is either a string or a dynamic-variable record.
4. A repository-backed source is either a string or a structured remote/path
   record.

TOML inline tables and full tables represent the same records and should
deserialize equivalently.
