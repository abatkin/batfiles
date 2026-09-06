# Batfiles Repository Format

This document is a schema-oriented summary of the repository formats necessary
for the batfiles dotfiles manager. It intentionally gives only enough behavior
to explain the data model; the other [specification documents](README.md) define
planning, precedence, state, and command execution policy.

## Repository Layout

The part of this section that runs — a repository is a file tree with a
`batfiles.toml` at its root, and only that file has intrinsic meaning — is
specified in [`docs/repoformat.md`](../repoformat.md), along with how the
manifest is read. The `remotes/` tree below is not built.

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

Each remote materializes at `remotes/<remote-id>/` inside the leaf repository,
keyed by its `[remotes]` map key rather than by the ID of any inclusion that
selects it. An included Git remote's optional manifest is therefore
`remotes/<remote-id>/batfiles.toml`, and two inclusions of one remote share the
single materialization at that path.

## Top-Level `batfiles.toml` Schema

The part of this section that runs — every section optional, no format-version
field, and known records closed — is specified in
[`docs/repoformat.md`](../repoformat.md), along with `[[actions]]` and
`[default-disabled]`. The other two sections below parse in no repository yet:
declaring one is an error until the code that reads it exists.

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

**Variables exist only to feed conditions.** A variable is read by a `when` or
an `unless` and nowhere else: no field of any action, remote, or manifest entry
interpolates one, and the format has no interpolation syntax at all. Every rule
below follows from that, and so does the tool's freedom to build actions long
before it builds variables.

Every variable value exposed by batfiles remains a string. This includes static
repository values, per-inclusion overrides, persisted and one-shot overrides,
dynamic-command results, facts, and host environment values. Batfiles does not
infer types from their contents.

