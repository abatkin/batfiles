# Batfiles Repository Format

The repository format: where the manifest lives, how it is read, the variables
and remotes it can declare, and the kinds of action it can declare.

## Repository layout

A batfiles repository is an ordinary file tree with a `batfiles.toml` at its
root.

```text
dotfiles/
├── batfiles.toml
├── remotes/                # generated and owned by batfiles
└── ...
```

Only `batfiles.toml` has intrinsic meaning. Every other name in the tree becomes
meaningful when an action references it, and means nothing on its own.

`remotes/` is the exception, and it is not yours to write: batfiles generates it
to hold the [remotes](#remotes) the manifest declares, one per remote ID. What is
in it is somebody else's content rather than this repository's, so a repository
under version control should ignore it.

The **leaf repository** is the one a command works on, selected by
`--batfiles-dir` or `BATFILES_DIR`, then by a `batfiles.toml` in the current
directory, and finally by `<selected-home>/dotfiles`; see
[location selection](environment.md#location-selection). A [remote](#remotes)
repository may carry its own `batfiles.toml`; it is optional, and it is read only
when an [`include-remote`](#include-remote) selects that remote.

## Reading the manifest

`sync`, `apply-action`, and `apply-group` read the leaf manifest. A command that
never opens it cannot be failed by it: a malformed manifest does not stop an
enable, a disable, or a `vars` lookup, all of which work on machine-local state
instead.

The rules below are written for the manifest, and every document batfiles reads
follows them — with one exception, noted where it applies: a missing [state
file](state.md) is an empty document rather than an error. How a document
batfiles owns is replaced when it changes is specified alongside that one, under
[writing](state.md#writing).

- The document is read and checked whole before anything in it is used, its
  [dynamic variables](#dynamic-variables)' commands included. A command that
  cannot make sense of its manifest stops before it has done any work. An
  included manifest is read only once the leaf's variables have decided that
  its inclusion is opened, so a malformed one fails the command after the
  leaf's own commands have run, though before anything in it runs.
- A missing manifest is an error. A repository is a repository because it has
  one, so batfiles reports the path rather than proceeding as if the file were
  empty. This is the one rule a [state file](state.md) does not share: a machine
  that has disabled nothing has nothing to record, so a missing one is empty.
- A malformed document is an error, reported with the file and the position
  within it, and the file is left untouched rather than repaired or replaced.
- A valid TOML file is not automatically a valid manifest: the records below are
  closed, so an unknown key is an error too. An empty document is still a valid
  manifest, since every section is optional.
- A document that parses but breaks a rule TOML cannot express is an error on
  the same terms, raised before anything acts on any of it. Two kinds: a rule
  spanning more than one record, such as the
  [uniqueness of action IDs](#names-and-ids); and a rule about the shape of a
  single value that its type does not capture, such as what a `source` and a
  `dest` may say. Both are decidable from the document alone, so both are
  settled while it is being read. The diagnostic names the action by its
  position in the file, counting from 1.

## Top-level schema

There is no format-version field, and every top-level section is optional. Four
sections exist:

```toml
[remotes]                  # map<ID, Remote>

[[actions]]                # ordered list<Action>

[vars]                     # map<variable name, string | dynamic variable>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]
```

Known records are closed: an unknown key, in the document or in a record, is
invalid. That is what a field from an unbuilt part of the format runs into, and
what an unbuilt *value* shape runs into is the same rule read one level down —
a `file` or `archive` [remote](#remotes) is refused by name rather than
appearing to have been understood.

The map keys under `[remotes]` and `[vars]` are user-chosen names rather than
schema fields, so neither section is closed against the names it holds; what
each name is allowed to *be* is [the two naming rules](#names-and-ids), and what
its value is allowed to be is the record or the type the section specifies.

## Names and IDs

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

- IDs, group names, and [remote](#remotes) names match that rule. In particular
  an ID cannot contain whitespace, `.`, or `,`: dots compose
  [addresses](cmdline.md#addresses), and commas delimit environment lists.
- Action IDs are unique within a repository. A repeated one is a load error
  naming both actions, because an address matching two of them could not say
  which was meant.
- A `[remotes]` key is the ID of the remote it declares, so it follows this rule
  and not TOML's looser one for a bare key. `[remotes.core-2]` is a remote;
  `[remotes._hidden]` and `[remotes."core.extra"]` are load errors, the second
  because a dotted name is already an address.
- Two remote IDs may not differ only in case. An ID is also the directory its
  remote [materializes](#materialization) in, and `core` and `Core` are one
  directory on macOS and Windows, where the second remote would find and update
  the first one's clone instead of its own. The pair is a load error on every
  platform, including the ones that would keep them apart: a manifest is the
  same repository on all of a person's machines, and a rule enforced only on
  some of them would move the failure to the machine least able to explain it.
- Group names, action IDs, and remote IDs occupy distinct namespaces. An action
  and the remote it installs from may share a name without either becoming
  ambiguous, and nothing resolves one against the other.

```text
variable name = string matching [A-Za-z_][A-Za-z0-9_]*
```

- [Variable names](#variables) follow that deliberately different rule. An
  underscore may start one and a hyphen may not appear in one, which is the
  reverse of the ID rule on both counts: `_hidden` is a name and not an ID,
  while `9front` and `oh-my-zsh` are IDs and not names. Neither rule stands in
  for the other, and a manifest uses both.
- `facts`, `env`, `vars`, `true`, and `false` cannot name a variable. They are
  the expression language's own identifiers, and reserving all five is what
  keeps a [condition](#conditions)'s namespace lookup
  unambiguous without a precedence rule: no variable can shadow a namespace.
- Names are case-sensitive, so `editor` and `EDITOR` are two variables.

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

**`url` and `ref` are [`git-clone`](#git-clone)'s `source` and `ref`, under the
name a remote declaration reads better with.** So `url` is whatever git accepts —
an `https://` URL, an `scp`-style `git@github.com:user/repo.git`, `ssh://`,
`git://`, or a plain directory on this machine — and only an empty value is
refused here. `ref` follows [the same rules](#ref-following-one-branch-tag-or-commit)
a cloned action's does: a name a branch could have is followed as a branch, and
anything else that resolves is checked out detached.

One spelling and one rule, so a repository that pins a clone and a repository
that pins a remote are written the same way and read the same way. `branch` is
not a field of either, and a manifest writing it is refused as an unknown field.

**`allow-dynamic-vars` is what lets a remote run commands on this machine.** A
dynamic variable is an arbitrary program, so the leaf decides whether the
manifest an [`include-remote`](#include-remote) reads from this remote may run
its own. Without it, each dynamic declaration in that manifest declares nothing,
and `-v` names the ones left out. The leaf's own declarations always run.

**Declaring a remote does not install anything.** It names a source, and an
action decides whether and where its content is installed. That is why the
section is a map rather than an ordered list, as `[[actions]]` is: a remote is
looked up rather than executed, and [materializing](#materialization) one
happens before the ordered list and depends on nothing in it.

**`[remotes]` means something only in the leaf repository.** The map an included
manifest declares is [ignored](#an-included-manifests-own-remotes), because
inclusion is one level deep and nothing in the run can reach one of its records.

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

**The materialization is the file itself**, at `remotes/<id>`, so a
[repository path](#sources-and-destinations) names it whole — as `@pathogen`, or
`{ remote = "pathogen" }` — and names nothing within it:

```toml
[[actions]]
type = "symlink"
source = "@pathogen"
dest = "~/.vim/autoload/pathogen.vim"
```

A path under a file remote is refused as the manifest is read, and so is a
`source-dir` naming one, since a file is never a directory. It is fetched by
[the transfer the fetching actions share](#the-transfer-both-fetching-actions-share)
and lands with the [permissions](safety.md#installed-permissions) a fetched file
does.

### `archive`

A tarball fetched from a URL and unpacked.

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

**The archive is unpacked into `remotes/<id>/` exactly as a
[`fetch-archive`](#fetch-archive) unpacks one into its `dest`**: the same formats,
the same `archive-root`, the same filters, and the same [archive
safety](safety.md#archive-extraction). An action then reads paths within it as it
would within a Git remote's clone — `@fzf/bin/fzf`.

Neither a file nor an archive remote has a manifest, so neither can be named by an
[`include-remote`](#include-remote).

### Materialization

**Declaring a remote is what brings it onto the machine.** `sync` materializes
each declared remote at `remotes/<id>` inside the leaf repository, before the
first action — cloning a Git remote, fetching a file, unpacking an archive — and
brings it up to date there on every later run. Nothing has to name a remote for
this to happen: a declared remote is materialized whether or not an action
installs from it.

The ID a remote is declared under is the directory it lands in, so two records
naming one repository are two materializations, and a
[repository path](#sources-and-destinations) reaching a remote's content names
the record rather than the URL. Because the ID is a directory name,
[two of them may not differ only in case](#names-and-ids).

**A Git remote's materialization is a clone like any other.** It follows `ref`
where one is written and the branch it is on where none is, and later runs update
it under the [Git update policy](safety.md#git-updates) — fast-forward only, and
left alone with a warning where that is not possible. [Clone
validation](safety.md#clone-validation) applies unchanged, but a `remotes/<id>`
holding something that is not a usable clone is refused under every policy
rather than backed up: `remotes/` is batfiles' own, and what it does not
recognize there is a mistake to report.

**A file or archive remote is fetched when it is missing or its declaration has
changed.** Once one is in place, batfiles writes a stamp beside it,
`remotes/<id>.batfiles-source`, recording the `type`, `url`, `sha256`,
`archive-root`, `include`, and `exclude` it was fetched from. A later `sync` compares the stamp with the
manifest:

| At `remotes/<id>` | Stamp | `sync` |
| --- | --- | --- |
| Nothing | Any, or none | Fetches it |
| The kind of node the stamp's type is | Matches the manifest | Leaves it; nothing is requested, and `-v` reports it unchanged |
| The kind of node the stamp's type is | Differs from the manifest | Fetches it again and replaces it |
| Anything else | | Refuses, naming what is there |

A digest is compared without regard to case, so rewriting one in capitals fetches
nothing, and a filter is compared as a list, so writing one pattern as a list of
one fetches nothing either; reordering a filter's patterns does. Whatever is fetched is built beside the materialization and
[replaces it](safety.md#replacing-a-materialization) only once it is complete and
verified, so a failed fetch — a server that is down, a digest that does not
match — leaves the earlier materialization and its stamp as they were, and fails
the run. To fetch one again whose declaration has not changed, run
[`sync --refresh-remotes`](cmdline.md#sync).

**What no stamp claims is not batfiles' to replace.** A `remotes/<id>` with no
stamp beside it, with one that cannot be read as one, or holding a node of the
other kind — a directory where the stamp records a file — is refused, naming the
path and what is there, and nothing is fetched. The usual cause is a clone left
by a Git remote once declared under the same ID, which may hold work of its own;
move it aside or remove it, and run `sync` again. The reverse needs nothing: a Git
remote materialized under an ID removes a stamp left beside it, since what is
there now is a clone.

**A remote that cannot be materialized stops the run**, before any action, the
way a failed action stops it. What `sync` reports while doing all this is in
[`cmdline.md`](cmdline.md#sync).

**Only `sync` materializes, and not `sync --dry-run`.** The
[apply commands](cmdline.md#apply-action) use whatever is on the machine already
and fetch nothing, and a dry run is in the same position by the
[dry-run rule](cmdline.md#dry-run-behavior): it says what it would clone, update,
or fetch, and does none of it. So an action any of them runs that installs from a
remote reads the materialization as it stands, however stale that is. Where
there is none, the action is refused by name rather than reported as a missing
file.

### A remote's condition

A remote takes a [condition](#conditions) like any other record, and what it
decides is whether this machine has the remote at all:

```toml
[remotes.corporate]
type = "git"
url = "git@git.example.com:it/dotfiles.git"
when = "work"
```

**A remote the condition closes is not materialized.** `sync` neither clones nor
updates it, and says so where it would have reported the work — see
[exclusion reporting](cmdline.md#exclusion-reporting). Nothing else about the
run changes: an excluded remote is the manifest working as written, so the
actions still run.

**What is not materialized is also not read.** A [repository
path](#sources-and-destinations) naming an excluded remote is refused by name,
and by every command rather than by `sync` alone: the apply commands materialize
nothing, but they decide a remote's condition all the same, since which trees
this machine may install from is the manifest's answer and not the filesystem's.
An action installing from a conditional remote normally carries the same
condition; one that does not is refused when the remote's condition closes.

**A materialization an earlier run left behind stays where it is.** Batfiles
removes nothing it was not asked to, so a machine that stops being a work
machine keeps `remotes/corporate` until someone deletes it. It is not read while
the remote is excluded: a manifest must not install different content on two
machines according to which of them once satisfied the condition.

A condition batfiles [cannot evaluate](#when-a-condition-cannot-be-evaluated)
closes the gate here as everywhere else, with the warning that rule specifies.

## Sources and destinations

A `source` or `source-dir` is a **repository path**: a path within a repository,
and which repository that is. Five action fields take one — `symlink`'s
`source`, `symlink-dir`'s `source-dir`, `copy`'s `source`, `copy-dir`'s
`source-dir`, and `git-clone-list`'s `source`. The fetching actions and
`git-clone` name something off this machine instead, and their `source` syntax
is in their own action tables.

A repository path is written in one of three ways:

```toml
source = "files/zshrc"                              # this repository
source = "@core/files/zshrc"                        # the remote `core`
source = { remote = "core", path = "files/zshrc" }  # the same, written out
```

A plain string is a path within the repository that declared the action. A
string beginning with `@` is shorthand for the structured form, and the two are
the same value: `@core/files/zshrc` and
`{ remote = "core", path = "files/zshrc" }` resolve alike and are quoted alike
in diagnostics. The structured form is closed and requires `remote` — a table
with only a `path` is the plain string written the long way around and means
nothing else. Without `path`, it is `@<remote>`.

**The remote must be one the same manifest [declares](#remotes).** A path naming
a remote no `[remotes]` entry declares is refused when the manifest is read,
before anything runs, and the diagnostic says which record to add. A path
resolves within that remote's [materialization](#materialization), which has to
already be on this machine: `sync` brings the declared remotes down before the
first action, and a path into one that is not there is refused by name rather
than reported as a missing file. So an [apply command](cmdline.md#apply-action),
which materializes nothing, can only install from a remote `sync` has already
brought down.

**A [file remote](#file) is named whole.** Its materialization is the one file,
so `@pathogen` and `{ remote = "pathogen" }` name it, a path within it is refused,
and so is a `source-dir` naming it. Every other remote is a tree, and is named by
a path within it.

**`@` at the start of a repository path is reserved and has no escape.** The
character introduces a remote reference wherever a repository path begins with
it, in both spellings: a `path` inside a structured reference may not begin with
one either, so a second `@` cannot start the reference over. Elsewhere in a path
it is an ordinary character — `files/@work/zshrc` is a path like any other. A
repository whose files begin with `@` therefore cannot name one as a source.

Every other rule holds whichever tree the path reads from. A repository path is a
nonempty relative path within that tree. It may contain `.` and `..` only if it
remains strictly inside; `.`, `./`, `shell/..`, and a reference naming a remote
and no path cannot name the whole of a tree — a file remote, which is not a
tree, aside. Use `/` separators. Anchored paths
and drive prefixes are rejected according to the host's path syntax. A symlink
stored inside a repository may point outside it. Diagnostics name the tree the
path is read from, so a refusal about `@core/../secrets` says `remote core`
rather than sending the reader to the wrong repository.

Sources are checked for presence when the action runs, including dry runs. A
final broken symlink counts as present for linking; a copy must be able to read
its target. `source-dir` must resolve to a directory. Executable clone lists are
read during preparation before any action writes.

`dest` and `dest-dir` use the selected home as their relative base:

| Form | Resolution |
| --- | --- |
| `~` | Selected home |
| `~/path` or `path` | Path relative to selected home |
| Absolute path | Used as written |
| Empty value or `~other` | Invalid |

Destinations may leave the selected home. Roots are anchored and paths are
normalized lexically. [Location selection](environment.md#location-selection)
defines the roots; [installation safety](safety.md) defines filesystem
resolution, source containment checks, occupied destinations, and staging.

## Entry filters

`include` and `exclude` choose which entries of a tree an action installs.
[`symlink-dir`](#symlink-dir), [`copy`](#copy), [`copy-dir`](#copy-dir),
[`fetch-archive`](#fetch-archive), and the [`archive` remote](#archive) take
them. Each is one glob or a list of them:

```toml
include = "*rc"
exclude = ["private", "*.bak"]
```

**An entry is named by its path from the root of what is filtered**, with `/`
between its segments on every platform; each record says what its root is.
A pattern matches the whole of that path, so `*.bak` matches only an entry at the
root and `**/*.bak` matches one at any depth.

| Syntax | Matches |
| --- | --- |
| `*` | Any run of characters within one segment, including a leading `.` |
| `?` | Any one byte within one segment |
| `[abc]`, `[a-z]`, `[!abc]` | One byte from, or not from, the class, within one segment |
| `{a,b}` | Either alternative, within one segment |
| `**` | Any number of whole segments, none included, written as a segment of its own |

A pattern is matched one `/`-separated segment at a time, so only `**` reaches
past a segment: `a[!x]b` matches `a-b` and never `a/b`. A class or alternative
containing `/` is refused. Matching is case-sensitive on every platform, and `\`
is an ordinary character rather than an escape; write `[*]` to match a literal
`*`.

**`?` and a class match one byte, not one character.** A character outside ASCII
is more than one byte in a name, so neither matches it: `?.txt` and `[é].txt`
both miss `é.txt`. Write the character itself, or `*`, which matches it as it
matches any run of bytes.

**A pattern matching a directory decides everything beneath it.** An entry is
installed when it or one of the directories holding it matches an `include` —
or no `include` is written — and neither it nor any of them matches an
`exclude`. So `include = "bin"` takes all of `bin`, `exclude = "private"` leaves
out all of `private`, and an exclude wins wherever the two meet:
`include = "bin"` with `exclude = "bin/secret"` installs `bin` without `secret`.
`include = []` selects nothing.

**A directory that is not selected is still made to hold what is.** Where a
filter selects entries below the root, as every record but `symlink-dir` does, a directory
neither selected nor excluded is created when something beneath it is selected,
and left out when nothing is. `include = "scripts/lib"` installs `scripts`
holding only `lib`.

**A pattern no entry could have is refused as the manifest is read**: an empty
one, one beginning with `/`, one with an empty, `.`, or `..` segment — `bin/`
included — and one that is not a glob at all.

**A pattern that matched nothing is said at `-v`, and is not an error.** A
directory's contents change, so a pattern can be waiting for a file that is not
there yet. The line names the pattern and the tree it was matched in.

## Actions

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

A group is a name several actions share, so that a later command can talk about
all of them at once. An action belongs to at most one, named by its `group`
field.

**Nothing declares a group.** There is no `[groups]` section and no list to
register a name in: a group exists because some action names it, and it holds
exactly the actions that do. A name no action uses is therefore not an unknown
group but no group at all, and a `group` value is checked as an
[ID](#names-and-ids) rather than resolved against anything.

Membership says nothing about order or adjacency. Actions run in declaration
order whatever their groups are, so a group's actions may be spread through the
manifest with others in between, and grouping them together is a convenience for
whoever reads the file rather than something batfiles requires or arranges.

Group names and action IDs are separate namespaces, so a group may share a name
with an action without either becoming ambiguous.

Use a group to apply, disable, or skip its members together. The command-line
reference owns [selection and apply overrides](cmdline.md#selecting-what-a-run-does)
and [verbose action headings](cmdline.md#sync).

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

`source` is the link's target and `dest` is the link itself; both follow
[Sources and destinations](#sources-and-destinations). The anchoring matters more
here than elsewhere, because a symlink stores the target it is handed and reads
it back relative to the link's own directory: a target left relative to the
working directory would point somewhere other than where it was meant to.

Missing links are created, correct links are kept, and replaceable links are
repaired. Anything else in the way is a
[conflict](safety.md#conflicts-and-backups): backed up and replaced by default.
The safety reference owns
the [destination policy](safety.md#replacing-what-is-already-there) and the
[source-containment check](safety.md#installing-into-what-you-install-from),
including its exception for an already-correct link.

On a platform where batfiles cannot create a symlink, a `symlink` action is an
error naming the action rather than a silent skip or a copy substituted for the
link. The check happens before the destination is examined, so the refusal
arrives without anything already there having been inspected or touched.

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

This is the action for a directory whose contents you do not want to enumerate.
Adding a file to `source-dir` installs it on the next `sync` with no change to
the manifest, which is the whole reason it exists rather than one `symlink` per
file.

`source-dir` follows the [`source` rules](#sources-and-destinations) and
`dest-dir` the `dest` rules; both are decided while the manifest is read. A
`source-dir` naming the root of its tree is refused by the rule that already
covers it, which matters more here than for `symlink` — it would link
`batfiles.toml` and `.git` into the home rather than install one of them.

**One link per direct child, whatever the child is.** Nothing descends. A child
that is itself a directory becomes a single symlink to that directory, exactly
as a directory `source` does for `symlink`, and what is under it is reached
through that one link. So `files/config/` holding `a` and `b` installs as one
link at `~/.config`, not as a directory holding two.

**The children are linked in sorted order**, because the order a filesystem
happens to hold them in is not one anybody can diff.

`dest-dir` follows the [directory-container policy](safety.md#directory-containers);
each child's destination follows the [symlink policy](safety.md#replacing-what-is-already-there).
Children are processed in order, stopping at the first failure without rolling
back earlier children, as described under [execution failures](cmdline.md#execution-failures).

**`dot-prefix` refuses a child that is already dotted.** A `source-dir`
containing `.hidden` would install `..hidden`, which is a legal file name and
never the one that was meant, so the action fails and names the child. A
dot-prefixed directory holds undotted names.

**An empty `source-dir` links nothing, is not an error, and still creates its
`dest-dir`.** A directory that is empty today is a repository in progress rather
than a manifest that cannot be honored. The destination is what the action was
told to fill, so it is made whether or not there is anything to put in it yet —
the same directory `create-dir` makes when a manifest asks for one outright, and
reported the same way, so an action that installed nothing does not leave a
directory in the home without saying so. That there were no children to link is
said at `-v`.

A `source-dir` that exists but is not a directory is an error, because there are
no children to link and linking the thing itself is what `symlink` is for.

The [containment rule](safety.md#installing-into-what-you-install-from) prevents
creating `dest-dir` inside `source-dir` before children are enumerated.

The platform rule is `symlink`'s: where batfiles cannot create a symlink the
action is refused by name, before the source directory is read.

**Filters choose children by name.** An [entry filter](#entry-filters) here
matches each child's name as it is in `source-dir`, before `dot-prefix`, so a
pattern containing `/` could never match and is refused as the manifest is read.
A child the filters leave out is not linked, and nothing already at its
destination is touched. Filters that select no child still create `dest-dir`, as
an empty `source-dir` does, and `-v` says that no child was selected.

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

The same two fields as [`symlink`](#symlink), installing the same thing in the
same place, and the difference is what the user gets: a detached copy that
editing does not write back into the repository and that a later `sync` will not
undo. This is the action for a file whose *initial* contents a repository wants
to supply — a machine-local override, a template to fill in.

`source` and `dest` follow [Sources and destinations](#sources-and-destinations).
The [seed policy](safety.md#seeds-do-not-replace-and-so-do-not-refuse) installs
only at a vacant destination, unless `--refresh-content` asks for it to be
[installed again](safety.md#refreshing-seeds). A directory source is installed
whole; an existing directory is not merged, even by a refresh, which replaces it
whole. Use [`copy-dir`](#copy-dir) to seed missing direct children.

Copies preserve source permissions under the shared [permission and staging
rules](safety.md#installed-permissions). A destination inside the source
directory is refused under the [containment rule](safety.md#installing-into-what-you-install-from).

Only regular files and directories are copied. Symlinks, sockets, FIFOs, and
devices nested in a copied tree are errors naming the offending path. The
manifest's source itself may resolve through a symlink.

**Filters choose what of a directory source is copied.** An [entry
filter](#entry-filters) here matches paths relative to `source`, at any depth, and
the directory installed holds only what it selects — possibly nothing, in which
case an empty directory is seeded, as an empty source would be. An entry the
filters leave out is never inspected, so a symlink among them is not an error.
Filters are for choosing among entries, and a `copy` writing them whose source is
a file fails, whatever is at its destination. The source is read to decide what
is selected in either run mode, before the destination is considered, and a
[refresh](safety.md#refreshing-seeds) compares the destination with the filtered
tree, not with the whole source.

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

This is [`copy`](#copy) done once per child, exactly as
[`symlink-dir`](#symlink-dir) is `symlink` done once per child. Each child is
seeded on its own, so a destination that already holds some of them gains the
rest — which is what makes this, and not `copy`, the way to fill in defaults
around a configuration someone already has.

`source-dir`, `dest-dir`, `dot-prefix`, the sorted order, the container rules for
`dest-dir`, and the refusal of an already-dotted child under `dot-prefix` are all
`symlink-dir`'s, unchanged. The permission and node-type rules are `copy`'s,
unchanged. What is left to say is one thing:

**One level, and no merging below it.** A child that is itself a directory is one
thing installed: reproduced whole where nothing is at its destination, and kept
untouched where something is. Nothing descends to decide entry by entry inside
it. Seeding *into* a directory the user already has would interleave two
configurations that were never written to combine, and leave nobody able to tell
afterwards which file came from where. A child being kept does not stop the
action — its siblings are still seeded.

An empty `source-dir` copies nothing and is not an error, and creates its
`dest-dir`, on the same terms as `symlink-dir`'s. A `source-dir` that exists but
is not a directory is an error.

**Filters reach below the children.** An [entry filter](#entry-filters) here
matches paths relative to `source-dir`, so `scripts` names a child and
`scripts/lib` something inside one; patterns are matched before `dot-prefix`. A
child is seeded when it, or something beneath it, is selected, and holds only
what is selected; a child with nothing selected is not seeded at all, and what is
at its destination is not looked at. Everything else is `copy`'s filter rule,
unchanged.

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

**The same bargain as [`copy`](#copy)**, with the content coming from a URL
rather than from the repository: the destination decides, missing parents are
created, and what lands is the user's from then on. Anything at all at `dest`
means the action is done — and the check comes first, so a destination that is
occupied costs no transfer. `--refresh-content` fetches it
[again](safety.md#refreshing-seeds).

`source` is a URL and is never resolved against a filesystem root. `dest` follows
[source and destination syntax](#sources-and-destinations). Fetched files use the
shared [staging](safety.md#staging-and-publication) and
[permission rules](safety.md#installed-permissions).

**What arrives is installed as a file, whatever it holds.** A `fetch-file` whose
URL names a tarball installs the tarball. Unpacking one is
[`fetch-archive`](#fetch-archive), a separate action type; `archive-root` is its
field and is unknown here, so a manifest that writes it on a `fetch-file` is
rejected rather than fetching an archive it would not unpack.

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

The sibling of [`fetch-file`](#fetch-file), and [the
transfer](#the-transfer-both-fetching-actions-share) is the same one. What
differs is only what is done with the body.

**`dest` is one name, not a merge root.** The unpacked tree is installed as a
single thing, exactly the way [`copy`](#copy) installs a directory, so anything
at all at `dest` means the action is done and no request is made — including a
directory an earlier `create-dir` left there. A manifest declaring both is
asking for the directory twice. `--refresh-content` fetches and unpacks it
[again](safety.md#refreshing-seeds), replacing the tree whole.

**Gzipped tar and plain tar, decided by reading the archive.** The format comes
from the archive's own leading bytes rather than from what the URL appears to end
in, because a release URL redirects, carries a query string, and is named by
whoever published it. A plain tar is recognized by the checksum its first header
carries of itself rather than by any one format's magic, so V7, `ustar`, GNU, and
pax archives are all read. A body that is not a tar at all is an error naming
what it is instead — "a zip archive", "a bzip2 archive" — rather than a failure
to parse.

**`archive-root` strips a prefix off every entry.** Release tarballs usually put
everything under one directory named for the version, and without stripping it
`dest` would hold that directory rather than the tool. `*` asks batfiles to find
it: an archive with a single top-level directory has it stripped, and one with
several is an error naming them, because there is no answer to guess at. A
written-out prefix does the same job explicitly, and doubles as a way of
installing one directory out of an archive — entries outside it are not
installed. A prefix the archive holds nothing under is an error. It is spelled
the way an entry path is, so it may not be absolute and may not contain `..`;
one that is, is refused as the manifest is read.

[Archive safety](safety.md#archive-extraction) specifies path and link checks.
[Installed permissions](safety.md#installed-permissions) specifies entry and
root modes. Extraction is staged and published only after it succeeds.

**Filters are written against the tree as it will be installed.** An [entry
filter](#entry-filters) here matches an entry's path with `archive-root` already
stripped, rather than as the archive spells it:

```toml
[[actions]]
type = "fetch-archive"
source = "https://example.com/tool.tar.gz"
dest = "~/.local/tool"
archive-root = "*"
include = ["bin", "lib"]
exclude = "**/*.md"
```

A directory entry holding something selected is installed with the mode the
archive gives it. Filters that leave nothing to install are an error, as an
`archive-root` with nothing under it is, and so is a selected hard link to an
entry they leave out. They do not relax [archive
safety](safety.md#archive-extraction). What they matched is known only once the
archive is unpacked, so a pattern that matched nothing is said at `-v` by a run
that unpacks it, and never by a dry run.

### The transfer both fetching actions share

Both fetching actions use these rules:

- HTTP, HTTPS, and [`file://`](#file-urls) URLs; up to five redirects.
- Status `200 OK` is required. Partial, empty-status, and conditional responses
  such as 206, 204, and 304 fail.
- An optional `sha256` is checked against the downloaded bytes. A mismatch
  reports both digests and installs nothing. Archives are verified before
  extraction.
- No `Accept-Encoding` request header. Proxy environment variables are honored.
- TLS certificates use the operating system's trust store, including corporate
  CAs trusted by that machine.
- Thirty seconds to connect, thirty seconds to receive response headers, and
  ten minutes total for the body.

An incomplete or failed transfer is not published. See
[staging and publication](safety.md#staging-and-publication).

#### File URLs

A `file://` URL reads a file on this machine through the same transfer: the
whole file is read, and `sha256` is checked against it, exactly as a download's
bytes are. Status codes, redirects, proxies, and timeouts have nothing to apply
to.

It names an absolute path, as `file:///home/you/tool.tar.gz` or
`file://localhost/home/you/tool.tar.gz`; on Windows, `file:///C:/tools/a.zip`.
The path is percent-decoded, so a space is written `%20`, and a `?` or `#` that
belongs in it is written `%3F` or `%23` — written as themselves, they would be a
query or fragment, which a file does not have, and a URL holding either is
refused. So is one naming any host but `localhost`: a file URL reads this
machine. Both are refused as the manifest is read. A path naming nothing, or
something that cannot be read as a file, fails when the action runs, naming the
path.

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

**`source` is whatever `git` accepts**, and batfiles hands it over as written:
an `https://` URL, an `scp`-style `git@github.com:user/repo.git`, `ssh://`,
`git://`, and a plain directory on this machine are all repositories git can
clone. Only an empty value is rejected here — what the rest means is git's
question, and what git says when it cannot make sense of one is better than
anything batfiles could guess. Like a fetching action's `source` it is never
resolved against a root and follows none of [Sources and
destinations](#sources-and-destinations); `dest` follows all of it, and its
missing parents are created.

Batfiles runs Git from `PATH` using the
[documented environment](environment.md#variables-passed-on-to-git).
Submodules are not initialized or updated. Initialize them manually with
`git submodule update --init --recursive` when required.

#### `ref`: following one branch, tag, or commit

Without a `ref`, a clone follows whatever branch it is on and an update asks
what that branch tracks. With one, the record says where the checkout should be
and batfiles puts it there, on every run.

**What the string names is decided after the fetch, and against the remote
first.** Every remote is fetched, since any of them may be the one publishing
what was asked for. Batfiles then looks for `ref` in the clone's remote-tracking
branches — the clone's own `origin` first, where it has more than one remote — and anything
that matches is followed as a branch: a local branch of that name is checked
out, created to track the remote one where there is none, and fast-forwarded on
every later run. So `ref = "main"` means the `main` that upstream publishes,
which is the thing worth pinning; it deliberately does not mean the local
`main` a clone happens to have, because a fetch never moves that and the clone
would silently stop updating.

Anything else that resolves — a tag, a commit, a full `refs/…` name, or an
expression such as `main~1` — is checked out **detached**, which is what pins a
clone to something that does not move. Only a name a branch could have is looked
for among the remotes, so `HEAD` and `main~1` are read as the commits they
resolve to rather than as branches nothing could create. A
detached checkout sitting at the right object is the correct state rather than
something to repair, so a later run reports it as unchanged and leaves it alone.
A `ref` that resolves to nothing at all fails, naming what was asked for.

[Git updates](safety.md#git-updates) specifies dirty-worktree checks,
fast-forwarding, local-file protection, and failure handling.
[Clone validation](safety.md#clone-validation) specifies which existing
checkouts can be updated; anything else at `dest` is a
[conflict](safety.md#conflicts-and-backups). Existing clones retain their
configured remotes.

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

Use this action for a list of plugin repositories, with one repository per line
in the [clone list format](#the-clone-list-format).

`source` names the list file, in this repository or in a remote it names; each
line supplies a Git repository source. What a line names was never a repository
path, so where the list itself came from changes nothing about how it is read.
A warning about an entry names the list as the manifest wrote it, remote and
all. `dest-dir` is the container for the resulting clones,
with one child directory per entry. Both action fields follow
[Sources and destinations](#sources-and-destinations).

Selected, unskipped lists must already exist when execution is prepared; an
earlier action cannot produce a list for the same run. Skipped lists are not
opened. See [clone-list preparation](cmdline.md#clone-list-preparation) for
validation order and failures.

The destination directory is created before processing entries, even for an
empty list. Entries are cloned in list order using [`git-clone`](#git-clone)'s
behavior, including its [ref rules](#ref-following-one-branch-tag-or-commit).

An entry written with an `id` can be named on its own, as `zsh-plugins.<id>`
here; see [the clone list format](#the-clone-list-format).

Entries are processed independently: recoverable failures warn and leave later
entries eligible to run. The command reference owns
[clone-list warning and exit semantics](cmdline.md#clone-list-entry-failures).

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

**`remote` names a declaration, not a URL.** It must be a key of `[remotes]` in
the manifest writing the inclusion; naming one nothing declares is refused as the
manifest is read, the way a source naming an undeclared remote is. So is naming
a [`file`](#file) or [`archive`](#archive) remote, which has no manifest to read. One remote may
be included more than once, and every inclusion of it reads the one
[materialization](#materialization) its ID owns.

**What is read is `remotes/<remote>/batfiles.toml`.** A remote's manifest is
optional, because a remote whose content an action merely installs from has no
use for one; an inclusion is what asks for one, so a materialization without one
is refused by name rather than treated as a repository that includes nothing.
Everything in it is an ordinary manifest, read and checked as a leaf's is, but
for the three things [inclusion being one level
deep](#what-an-included-action-may-not-write) settles: an included action may not
source from a remote, an included `include-remote` is left out, and the
`[remotes]` the manifest declares is ignored.

**What it reads is spliced into the list where the inclusion is written.** The
actions it contributes run in the order the included manifest declares them, in
the inclusion's position among the leaf's own records, and each is carried out
exactly as the leaf declaring it would have been. An included action's
[repository paths](#sources-and-destinations) are read from that remote's
materialization rather than from the leaf repository, including the file a
[`git-clone-list`](#git-clone-list) names.

**Which tree is read is settled before the list runs.** `sync` materializes
declared remotes ahead of the first action, so an inclusion reads what that run
brought down. Every other command reads what is already on the machine and
fetches nothing, which is where a plan can come out [partial](cmdline.md#plan-completeness).
The whole list is expanded before the first action runs, so an inclusion that
cannot be read fails the run having installed nothing, as a malformed clone list
does.

**A remote its own [condition](#a-remotes-condition) closes includes nothing.**
The inclusion is skipped for that reason, as a record its own condition closed
would be, and the run carries on: a manifest leaving something out on this
machine is the manifest working as written, and the plan is still whole.

```text
include-remote corp (group work) - skipped: remote `corporate` is excluded here: when "work" is false
```

The inclusion takes a `when` or `unless` of its own as well, deciding the
inclusion rather than the remote, and one closed that way is skipped like any
other action.

The `id` is what makes the included actions and groups addressable, by standing
as the first segment of a qualified [address](cmdline.md#addresses) such as
`corp.zshrc`. It is the inclusion's own name and need not match `remote`. An
inclusion written without an `id` still contributes its actions, and nothing can
name them: running and being addressable are separate questions. The command
reference owns what those addresses resolve to, including why
[`apply-action` naming an inclusion is refused](cmdline.md#apply-action).

**Every inclusion is named in reports, whether or not it has an `id`.** One with
an `id` is called ``include-remote `corp` ``, since that is what a reader would
type. One without is called by the position it was written at and the remote it
includes:

```text
include-remote action 2 of remote `corporate`
```

That name is stable for a manifest and shared by no two of its inclusions, so
two inclusions of one remote written without IDs are still told apart — in the
lines about records they contributed, in their filter warnings, and in their
[variable blocks](#variables-for-one-inclusion). A record such an inclusion
contributed keeps the names the manifest that declared it wrote, since no address
reaches it, and the line says which inclusion it came from:

```text
symlink zshrc (group shell, from include-remote action 2 of remote `corporate`)
```

**Naming the inclusion names everything it contributed.** It is a record in the
list rather than a phase beside it, so a [group](#groups) holding one reaches what
it brought in, and a closed condition on it leaves the included manifest unread.
A run [excludes an inclusion as a unit](cmdline.md#selecting-what-a-run-does) for
the same reason.

#### Selecting part of a remote

The four selection fields say which of the remote's actions this inclusion
takes. Each is written as one ID or a list of them — `exclude-actions = "p10k"`
and `exclude-actions = ["p10k"]` are the same list — and each names records the
way the manifest that declared them does: unqualified, since the qualifier is
this inclusion's own `id`.

**With none of them written, the inclusion takes everything.** An absent field
and an empty list are different: `install-actions = []` is a selection that
names nothing and takes nothing, while omitting the field makes no selection at
all.

**One selection, optionally narrowed.** At most one of `install-actions`,
`install-groups`, and `exclude-groups` may be written: each describes the whole
selection, and a record writing two of them has described it twice.
`exclude-actions` narrows a selection rather than making one, so it goes with
either group filter or by itself; with `install-actions`, which already names
every action to take, it could only contradict it. Any other pair is refused as
the manifest is read.

**A filter names what the remote's manifest calls a record.** An included action
written without an `id` is reached by no `install-actions` and named by no
`exclude-actions`, and one written without a `group` likewise for the two group
filters. So an allow-list leaves such a record out, nothing having selected it,
and a deny-list keeps it, nothing having excluded it.

**A record the filters leave out is skipped, not forgotten.** It keeps the
[address](cmdline.md#addresses) that reaches it, so a skip or a disable naming it
is answered rather than reported as matching nothing.

**Selection is not a waiver, and it is not waivable.** An action the filters take
is still subject to everything else that leaves a record out, and an action they
leave out stays left out under every command: the filters are the leaf repository
describing what it composed, which is the same reason [a remote's own
condition](#a-remotes-condition) holds under every command. The command reference
owns where each source sits — [what a run passes over](cmdline.md#selecting-what-a-run-does),
the [per-command table](cmdline.md#selection-by-command), and the
[reported line](cmdline.md#exclusion-reporting).

**A filter name the remote does not declare warns.** The manifest has been read,
so batfiles can tell, and a name matching nothing selects and excludes nothing.
It is a warning rather than a failure, since a remote at an older revision than
the leaf expects is a repository to update and not a run to stop. Action filters
are answered by IDs and group filters by group names, so one namespace's name
never satisfies the other's filter.

```text
warning: include-remote `corp`: install-actions `nowhere` matched no action in remote `corporate`
```

#### Variables for one inclusion

`vars` declares [variable](#variables) values for what this inclusion
contributes: the leaf saying what it is including the remote *for*, so that the
conditions the remote wrote decide the way this repository wants them decided.

```toml
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
```

Names and values follow the rules `[vars]` follows, checked as the document is
read: a name the [variable rule](#names-and-ids) refuses, and a value that is not
a string, each fail the manifest at the line they are written on. An omitted
field and an empty table say the same thing, which is that the inclusion
overrides nothing.

**They reach what the inclusion contributes, and nothing else.** The leaf's own
records are decided against the leaf's own values, and a second inclusion of the
same remote is a separate scope: two inclusions writing different overrides get
different answers out of the same included manifest. A contributed
[`git-clone-list`](#git-clone-list) has its entry conditions decided in the same
scope its own record was.

**They sit above the leaf's `[vars]` and below everything this machine says.**
Overriding the leaf's own value, and [the remote's
own](#variables-an-included-remote-declares), is the point of the field, and this
machine overrides the field in turn, because a leaf composing a remote does not
get to overrule the machine it is being installed on. The whole order is
[variable precedence](environment.md#variable-precedence), and an inclusion's
overrides are reported at `-vv` under a heading naming the inclusion, in the
[listing format](cmdline.md#vars-list) the run's own variables use.

**The inclusion's own condition is not decided by them.** A `when` or `unless`
on the record, and [the remote's own condition](#a-remotes-condition), are read
against the leaf's variables: what an inclusion hands to the records it brings in
cannot decide whether it brings them in, and a remote is materialized once for a
run however many inclusions name it.

#### Variables an included remote declares

The included manifest's own [`[vars]`](#variables) is the lowest layer of the
scope its records are decided against, beneath the leaf's `[vars]` and beneath
everything [above that](environment.md#variable-precedence). A remote can
therefore say what its own conditions read without the leaf declaring anything:

```toml
# In the remote's own batfiles.toml.
[vars]
profile = "work"
```

**A leaf overrides it without having to know it is there.** Declaring the same
name in the leaf's `[vars]`, in that inclusion's `vars`, or on this machine
replaces the value the remote wrote; a name only the remote declares stands.
That is what lets a repository be composed by leaves that have never read its
variable list.

**It holds inside the inclusion that read it.** The leaf's own records are
decided against a set that never saw it, and so are a second inclusion's, so two
remotes both declaring `profile` do not collide. It reaches a contributed
[`git-clone-list`](#git-clone-list)'s entry conditions, as the rest of the scope
does.

**It cannot decide whether it is read.** The scope is derived from the manifest
the inclusion opened, so the inclusion's own condition and [the remote's
own](#a-remotes-condition) are settled before it — against the leaf's variables,
exactly as the inclusion's `vars` are.

**Its dynamic variables run only if the leaf allows them.** A remote whose
[`[remotes]` entry](#git) does not set `allow-dynamic-vars = true` runs none of
its [dynamic variables](#dynamic-variables), and each of them declares nothing,
so what is beneath it stands. Allowed, they run in the remote's materialization
as the inclusion is opened, and every inclusion of that remote shares one
capture and one [cache entry](state.md#dynamic-varstoml-dynamic-variable-cache)
per variable, whatever its `id` or `vars`. An inclusion the run does not open —
excluded, disabled, skipped, or not reached by an apply command's target — runs
nothing.

At `-vv` its declarations appear in the same block as that inclusion's
overrides, with an origin naming the inclusion that opened the manifest:

```text
include-remote `corp` variables:
  profile = "work" (batfiles.toml of include-remote `corp`)
```

#### What an included action may not write

An included action's `source` names a path in the repository that declared it,
and nothing else. The [remote reference](#sources-and-destinations) spellings —
`@core/files/zshrc` and `{ remote = "core", path = "files/zshrc" }` — are refused
in a manifest read as an inclusion, whether or not that manifest declares a
remote by the name.

Remote references belong to the leaf repository. That is what keeps inclusion one
level deep: an included repository does not reach further repositories, and it
cannot reinterpret a name the leaf declared as one of its own.

An `include-remote` in an included manifest follows from the same rule, and is
**left out rather than refused**: the record warns and the rest of the manifest
is contributed as usual. The manifest breaking the rule belongs to someone else,
and what it declares beside the nested inclusion is still good — but a
declaration that is not honored is worth saying out loud rather than passing over
in silence.

```text
warning: not included: include-remote corp.shared; an included repository does not reach further repositories
```

The remote such a record names is not required to exist. What it would resolve
against is the included manifest's own [`[remotes]`](#an-included-manifests-own-remotes),
which is ignored, so the record is left out whichever name it wrote.

#### An included manifest's own `[remotes]`

The third thing the one-level rule settles, and the one about a section rather
than a record. An included manifest's [`[remotes]`](#remotes) is **read as part
of the document and then ignored**: nothing materializes it, and nothing in the
run can name one of its records — an included action may not source from a
remote, and an included `include-remote` is left out.

Ignored includes unchecked. A record there is read far enough to be a `[remotes]`
entry this batfiles understands, and no further: an empty or malformed `url`, a
`sha256` that is not a digest, a `ref` written with nothing in it, and two keys
differing only in case each fail the repository that declared them **where that
repository is the leaf**, and none of them fails a run that merely includes it. A
`type` the schema does not name, or a field its record does not have, is still
refused, because the document stops being one batfiles can read.

The map is reported once per inclusion, naming every record passed over, for the
same reason a dropped nested inclusion is reported: a declaration that is not
honored is worth a line. A manifest declaring no remotes says nothing.

```text
warning: include-remote `corp`: ignoring the remotes `corporate` declares (`shared`, `vendor`); an included repository does not reach further repositories, so nothing materializes them and no included action can name one
```

## The clone list format

The file a [`git-clone-list`](#git-clone-list) names is not TOML. It is one
repository per line, in the shape a hand-maintained list of plugins already
takes:

```text
# vim plugins
https://github.com/tpope/vim-fugitive.git
https://github.com/vim-airline/vim-airline
https://github.com/romkatv/powerlevel10k.git dest-name=p10k id=p10k  # the prompt
```

An entry is a repository, optional `key=value` metadata beside it, and an
optional comment:

```text
<repository> [<key>=<value> ...] [# <comment>]
```

The repository is the first whitespace-delimited field, and it is whatever `git`
accepts, exactly as [`git-clone`](#git-clone)'s `source` is: only an empty one is
refused here. **Blank lines and comments declare nothing.** The first `#`
outside a quoted value begins a comment and nothing after it is read, so a
comment may contain anything at all — including text that looks like metadata. A
`#` that is part of a repository URL has to be written `%23`.

| Key         | Type      | Description                                                |
|-------------|-----------|-------------------------------------------------------------|
| `id`        | `ID`      | Addresses the entry, and names it in what the run says.     |
| `ref`       | string    | The branch, tag, or commit to follow.                       |
| `dest-name` | string    | What to call the clone, in place of the derived name.       |
| `when`      | condition | Clone the entry only where the condition is true.           |
| `unless`    | condition | Clone it only where the condition is false.                 |

Any other key is an error: a misspelled one, writing a key twice, writing `key=`
with no value, and writing a field after the repository that is not `key=value`
at all — most often a second repository on the same line.

**The two condition keys are an [ordinary condition](#conditions)**, decided
against the same variables an action's is, and refused on the same terms: one to
a line, parsed as the list is read. This is per-machine plugin selection — one
list, shared across machines, with the entries each machine wants:

```text
https://github.com/zsh-users/zsh-syntax-highlighting.git
https://github.com/company/internal-zsh-tools.git when="work"
https://github.com/foo/mac-only.git unless="facts.os != 'macos'"
```

Entry conditions follow the same [fail-closed semantics](#when-a-condition-cannot-be-evaluated)
as action conditions. See [preparation](cmdline.md#clone-list-preparation) for
evaluation timing and [exclusion reporting](cmdline.md#exclusion-reporting) for
output examples.

**An `id` makes the entry addressable**, under its list: in a list declared as
`zsh-plugins`, the first example's `id=p10k` entry is `zsh-plugins.p10k`, which a machine can
[disable](cmdline.md#enable-and-disable-actions-or-groups), skip for one run, or
apply on its own. A condition is the repository choosing entries per machine;
an address is how a machine chooses for itself. The command reference owns what
an entry [address](cmdline.md#addresses) reaches and [what each command
waives](cmdline.md#selection-by-command). A line about an entry that carries an
`id` says `id=p10k, plugins.txt line 3`, where one without is named by its file
and line alone.

An `id` follows the [ID rule](#names-and-ids), which is not the rule a directory
name follows: `ack.vim` is a perfectly good directory and not a valid ID,
because a dot composes a qualified address. So an entry's ID is never derived
from its name; write one when you want to name the entry.

**Values may be quoted**, with `'` or `"`, which is what lets one hold a space or
a `#`. Inside a quoted value the only escapes are `\\`, `\"`, and `\'`; a
backslash before anything else is an error rather than a newline or a silently
dropped character. A quote that never closes is an error too, since it would
otherwise swallow the rest of the line.

### What a clone is called

Without `dest-name`, the directory an entry clones into is derived from the
repository: everything after its last `/` or `:`, with a trailing `.git`
removed. Both characters are separators because both end a repository name —
`git@github.com:repo.git` has no slash, while `ssh://git@host:2222/user/repo.git`
has a colon that is a port, and taking whichever comes last reads every form
correctly. The derivation is textual and does not ask what kind of URL it is
looking at.

Derived or written, the name must be **one ordinary directory component**: not
empty, not `.`, `..`, or `.git`, and holding no `/`, `\`, or `:`. `dest-name` is
a name and not a path — an entry cannot install outside the directory its action
declared, whether by climbing out of it or by naming a directory of its own. A
repository whose last component is no directory name, such as one ending at its
own separator, is an error that asks for a `dest-name`; a Windows path is the
ordinary case of it, its drive letter being the last separator.

**Two entries may not clone into one directory**, and two may not share an `id`.
Both are caught as the list is read, and both name the earlier line as well as
the later one. The same repository under two names is not a repeat: what has to
differ is the directory.

**Two names that differ only in case are one of those directories**, and are
refused on every platform — including the ones where they genuinely are two.
`Plugin` and `plugin` are one directory on Windows and on a typical macOS
volume, and a list is meant to read the same on every machine that shares the
repository. Where they do collide, the second entry would find the first's
clone and be satisfied by it, and an update never asks which repository a clone
came from ([the remote a clone fetches from](safety.md#git-updates)),
so the wrong repository would sit there reporting success on every run. Case
that carries a real difference is untouched: only names that are the same word
collide, and a clone still lands under the name exactly as it is written.

Every fault names the file, the line, and what is wrong with it, and the first
one stops the run — a list with two mistakes reports the earlier one and the
next run reports the rest, which is how the manifest's own rules behave.

## Variables

`[vars]` is a map from a [variable name](#names-and-ids) to a string, or to a
[dynamic variable](#dynamic-variables) whose value a command produces:

```toml
[vars]
work = "false"
profile = "personal"
rank = "3"
email = { command = ["git", "config", "user.email"], cache = "1d" }
```

**Variables exist only to feed conditions.** A variable is read by a `when` or an
`unless` and nowhere else: no field of any action interpolates one, and the
format has no interpolation syntax at all.

**Every value is a string, and only a string.** Booleans, integers, floats,
dates, and arrays are not variable values, so `work = true` and `rank = 3` are
errors naming the line they are written on rather than values converted to
`"true"` and `"3"`. Write the string. An empty value is a legitimate one. A
*table* is not a value either: it is a [dynamic variable](#dynamic-variables),
and must be exactly that record.

What is checked is the name and the shape of the value, both while the document
is being read. Declaring the section installs nothing on its own: a manifest
whose records carry no [condition](#conditions) installs exactly what it would
have installed without a `[vars]` at all — although its dynamic variables' commands
still run.

**This is the lowest layer of the leaf repository's own set.** Machine-local
values in [`vars.toml`](state.md#varstoml-machine-local-variables), the
`BATFILES_VAR_*` environment, and `--var` each override a name declared here, in
that order. Every command that executes actions merges them into one flat set
and prints it at `-vv`; the rule is [variable
precedence](environment.md#variable-precedence). A record an
[`include-remote`](#include-remote) contributed is decided against that set with
two more layers in it: [the inclusion's own `vars`](#variables-for-one-inclusion)
directly above this one, and [the included remote's own
`[vars]`](#variables-an-included-remote-declares) below it.

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

The record is closed, and every rule on it is checked as the document is read,
with the line it is on: an unknown field, an empty `command` list, a `capture`
other than the two, a duration that does not parse, and a zero
`command-timeout` are each refused by name.

**A command is an arbitrary program, run as you.** It runs in the root of the
repository that declared it, with batfiles' own environment, and in a dry run
too; [how dynamic commands are
run](environment.md#how-dynamic-commands-are-run) is the whole contract. A
repository's own declarations always run. An included remote's run only when
the leaf [allows it](#git), and only for an inclusion the run opens.

**What it produces is a string, like every other value.** `capture = "stdout"`
takes the command's standard output with surrounding whitespace trimmed, and
requires it to exit zero; output that is not UTF-8, or more than 1 MiB of it, is
a failure rather than being reinterpreted or cut short. `capture = "status"`
takes `"true"` for an exit status of zero and `"false"` for anything else, and
reads no output at all.

**A value is cached, and reused while it is fresh.** Every command that runs one
records the result in the [dynamic-variable
cache](state.md#dynamic-varstoml-dynamic-variable-cache), and a later run uses
that entry instead until `cache` has passed. The state reference says what a
failure falls back on.

**A declaration with no value still declares the variable.** A command that
fails with nothing cached leaves the variable valueless, and a condition reads
it as the empty string — not the value a lower layer declared. That is false,
so `when = "has_op"` closes and `unless = "has_op"` opens: a repository that
must not install something when its command cannot answer should write `when`,
not `unless`.

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

A condition decides whether the record carrying it applies to this machine. Two
fields spell one, and a record writes at most one of them:

| Field    | Type      | The record applies when |
|----------|-----------|-------------------------|
| `when`   | condition | the condition is true   |
| `unless` | condition | the condition is false  |

```toml
[[actions]]
type = "symlink"
id = "gitconfig-work"
source = "git/gitconfig.work"
dest = "~/.gitconfig"
when = "work && facts.os == 'macos'"
```

Four kinds of record take them: an [action](#actions), an entry of a [clone
list](#the-clone-list-format), a [remote](#remotes), and a [default-disabled
candidate](#default-disabled-bootstrap-entries). What one decides is the
record's own: an action or an entry is carried out or passed over, and a remote
is [brought onto the machine](#a-remotes-condition) or is not. The last kind is
checked and never evaluated, because nothing adopts a candidate yet.

**Writing both on one record is a load error.** They are not one rule and its
negation applied twice, and a record writing both has no reading that is
obviously the one that was meant.

### The expression

The value is an expression in the
[Simple Expressions](https://github.com/abatkin/expressions-rs) language.
Batfiles supplies user variables as bare string-valued identifiers and three
reserved, string-valued namespaces: [`facts`](environment.md#host-facts-in-conditions)
and [`env`](environment.md#host-environment-in-conditions) describe the machine,
and `vars` is the user variables again, read totally. All three accept member
syntax for identifier-compatible keys — `facts.os`, `env.HOME` — and index
syntax for any key at all, such as `env["XDG_CURRENT_DESKTOP"]`.

**The expression is parsed when the document holding it is read**, not when it is
evaluated. A malformed condition is therefore a load error naming the file, the
line the condition is written on, and the position within the condition — the
same treatment every other malformed value gets. Every condition in a manifest
is parsed, including ones no run will ever evaluate.

### Identifiers

A bare identifier is a user variable, in [any of the four
layers](environment.md#variable-precedence) that can declare one:

| The name is            | Resolves to      |
|------------------------|------------------|
| declared, with a value | that string      |
| declared, and empty    | the empty string |
| declared nowhere       | an error         |

This is deliberately asymmetric with `facts` and `env`, whose missing keys are
the empty string. A namespace is extensible, so a key batfiles does not define
yet is forward compatibility; a variable name is not, so a name nothing declares
is a typo. Left silent, a misspelt `unless` would read as false on every machine
forever — and a false `unless` *installs* what it was written to suppress.

`vars` reads the same variables and is total, which is the spelling for one that
is legitimately optional:

```toml
when = "work"                     # an error if nothing declares `work`
when = "vars.work"                # false if nothing declares it
when = "vars.work || vars.school" # and it composes
```

Use `vars` for a variable set with `vars set` on some machines only, or passed as
`--var` on some runs only. For one that is always meant to exist, the bare
identifier is the spelling that catches a typo. Member syntax always works there,
because every variable name is identifier-compatible by construction; indexing is
accepted for symmetry with `env`, where it is sometimes required.

### Truthiness

Wherever a boolean is wanted — the condition's own result, and every `&&`, `||`,
and `!` operand alike — a value is read by this table:

| Value                                   | Reads as                     |
|-----------------------------------------|------------------------------|
| a real boolean                          | itself                       |
| a number                                | `false` at zero, else `true` |
| `"true"`, `"1"`, `"yes"`, `"on"`        | `true`                       |
| `"false"`, `"0"`, `"no"`, `"off"`, `""` | `false`                      |
| anything else                           | an error                     |

The table is closed on both sides. A value outside it is an error rather than
silently true, because `profile = "personal"` written as `when = "profile"` is a
bare identifier where a comparison was meant, and reading it as true would leave
the gate permanently open with nothing on screen to say so.

Comparison and `+` are unaffected: those keep the language's own rules, so `==`
behaves exactly as it defines it. The table governs boolean contexts only, and it
is the one place batfiles infers anything from a string's contents — enumerated
rather than heuristic for that reason.

### When a condition cannot be evaluated

A condition that parses can still fail on the machine that evaluates it: on an
identifier no layer declares, on a result outside the truthiness table, on
arithmetic that overflows. A [dynamic variable](#dynamic-variables) whose
command failed with nothing cached is not one of these: it is declared, and
reads as the empty string. **The failure closes the gate**: the record is passed
over. The command reference specifies [warnings](cmdline.md#exclusion-reporting)
and [continuation and exit behavior](cmdline.md#execution-failures).

**Closing is the answer for `when` and `unless` alike**, which is why the
warning names the spelling. The `unless` case looks like it should invert and
does not: a false `unless` *opens* a gate, so reading an undecidable condition
as false would make a misspelt `unless = "no_gui_"` install the very thing it
was written to suppress.

The warning never repeats the offending *value*, only the condition and the fix.
A condition is the one place a value reaches a diagnostic without having been
asked for — `when = "env.GITHUB_TOKEN"` puts a credential outside the table —
and the manifest batfiles is evaluating is not always the reader's own.

The [selection rules](cmdline.md#selection-by-command) determine when an action's
condition is consulted or waived.

## Default-disabled bootstrap entries

`[default-disabled]` is where a leaf repository says what a fresh machine should
start with switched off — a plugin that takes a long time to install, a group
that only belongs on a work machine. It holds two arrays of closed records:

```toml
[[default-disabled.actions]]
id = "p10k"

[[default-disabled.groups]]
group = "gui"
```

### Action entry

| Field | Type | Required | Description                            |
|-------|------|:--------:|----------------------------------------|
| `id`  | `ID` |   yes    | The action to start out disabled.      |

### Group entry

| Field   | Type | Required | Description                         |
|---------|------|:--------:|-------------------------------------|
| `group` | `ID` |   yes    | The group to start out disabled.    |

**These are candidates offered once, not a standing setting.** A repository
saying an action is default-disabled is describing where a machine starts, and
nothing more: the moment a machine has an opinion of its own, recorded in
[`disabled.toml`](state.md#disabledtoml-disabled-actions-and-groups), that
opinion is the one that counts. Nothing in the section can switch an action off
again on a machine that has already enabled it.

**Only a bootstrap reads the section: [`clone`](cmdline.md#clone), or
[`sync --bootstrap`](cmdline.md#sync) for a checkout that arrived another
way.** A bootstrap sets a machine up for the first time, and adopting the
candidates is something it does once, before its first action, writing the
outcome to [`disabled.toml`](state.md). The candidates are the lowest layer of
the [adoption precedence](environment.md#bootstrap-adoption-precedence): the
`BATFILES_*` bootstrap lists and the bootstrap's own enable and disable options
are applied over them, so whatever a repository proposes, the invocation
setting the machine up can overturn it.

A candidate is offered only to a machine with no `disabled.toml` at all, which
is the rule above read from the other side: the document existing is the machine
having said something of its own. An entry's [condition](#conditions) is decided
there too, against the same effective variable set the run uses, and one this
machine cannot decide closes the gate — so the candidate is not offered and what
it names is left enabled.

**Every other command passes the section over.** A `sync` over a manifest
declaring candidates installs exactly what it would have installed without them,
and creates no `disabled.toml`: the section says where a machine starts rather
than what every run re-applies.

What is checked is the record's own syntax. Each entry names an
[address](cmdline.md#addresses), the records are closed like every other, and an
entry missing the field that names it is an error — so a candidate that could
never mean anything is caught on the machine that writes it rather than on the
one that finally bootstraps.

What an entry *names* is never looked up, which is the same rule
`disabled.toml` follows: a candidate may legitimately refer to an action a
later branch change or Git update introduces, so there is nothing to resolve it
against and no complaint to make about a name nothing answers to yet.

Both fields hold an [address](cmdline.md#addresses), so a candidate may name
what an included remote will contribute — `core.p10k` — for the same reason it
may name what a later branch will introduce: there is nothing to resolve it
against either way.

**An entry may carry a [condition](#conditions), so that a candidate is offered
only on the machines it suits.**

```toml
[[default-disabled.actions]]
id = "work-tools"
when = "work"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'macos'"
```

It is checked as the manifest is read and evaluated nowhere, which follows from
the section as a whole not being read: there is no adoption for a condition to
qualify. So `when = "work &&"` is a load error and `when` beside `unless` on one
entry is a load error, while a well-formed condition on a candidate decides
nothing about anything today — including about the action the entry names, which
runs or does not on its own terms.
