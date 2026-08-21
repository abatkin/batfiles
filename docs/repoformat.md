# Batfiles Repository Format

The part of the repository format that runs today: where the manifest lives, how
it is read, and the one kind of action it can declare. The rest of the schema —
remotes, variables, conditions, and the other action types — is in
[`future/repoformat.md`](future/repoformat.md) until those records parse.

## Repository layout

A batfiles repository is an ordinary file tree with a `batfiles.toml` at its
root.

```text
dotfiles/
├── batfiles.toml
└── ...
```

Only `batfiles.toml` has intrinsic meaning. Every other name in the tree becomes
meaningful when an action references it, and means nothing on its own.

The **leaf repository** is the one a command works on, selected by
`--batfiles-dir` or `BATFILES_DIR` and defaulting to `<selected-home>/dotfiles`;
see [location selection](environment.md#location-selection). A remote repository
may carry its own `batfiles.toml`, and nothing reads one yet.

## Reading the manifest

`sync` reads the leaf manifest, and it is the only command that does. A command
that never opens it cannot be failed by it: a malformed manifest does not stop
an enable, a disable, or a `vars` lookup, all of which work on machine-local
state instead.

- The document is read and checked whole before anything in it is used. A
  command that cannot make sense of its manifest stops before it has done any
  work.
- A missing manifest is an error. A repository is a repository because it has
  one, so batfiles reports the path rather than proceeding as if the file were
  empty.
- A malformed document is an error, reported with the file and the position
  within it, and the file is left untouched rather than repaired or replaced.
- A valid TOML file is not automatically a valid manifest: the records below are
  closed, so an unknown key is an error too. An empty document is still a valid
  manifest, since every section is optional.
- A document that parses but breaks a rule spanning more than one record — the
  [uniqueness of action IDs](#names-and-ids), so far — is an error on the same
  terms, raised before anything acts on any of it.

## Top-level schema

There is no format-version field, and every top-level section is optional. One
section exists:

```toml
[[actions]]                # ordered list<Action>
```

Known records are closed: an unknown key, in the document or in an action, is
invalid. That is what a section from an unbuilt part of the format runs into.
`[remotes]`, `[vars]`, and `[default-disabled]` are specified in
[`future/repoformat.md`](future/repoformat.md) and are rejected until the code
that reads them exists, so a manifest declaring one fails rather than appearing
to have been understood.

## Names and IDs

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

- IDs and group names match that rule. In particular an ID cannot contain
  whitespace, `.`, or `,`: dots are reserved for composing qualified addresses,
  and commas delimit environment lists.
- Action IDs are unique within a repository. A repeated one is a load error
  naming both actions, because an address matching two of them could not say
  which was meant.
- Group names and action IDs occupy distinct namespaces.

User variable names follow a deliberately different rule, which arrives with
variables.

## Actions

`[[actions]]` is an ordered list. Each entry is a closed record selected by its
required `type` field.

| Field   | Type               | Required | Description                                                            |
|---------|--------------------|:--------:|------------------------------------------------------------------------|
| `type`  | action-type string |   yes    | Selects the action variant. `symlink` is the only one that exists.     |
| `id`    | `ID`               |    no    | Makes the action addressable.                                          |
| `group` | `ID`               |    no    | Places the action in one group. Validated as an ID; nothing selects by group yet. |

### `symlink`

Declares one symlink, from a path in the repository to a destination.

```toml
[[actions]]
type = "symlink"
id = "zshrc"
source = "shell/zshrc"
dest = "~/.zshrc"
```

| Field    | Type   | Required | Description                                  |
|----------|--------|:--------:|----------------------------------------------|
| `source` | string |   yes    | The source, relative to the repository root. |
| `dest`   | string |   yes    | The destination path, as written.            |

Both paths are resolved when the action runs, not when the manifest is read, and
both are anchored to absolute paths first. A selected root may be written
relative to wherever batfiles is invoked, but a symlink stores the target it is
handed and reads it back relative to the link's own directory, so a relative
target would point somewhere other than where it was meant to.

- `source` names a path within the repository that declares it. An absolute
  source is invalid, and so is a relative one that climbs out of the repository.
  That check is lexical rather than a canonicalization of every component, so a
  symlink deliberately stored inside the repository may point anywhere and is
  followed like any other. The path must exist: a repository naming a file it
  does not contain is a mistake batfiles can see, and the alternative is a link
  to nothing.
- `dest` beginning with `~` uses the selected home rather than an independently
  discovered one. `~user` is not expanded and is an error. A relative `dest`
  also resolves from the selected home, and an absolute one is used as written;
  `.` and `..` are resolved textually. None of this makes the home a boundary —
  a destination may deliberately point outside it.

What happens at the destination depends on what is already there:

| Already at the destination                     | Result                                                            |
|------------------------------------------------|-------------------------------------------------------------------|
| nothing                                        | The link is created, along with any missing parent directories.   |
| a symlink already pointing at the source       | Nothing, reported only at `-v`.                                   |
| a symlink pointing elsewhere in the repository | It is repointed at the source.                                    |
| a symlink pointing outside the repository      | An error naming the path and what it found, with nothing written. |
| a regular file                                 | An error naming the path and what it found, with nothing written. |
| a directory                                    | An error naming the path and what it found, with nothing written. |
| anything else                                  | An error naming the path and what it found, with nothing written. |

On a platform where batfiles cannot create a symlink, a `symlink` action is an
error naming the action rather than a silent skip or a copy substituted for the
link. The check happens before the destination is examined, so a repair cannot
remove the existing link and then discover it has nothing to put back.

The destination is examined without following a final symlink, so a link is
judged by where it points rather than by what it reaches. Where it points is
also what the symlink rows above mean by "the source", "elsewhere in the
repository", and "outside the repository": a link's target is read as the
operating system would read it, with a relative one resolved from the link's own
directory. A link spelled `../dotfiles/zshrc` can be exactly the link the action
asks for, and one spelled `<repository>/../elsewhere` leaves the repository
despite beginning inside it.

A symlink into the repository is one batfiles would have made and holds no
content of its own, so repairing it loses nothing. A file, a directory, or a
link somewhere unexpected is someone's data, and there is no backup policy yet
with which to give it back.

The four error rows are one refusal, but not one message: the diagnostic says
which of them it found, because the path alone does not tell the user whether
they are looking at a file to move, a directory to merge by hand, or a link some
other tool installed. A link is named both as it is written and as it resolves,
the two differing exactly when the target is relative, and the resolved form
being the one the row above was decided on. There is no way to waive any of
this yet; the remedy is to move the destination aside and run `sync` again.

Directory mode, which links a directory's children through `source-dir`,
`dest-dir`, and glob filters, is specified in
[`future/repoformat.md`](future/repoformat.md#symlink) and is not built. A
manifest that writes those fields is rejected rather than linking nothing.
