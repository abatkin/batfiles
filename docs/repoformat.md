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

## Sources and destinations

A `source` names a path in a repository, and a `dest` names a path on the
machine. Which of the two an action takes is a property of the action type, and
the tables below say; these rules decide what the field means wherever one
appears. Both kinds are resolved when the action runs rather than when the
manifest is read, and both are anchored to absolute paths first. A selected root may be written relative to
wherever batfiles was invoked, and a path batfiles stores on disk — the target of
a symlink, say — is read back relative to its own location rather than to that
working directory.

Resolution is lexical throughout: `.` and `..` are cancelled textually, and
batfiles does not canonicalize every component to prove where a path ends up. A
parent component that is itself a symlink is followed by ordinary
operating-system path resolution, because that link is something the user put
there deliberately.

**A `source` is contained by its repository.** It resolves from the repository
that declared it, and the result must stay inside that repository. An absolute
source is invalid, and so is a relative one that normalizes outside it, even
where the outside path exists; an internal `.` or `..` is fine as long as the
result stays in. The containment check is lexical, so a symlink deliberately
stored inside the repository may point anywhere and is followed like any other.
The path must exist: a repository naming a file it does not contain is a mistake
batfiles can see, and the alternative is installing something that points at
nothing.

**A `dest` is anchored to the selected home, which is not a boundary.** A `dest`
beginning with `~` uses the selected home rather than an independently discovered
shell home, and `~user` is not expanded and is an error. A relative `dest`
resolves from the selected home, and an absolute one is used as written. Most
destinations sit in the home by convention rather than by rule: `--home-dir`
selects the base for home-relative behavior, it does not create a jail. People
symlink parts of their home onto other volumes, and an explicit absolute or
traversing destination has to keep working.

Where the selected home cannot be determined, batfiles reports that rather than
silently substituting the working directory. See
[location selection](environment.md#location-selection).

## Replacing what is already there

Creating something where nothing exists is safe. Replacing a node that is already
there is destructive, so an action that installs something at a `dest` inspects
that destination first and decides from what it finds.

**The destination is examined without following a final symlink**, so a link is
judged by where it points rather than by what it reaches. Its target is read as
the operating system would read it, with a relative target resolved from the
link's own directory. A link spelled `../dotfiles/zshrc` may point exactly where
an action wants it to, and one spelled `<repository>/../elsewhere` leaves the
repository despite beginning inside it; judging the spelling gets both backwards.

What is found there is one of:

- **Nothing**, in which case the action creates what it was asked to, along with
  any missing parent directories.
- **A symlink batfiles owns** — one whose target resolves inside the selected
  leaf repository. Replacing it destroys nothing: the link holds no content of
  its own, and what it pointed at is left alone. A link anywhere else is
  unmanaged, even when its name is exactly the one an action would install.
- **A regular file, a directory, or none of those** — a socket, a fifo, a device.
  This is someone's data.

An action decides in this order:

1. Determine what exists, without treating a final symlink as its target.
2. If the requested result is already there, do nothing, and say so only at `-v`.
3. If it is a symlink batfiles owns, replace it directly.
4. Otherwise the node is unmanaged. Until there is a backup policy with which to
   give it back, the action fails and names the path; see
   [`future/safety.md`](future/safety.md#replacement-and-backups).

A refusal under 4 says which of those kinds it found, because the path alone does
not tell the user whether they are looking at a file to move, a directory to
merge by hand, or a link some other tool installed. A link is named both as it is
written and as it resolves — the two differ exactly when the target is relative,
and the resolved form is the one the refusal was decided on. There is no way to
waive any of this yet; the remedy is to move the destination aside and run the
command again.

Replacing an owned symlink under 3 happens in place rather than through a
temporary sibling. The link carries no content, so a run interrupted partway
through leaves at most a missing link that the next `sync` puts back from the
manifest. The staged-write rule that machine-local state files follow governs
writes that carry content, which is a different case from a node batfiles can
reconstruct.

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

`source` is the link's target and `dest` is the link itself; both follow
[Sources and destinations](#sources-and-destinations). The anchoring matters more
here than elsewhere, because a symlink stores the target it is handed and reads
it back relative to the link's own directory: a target left relative to the
working directory would point somewhere other than where it was meant to.

What happens at the destination depends on what is already there, applying
[Replacing what is already there](#replacing-what-is-already-there):

| Already at the destination                     | Result                                                            |
|------------------------------------------------|-------------------------------------------------------------------|
| nothing                                        | The link is created, along with any missing parent directories.   |
| a symlink already pointing at the source       | Nothing, reported only at `-v`.                                   |
| a symlink pointing elsewhere in the repository | It is repointed at the source.                                    |
| a symlink pointing outside the repository      | An error naming the path and what it found, with nothing written. |
| a regular file                                 | An error naming the path and what it found, with nothing written. |
| a directory                                    | An error naming the path and what it found, with nothing written. |
| anything else                                  | An error naming the path and what it found, with nothing written. |

Where it points, not how it is spelled, is what the three symlink rows mean by
"the source", "elsewhere in the repository", and "outside the repository". The
four error rows are one refusal but not one message, and a repaired link is
repointed regardless of how the stale one was written.

On a platform where batfiles cannot create a symlink, a `symlink` action is an
error naming the action rather than a silent skip or a copy substituted for the
link. The check happens before the destination is examined, so the refusal
arrives without anything already there having been inspected or touched.

Directory mode, which links a directory's children through `source-dir`,
`dest-dir`, and glob filters, is specified in
[`future/repoformat.md`](future/repoformat.md#symlink) and is not built. A
manifest that writes those fields is rejected rather than linking nothing.
