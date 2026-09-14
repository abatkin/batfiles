# Batfiles Repository Format

This document is a schema-oriented summary of the repository formats necessary
for the batfiles dotfiles manager. It intentionally gives only enough behavior
to explain the data model; the other [specification documents](README.md) define
planning, precedence, state, and command execution policy.

## Repository Layout

The part of this section that runs — a repository is a file tree with a
`batfiles.toml` at its root, only that file has intrinsic meaning, and batfiles
generates `remotes/<remote-id>/` to hold what the manifest declares — is
specified in [`docs/repoformat.md`](../repoformat.md), along with how the
manifest is read and what materializing a remote does.

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

What is not built is the reading of what a materialization holds. A Git remote
may have a `batfiles.toml` of its own; it is optional and is read only when a
leaf `include-remote` action selects it, so an included remote's manifest is
`remotes/<remote-id>/batfiles.toml`. Because a materialization is keyed by the
remote's map key rather than by the ID of any inclusion that selects it, two
inclusions of one remote share the single materialization at that path.

## Top-Level `batfiles.toml` Schema

The part of this section that runs — every section optional, no format-version
field, and known records closed — is specified in
[`docs/repoformat.md`](../repoformat.md), along with all four sections:
`[remotes]`, `[[actions]]`, `[vars]`, and `[default-disabled]`. What is not
built is two of the value shapes below: a `file` or `archive` remote, and a
table-valued variable.

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
must still match one of the known value shapes. The built half of that rule,
including the two naming rules the keys themselves follow, is specified in
[`docs/repoformat.md`](../repoformat.md#top-level-schema).

## Shared Value Types

The schema uses these reusable value shapes.

### String-valued variables

That a variable is read only by a condition, and that a static `[vars]` value is
a string and nothing else, are specified in
[`docs/repoformat.md`](../repoformat.md#variables). The persisted machine-local
layer follows the same rule and is built; its document is specified in
[`docs/state.md`](../state.md#varstoml-machine-local-variables). The rule extends
to every remaining layer that can produce a variable, none of which is built:
per-inclusion overrides and dynamic-command results are strings too, and batfiles
does not infer types from their contents.

The one exception is built and specified with the
[truthiness table](../repoformat.md#truthiness): a condition is where a string
has to become a decision. Nothing else re-types a value.

### Condition

Conditions are built and specified in
[`docs/repoformat.md`](../repoformat.md#conditions), along with the two fields
that spell one, the namespaces they read, the truthiness table, the rule that a
record writes one of the two or neither, and what [a condition that cannot be
evaluated](../repoformat.md#when-a-condition-cannot-be-evaluated) does. One
thing about them is not built.

**The record that does not have one yet.** An `include-remote` takes a condition
too, and that record does not exist; it is specified with its own schema below.
A [remote](../repoformat.md#remotes) and a `[default-disabled]` entry each accept
one already, and nothing evaluates either: a remote is materialized whatever its
condition says, and nothing adopts a candidate.

For concision, schema tables below list only `when`. Every record that accepts
`when` also accepts `unless` as its negated alias, on the terms the built
[conditions](../repoformat.md#conditions) section gives them.

### Repository path

The three spellings, the `@` reservation, and the rule that a named remote must
be one the manifest declares are built and specified in
[`docs/repoformat.md`](../repoformat.md#sources-and-destinations). What is not
built is who may write a remote reference.

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

The ID rule, action-ID uniqueness, remote IDs, and the variable-name rule are
specified in [`docs/repoformat.md`](../repoformat.md#names-and-ids). Clone-list
entry IDs follow the current
[entry format](../repoformat.md#the-clone-list-format). What is not built is the
`include-remote` ID and the manifest-entry IDs an inclusion makes addressable.

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

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

`[remotes]` is a map from a user-selected name to a tagged remote record. The
map, the ID its keys follow, the shared `type` / `when` / `unless` fields, the
whole of the `git` record, and [materializing
one](../repoformat.md#materialization) are built and specified in
[`docs/repoformat.md`](../repoformat.md#remotes). Three things about the section
are not.

**The other two record variants**, below. A manifest declaring either is refused
by name, so a remote that batfiles cannot fetch is never read as one it can.
Nothing downloads or unpacks a remote; only a Git one materializes.

**Reaching a materialization's content.** A remote is cloned into the repository,
and no action can install from what is there: the [repository
path](#repository-path) that names one is unbuilt.

**One field of the `git` record**: `allow-dynamic-vars`, a boolean defaulting to
`false`, which says whether an included remote's dynamic variable declarations
may be executed. It arrives with the dynamic variables it governs, at step 9.1,
and is refused as an unknown field until then.

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

That declaring a remote only names a source, and that actions decide whether and
where its content is installed, holds for these two as it does for a `git`
remote and is specified with it.

## Variables

The static half of `[vars]` — the map, its name rule, and its string-only
values — is specified in
[`docs/repoformat.md`](../repoformat.md#variables). What is not built is the
other value shape: a table is a dynamic variable record, which arrives at step
9.1 and is rejected as a non-string value meanwhile.

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
| `command-timeout` | duration string                    |    no    | `"5s"`     | Maximum command runtime; must be greater than zero.                             |

Arbitrary table-shaped variable values are not supported; every table value in
`[vars]` must match this closed dynamic-variable record.

A captured value holds at most 1 MiB. A `capture = "stdout"` command that writes
more than that fails rather than having its output truncated, in the same way
that output which is not valid UTF-8 fails rather than being reinterpreted. See
[how dynamic commands are run](environment.md#how-dynamic-commands-are-run).

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
invalid.

Zero is valid as a `cache`, where it means the value is never fresh. It is not
valid as a `command-timeout`: a zero timeout asks for a command that is
guaranteed to fail, so `command-timeout` must be greater than zero.

## Default-Disabled Bootstrap Entries

The part of this section that runs — the two arrays of closed records, their
required `id` and `group` fields, and what is checked as the manifest is read —
is specified in [`docs/repoformat.md`](../repoformat.md#default-disabled-bootstrap-entries),
along with the `when` and `unless` an entry accepts. Two things about it are not
built.

**Adoption**, which is the whole point of the section. Nothing reads the
candidates today. A bootstrap command resolves them against its own enable and
disable options and writes the outcome to `disabled.toml`, following the
environment specification's [bootstrap adoption
precedence](environment.md#bootstrap-enable-and-disable-lists); until that
command exists, a manifest declaring candidates is accepted and has no effect.

**Evaluating an entry's condition**, which is part of the same command: a
candidate is adopted only on the machines its condition suits. The conditions are
parsed and checked today and decided nowhere, because there is no adoption for
them to qualify.

`[default-disabled]` in an included remote is structurally valid but ignored;
bootstrap policy belongs to the leaf repository.

## Actions

Current action types and their shared fields, `when` and `unless` included, are
specified in [`docs/repoformat.md`](../repoformat.md#actions). `create-dir` and
[`git-clone`](../repoformat.md#git-clone) — the latter including the
conservative update policy a later `sync` applies to the clone it finds and the
[`ref`](../repoformat.md#ref-following-one-branch-tag-or-commit) that says which
branch, tag, or commit it should be on — are built with nothing outstanding. The
sections below are the actions with something left to build.

### `symlink-dir`

The action itself — `source-dir`, `dest-dir`, and `dot-prefix` — is built and
specified in [`docs/repoformat.md`](../repoformat.md#symlink-dir). Only the two
filters below are not built, and a manifest that writes either is rejected.

```toml
[[actions]]
type = "symlink-dir"
source-dir = "files"
dest-dir = "~"
include = ["zshrc", "config"]
exclude = "private"
```

| Field     | Type         | Required | Description                    |
|-----------|--------------|:--------:|----------------------------------|
| `include` | `GlobFilter` |    no    | Direct child names to include. |
| `exclude` | `GlobFilter` |    no    | Direct child names to exclude. |

These filters are unbuilt; the pattern dialect remains open. Each child name is
one path segment, so `/` cannot match here. The proposed [`copy`](#copy) filters
select recursively.

### `copy`

`copy` and `copy-dir` are specified in
[`docs/repoformat.md`](../repoformat.md#copy). Only the two filters below are
not built, and they are rejected on both.

```toml
[[actions]]
type = "copy-dir"
source-dir = "local-files"
dest-dir = "~"
include = ["*"]
exclude = ["private/*"]
```

| Field     | Type         | Required | Description                                       |
|-----------|--------------|:--------:|-----------------------------------------------------|
| `include` | `GlobFilter` |    no    | Recursive selection when the source is a directory. |
| `exclude` | `GlobFilter` |    no    | Recursive exclusion when the source is a directory. |

These filters are unbuilt. Selection is recursive, so patterns may contain `/`.

### `git-clone-list`

Built and specified in
[`docs/repoformat.md`](../repoformat.md#git-clone-list), along with the [clone
list format](../repoformat.md#the-clone-list-format) it reads, the per-entry
`ref=` it honors, and the per-entry `when=` and `unless=` it decides. One thing
about it is not built.

**Entries are not individually selectable.** An entry may carry an `id`, and an
`<action>.<entry>` address may be written in `disabled.toml` or passed to
`--skip-action`; nothing resolves one, which is the outcome every list holding
an address already has a rule for. What has to happen for one to resolve is a
step of its own. A per-entry condition now covers the case that motivated it —
a plugin one machine wants and the others do not — so what is left is per-machine
selection the machine states for itself rather than the repository.

#### When the list is read

[Leaf clone lists](../repoformat.md#git-clone-list) are prepared before action
writes. For remote inclusion, materialize the source first, then prepare the
list before executing included actions. Step 7.2 owns this extension.

### `fetch-archive` entry filters

[`fetch-archive`](../repoformat.md#fetch-archive) is built. Two of the fields
specified for it are not: `include` and `exclude`, which select which of an
archive's entries are unpacked.

| Field     | Type         | Required | Default     | Description                          |
|-----------|--------------|:--------:|-------------|--------------------------------------|
| `include` | `GlobFilter` |    no    | all entries | Entries to include while extracting. |
| `exclude` | `GlobFilter` |    no    | none        | Entries to exclude while extracting. |

```toml
[[actions]]
type = "fetch-archive"
source = "https://example.com/tool.tar.gz"
dest = "~/.local/tool"
archive-root = "*"
include = ["bin/*"]
exclude = ["*.md"]
```

They match against an entry's path with `archive-root` already stripped, so a
filter is written against the tree as it will be installed rather than as the
archive spells it. Neither is accepted today: a manifest that writes one is
rejected, rather than installing more of an archive than it asked for.

They have no step. A named `archive-root` already installs one directory out of
an archive and nothing beside it, which is what the two repositories driving this
project would have wanted a filter for, and `GlobFilter` itself is not built
until step 7.3. Whichever of those arrives first is when this is worth revisiting.

A `file://` source is not fetched by either fetching action yet; it arrives with
file remotes at step 9.3.

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

The `remote` field must name a Git remote the same repository declares:
naming an undeclared remote, or a declared `file` or `archive` remote, is
invalid configuration.

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

## Serializer-Oriented Summary

A serializer or deserializer needs to account for four unions:

1. `Remote` is tagged by `type` as `git`, `file`, or `archive`.
2. `Action` is tagged by `type`; current variants are in the
   [action schema](../repoformat.md#actions).
3. A `[vars]` value is either a string or a dynamic-variable record.
4. A repository-backed source is either a string or a structured remote/path
   record.

TOML inline tables and full tables represent the same records and should
deserialize equivalently.
