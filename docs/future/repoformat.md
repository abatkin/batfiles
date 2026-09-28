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

That a Git remote may have a `batfiles.toml` of its own, read only when a leaf
`include-remote` selects that remote, is built and specified with
[`include-remote`](../repoformat.md#include-remote), along with the rule that two
inclusions of one remote share its single materialization.

## Top-Level `batfiles.toml` Schema

Built, and specified in [`docs/repoformat.md`](../repoformat.md#top-level-schema):
every section optional, no format-version field, known records closed, and every
value shape each section takes. Nothing about it is outstanding.

## Shared Value Types

The schema uses these reusable value shapes.

### String-valued variables

That a variable is read only by a condition, and that a static `[vars]` value is
a string and nothing else, are specified in
[`docs/repoformat.md`](../repoformat.md#variables). The persisted machine-local
layer follows the same rule and is built; its document is specified in
[`docs/state.md`](../state.md#varstoml-machine-local-variables). An inclusion's
[`vars`](../repoformat.md#variables-for-one-inclusion) follow it and are built
too, and so do an included remote's own `[vars]` and [dynamic
variables](../repoformat.md#dynamic-variables)' results: batfiles infers no type
from any value's contents.

The one exception is built and specified with the
[truthiness table](../repoformat.md#truthiness): a condition is where a string
has to become a decision. Nothing else re-types a value.

### Condition

Conditions are built and specified in
[`docs/repoformat.md`](../repoformat.md#conditions), along with the two fields
that spell one, the namespaces they read, the truthiness table, the rule that a
record writes one of the two or neither, and what [a condition that cannot be
evaluated](../repoformat.md#when-a-condition-cannot-be-evaluated) does. Every
record that takes a condition decides it: an action, a
[remote](../repoformat.md#a-remotes-condition), and an
[`include-remote`](../repoformat.md#include-remote), which decides the inclusion
rather than the remote. A `[default-disabled]` entry accepts one that nothing
evaluates, because nothing adopts a candidate.

For concision, schema tables below list only `when`. Every record that accepts
`when` also accepts `unless` as its negated alias, on the terms the built
[conditions](../repoformat.md#conditions) section gives them.

### Repository path

The three spellings, the `@` reservation, the rule that a named remote must be
one the manifest declares, and the rule that only the leaf repository's own
actions may write a remote reference are all built and specified in
[`docs/repoformat.md`](../repoformat.md#sources-and-destinations) and
[what an included action may not write](../repoformat.md#what-an-included-action-may-not-write).
Nothing about this value shape is outstanding.

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

The ID rule, action-ID uniqueness, remote IDs, the `include-remote` ID, and the
variable-name rule are specified in
[`docs/repoformat.md`](../repoformat.md#names-and-ids). Clone-list entry IDs
follow the current [entry format](../repoformat.md#the-clone-list-format), where
one names its entry in diagnostics alone. What is not built is the address that
reaches a single entry, in a leaf's list or in one an inclusion contributed.

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

Built, all three record variants — `git`, `file`, and `archive` — along with
[materializing](../repoformat.md#materialization) each, and specified in
[`docs/repoformat.md`](../repoformat.md#remotes). What an archive remote does not
take is the pair of [entry filters](#fetch-archive-entry-filters) a
`fetch-archive` does not take either.

## Default-Disabled Bootstrap Entries

This section is built, adoption and entry conditions included, and is specified
in [`docs/repoformat.md`](../repoformat.md#default-disabled-bootstrap-entries).
Nothing about it is outstanding: `clone` resolves the candidates against the
[adoption precedence](../environment.md#bootstrap-adoption-precedence) and writes
the outcome to [`disabled.toml`](../state.md#bootstrap-adoption), and an
included remote's own `[default-disabled]` is structurally valid and ignored,
because bootstrap policy belongs to the leaf repository.

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

### `fetch-archive` entry filters

[`fetch-archive`](../repoformat.md#fetch-archive) and the [`archive`
remote](../repoformat.md#archive) are built. Two of the fields specified for
them are not: `include` and `exclude`, which select which of an archive's entries
are unpacked, and would apply to both alike.

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
archive spells it. Neither is accepted today, on either record: a manifest that
writes one is rejected, rather than installing more of an archive than it asked
for.

They have no step. A named `archive-root` already installs one directory out of
an archive and nothing beside it, which is what the two repositories driving this
project would have wanted a filter for, and nothing matches a glob anywhere in
batfiles yet. The one-or-many spelling these share with an inclusion's selection
fields is built, but that is the spelling alone: what an inclusion's fields hold
are IDs, compared for equality against what a manifest declared. A caller that
wants a glob is what makes `GlobFilter` worth building.

### `include-remote`

The record, its required `remote`, the manifest it reads, the actions it splices
into the list, how those are addressed, the four selection fields that say which
of them it takes, its `vars` overrides and the included remote's own `[vars]`
with what each reaches, what it does with a remote that is excluded or not
materialized, and the rule that an inclusion is never applyable are built and
specified in [`docs/repoformat.md`](../repoformat.md#include-remote), including
the rule that `remote` names a `git` remote. Nothing about it is outstanding.

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