There is one exception, and it is enumerated rather than heuristic: a
[condition](#condition) is where a string has to become a decision, so a value
used in a boolean context is read through the fixed
[truthiness table](#truthiness). Nothing else re-types a value.

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
when = "work && facts.os == 'macos'"
```

Expressions use the
[Simple Expressions](https://github.com/abatkin/expressions-rs) language.
Batfiles makes user variables available as bare string-valued identifiers and
provides reserved, string-valued `facts`, `env`, and `vars` resolvers. The
resolvers accept member syntax for identifier-compatible keys, such as
`facts.os` and `env.HOME`, and index syntax for any key, such as
`env["XDG_CURRENT_DESKTOP"]`. A key missing from any of the three resolves to
the empty string instead of producing an evaluation error.

The string is parsed when the manifest is read, not when the condition is
evaluated. A malformed condition is therefore a load error that names the file,
the line the condition is written on, and the position within the condition —
the same treatment every other malformed value in the file receives. Every
condition in a manifest is parsed, including ones no evaluation will ever
reach.

Evaluation happens later, and can still fail: on an identifier no layer
declares, on a result outside the truthiness table, or on arithmetic that
overflows. A condition that cannot be evaluated **closes its gate** — the record
is excluded — and a warning names the record and the condition. The command does
not fail; one bad identifier in one third-party remote must not cost the whole
run, and nothing is silently ignored, because the warning says what happened.

Closing is the direction for `when` and `unless` alike. That is worth stating,
because the `unless` case looks like it should invert and does not: a false
`unless` *opens* a gate, so treating an unevaluable condition as false would make
a misspelt `unless = "no_gui_"` install the very thing it was written to
suppress. Excluding the record is the safe answer in both spellings.

Condition inputs follow the shared [string-valued variable
model](#string-valued-variables).

For concision, schema tables below list only `when`. Every TOML record or
manifest entry that accepts `when` also accepts `unless` as its negated alias.
The two fields are mutually exclusive; writing both on one record is a
validation error. This convention applies even though `unless` is not repeated
in each closed-record table.

#### Identifiers

A bare identifier is a user variable, and has three cases:

| The name is | Resolves to |
| --- | --- |
| declared, with a value | that string |
| declared, but its dynamic command produced no value | the empty string |
| not declared in any layer | an evaluation error |

"Declared" spans every layer that can contribute a variable: a repository's
`[vars]`, a remote's `[vars]`, an inclusion's `vars`, `vars.toml`,
`BATFILES_VAR_*`, and `--var`. The error fires only when nothing anywhere names
the variable.

This is deliberately asymmetric with `facts` and `env`, whose missing keys are
the empty string. A namespace is extensible, so a key batfiles does not define
yet is forward compatibility; a variable name is not extensible, so a name
nothing declares is a typo. Left silent, a misspelt `unless` would read as false
on every machine forever, and a false `unless` *installs* what it was written to
suppress.

#### The `vars` namespace

`vars` reads the same bindings as a bare identifier, but totally: a missing key
and a valueless variable are both the empty string.

```toml
when = "work"                     # an error if nothing declares `work`
when = "vars.work"                # false if nothing declares it
when = "vars.work || vars.school" # and it composes
```

Use it for a variable that is legitimately optional — one set with `vars set` on
some machines only, one passed as `--var` on some runs only, or one a
third-party remote's action reads and cannot require the leaf to define. For a
variable that is always meant to exist, the bare identifier is the spelling that
catches a typo.

Member syntax always works here, because every user variable name is
identifier-compatible by construction. Indexing (`vars["work"]`) is accepted for
symmetry with `env`, where it is sometimes required, but is never necessary.

#### Truthiness

Wherever a boolean is wanted — the condition's own result, and every `&&`, `||`,
and `!` operand alike — a value is read as true or false by this table:

| Value | Reads as |
| --- | --- |
| a real boolean | itself |
| a number | `false` at zero, `true` otherwise |
| `"true"`, `"1"`, `"yes"`, `"on"` | `true` |
| `"false"`, `"0"`, `"no"`, `"off"`, `""` | `false` |
| anything else | an evaluation error naming the value |

The table is closed on both sides. A value outside it is an error rather than
silently true, because `profile = "personal"` written as `when = "profile"` is a
bare identifier where a comparison was meant; reading it as true would make the
gate permanently open with nothing on screen to say so.

Comparison and `+` are unaffected: those keep Simple Expressions' own rules, so
`==` behaves exactly as the language defines it. The table above governs boolean
contexts only.

This is the one place batfiles infers anything from a string's contents, and it
is the stated exception to [string-valued
variables](#string-valued-variables)' rule that it does not. A condition is
where a string has to become a decision; the table is that conversion, and it is
enumerated rather than heuristic for exactly that reason.

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

The ID rule itself, and action-ID uniqueness, are specified in
[`docs/repoformat.md`](../repoformat.md#names-and-ids). The variable-name rule
below and remote IDs are not built. Clone-list entry IDs follow the current
[entry format](../repoformat.md#the-clone-list-format).

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

- User variable names match `[A-Za-z_][A-Za-z0-9_]*` and cannot be `facts`,
  `env`, `vars`, `true`, or `false`. Reserving all five is what makes a
  [condition](#condition)'s namespace lookup unambiguous without a precedence
  rule: no user variable can shadow a namespace. The consequence to know about
  is in `vars.toml`, whose keys are user variable names — a key literally named
  `vars` there makes the whole file fail to load, not just that one entry.
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
is specified in [`docs/repoformat.md`](../repoformat.md#default-disabled-bootstrap-entries).
Three things about it are not built.

**Adoption**, which is the whole point of the section. Nothing reads the
candidates today. A bootstrap command resolves them against its own enable and
disable options and writes the outcome to `disabled.toml`, following the
environment specification's [bootstrap adoption
precedence](environment.md#bootstrap-enable-and-disable-lists); until that
command exists, a manifest declaring candidates is accepted and has no effect.

**Conditions.** Each entry also takes `when` and `unless`, so that a candidate
is adopted only on the machines it suits. Both are rejected today, by the entry
records being closed.

```toml
[[default-disabled.actions]]
id = "core.work-tools"
when = "work"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'macos'"
```

`[default-disabled]` in an included remote is structurally valid but ignored;
bootstrap policy belongs to the leaf repository.

## Actions

Current action types and their shared fields are specified in
[`docs/repoformat.md`](../repoformat.md#actions). The proposed `when` and `unless`
fields conditionally select any action; see [conditions](#condition).

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

### `create-dir`

Built and specified in [`docs/repoformat.md`](../repoformat.md#create-dir).

### `git-clone-list`

Built and specified in
[`docs/repoformat.md`](../repoformat.md#git-clone-list), along with the [clone
list format](../repoformat.md#the-clone-list-format) it reads and the per-entry
`ref=` it honors. One thing about it is not built.

**Per-entry conditions.** `when` and `unless` are refused with the message that
names step 5.6, on the same terms as the identical fields on an action.

Entries are not individually selectable yet either. An entry may carry an `id`,
and an `<action>.<entry>` address may be written in `disabled.toml` or passed to
`--skip-action`; nothing resolves one, which is the outcome every list holding
an address already has a rule for. What has to happen for one to resolve is a
step of its own, and it is per-machine plugin selection.

#### When the list is read

[Leaf clone lists](../repoformat.md#git-clone-list) are prepared before action
writes. For remote inclusion, materialize the source first, then prepare the
list before executing included actions. Step 7.2 owns this extension.

### `git-clone`

Built and specified in
[`docs/repoformat.md`](../repoformat.md#git-clone), including the conservative
update policy a later `sync` applies to the clone it finds and the
[`ref`](../repoformat.md#ref-following-one-branch-tag-or-commit) that says which
branch, tag, or commit it should be on.

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

## Git Clone Manifest Format

The current [clone-list format](../repoformat.md#the-clone-list-format) defines
names, metadata, quoting, and collision handling. Proposed entry conditions use
the shared [condition](#condition) type and are scheduled for 5.6.

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
