# Batfiles Repository Format

The part of the repository format that runs today: where the manifest lives, how
it is read, the static variables it can declare, and the nine kinds of action it
can declare. The rest of the schema — remotes, conditions, dynamic variables,
and the other action types — is in
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
`--batfiles-dir` or `BATFILES_DIR`, then by a `batfiles.toml` in the current
directory, and finally by `<selected-home>/dotfiles`; see
[location selection](environment.md#location-selection). A remote repository may
carry its own `batfiles.toml`, and nothing reads one yet.

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

- The document is read and checked whole before anything in it is used. A
  command that cannot make sense of its manifest stops before it has done any
  work.
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

There is no format-version field, and every top-level section is optional. Three
sections exist:

```toml
[[actions]]                # ordered list<Action>

[vars]                     # map<variable name, string>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]
```

Known records are closed: an unknown key, in the document or in an action, is
invalid. That is what a section from an unbuilt part of the format runs into.
`[remotes]` is specified in [`future/repoformat.md`](future/repoformat.md) and is
rejected until the code that reads it exists, so a manifest declaring one fails
rather than appearing to have been understood.

## Names and IDs

```text
ID = string matching [A-Za-z0-9][A-Za-z0-9_-]*
```

- IDs and group names match that rule. In particular an ID cannot contain
  whitespace, `.`, or `,`: dots compose
  [addresses](cmdline.md#addresses), and commas delimit environment lists.
- Action IDs are unique within a repository. A repeated one is a load error
  naming both actions, because an address matching two of them could not say
  which was meant.
- Group names and action IDs occupy distinct namespaces.

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
  keeps a [condition](future/repoformat.md#condition)'s namespace lookup
  unambiguous without a precedence rule: no variable can shadow a namespace.
- Names are case-sensitive, so `editor` and `EDITOR` are two variables.

## Sources and destinations

Repository-local `source` and `source-dir` values are nonempty relative paths
within the declaring repository. They may contain `.` and `..` only if they
remain strictly inside it; `.`, `./`, and `shell/..` cannot name the whole
repository. Use `/` separators. Anchored paths and drive prefixes are rejected
according to the host's path syntax. A symlink stored inside the repository may
point outside it.

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

Fetching and Git actions have their own `source` syntax in the action tables.

## Actions

`[[actions]]` is an ordered list. Each entry is a closed record selected by its
required `type` field.

| Field   | Type               | Required | Description                                                            |
|---------|--------------------|:--------:|------------------------------------------------------------------------|
| `type`  | action-type string |   yes    | Selects the action variant. `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, `fetch-archive`, `git-clone`, and `git-clone-list` are the ones that exist. |
| `id`    | `ID`               |    no    | Makes the action addressable.                                          |
| `group` | `ID`               |    no    | Places the action in one group. See [groups](#groups).                 |

Each variant's record is closed independently, so a field belonging to another
variant is an unknown field rather than one that is quietly ignored. Writing
`source-dir` on a `symlink` is an error, so is writing `source` on a
`symlink-dir`, so is writing either on a `create-dir`, which installs nothing
and therefore has no source at all, and so is writing `dot-prefix` anywhere but
on the two actions that install a directory's children.

**The action types come in pairs, and the pairing is not about the source
type.** `symlink` and `copy` install **one thing at one name**, and that thing
may be a file or a directory. `symlink-dir` and `copy-dir` install **each direct
child of a directory, into a directory**. The `-dir` suffix says what is done
with the source's contents — enumerate them — rather than what the source is.
Choosing between the two members of a pair is the author's, and it is not
inferred from what happens to be on disk.

**A choice between two shapes is a `type`, never a boolean.** No action record
carries a field whose value decides which of its other fields mean anything.
That is what makes each record closed in the way the paragraph above promises:
a field that is meaningful only in one of two modes is accepted and ignored in
the other, which is precisely the silent misreading the format is written to
avoid. So `symlink-dir` is an action type rather than `symlink` with a
`children` flag, and unpacking a downloaded archive is
[`fetch-archive`](#fetch-archive) rather than `fetch-file` with an `extract`
flag — a repository asking for the wrong one gets an error naming the field, at
the moment the manifest is read. There is no `extract` field in the format at
all, on either action.

`dot-prefix` is the format's only boolean and is not an exception: it changes
what an installed child is called, and no other field's meaning turns on it.

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

**A group is a way of leaving several actions out at once.** `sync --skip-group`
and `BATFILES_SKIP_GROUPS` pass over every action naming it for one run, and
`disable-group` records the name in
[`disabled.toml`](state.md#disabledtoml-disabled-actions-and-groups), which
every later run honors until an `enable-group` removes it. What a run does with
the two is specified in
[selecting what a run does](cmdline.md#selecting-what-a-run-does).

Because a group is only the actions that name it, that selection reaches exactly
those: an action written with no `group` cannot be left out by group, and an
action written with no `id` can be left out *only* by group — and reached, by
anything naming a single record, only through its group.

**A group is also a way of applying several actions at once.**
[`apply-group`](cmdline.md#apply-group) carries out the actions naming it and no
others, in declaration order. Since a group is only its members, one no action
names does not exist, and applying it is a failure rather than a run with nothing
to do.

The field is also read by reporting: `sync -v` names the group each action
belongs to as it reaches it.

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
| `source` | string |   yes    | The source, relative to the repository root. Never empty.          |
| `dest`   | string |   yes    | The destination path, as written. Never empty; `~` is the home.    |

`source` is the link's target and `dest` is the link itself; both follow
[Sources and destinations](#sources-and-destinations). The anchoring matters more
here than elsewhere, because a symlink stores the target it is handed and reads
it back relative to the link's own directory: a target left relative to the
working directory would point somewhere other than where it was meant to.

What happens at the destination depends on what is already there, applying
[Replacing what is already there](safety.md#replacing-what-is-already-there):

| Already at the destination                          | Result                                                            |
|-----------------------------------------------------|-------------------------------------------------------------------|
| nothing                                             | The link is created, along with any missing parent directories.   |
| a symlink already pointing at the source            | Nothing, reported only at `-v`.                                   |
| a symlink pointing elsewhere in the repository      | It is repointed at the source.                                    |
| a symlink whose target is not there                 | It is repointed at the source, and the target it held is named.   |
| a symlink pointing outside the repository, and there | An error naming the path and what it found, with nothing written. |
| a regular file                                      | An error naming the path and what it found, with nothing written. |
| a directory                                         | An error naming the path and what it found, with nothing written. |
| anything else                                       | An error naming the path and what it found, with nothing written. |

Where it points, not how it is spelled, is what the four symlink rows mean by
"the source", "elsewhere in the repository", and "outside the repository" — and
the fourth row does not read where it points at all, only whether anything is
there. The three error rows are one refusal but not one message, and a repaired
link is repointed regardless of how the stale one was written.

A `dest` landing inside the `source` is an error, under
[Installing into what you install from](safety.md#installing-into-what-you-install-from).
Unlike the other actions this one is decided at the moment the link would be
written rather than up front, because a link that is already correct resolves
into its own source and would otherwise be refused on every run. Repointing a
replaceable link counts as writing one, and is refused before the link it found
is removed.

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
| `source-dir` | string  |   yes    | The directory whose direct children are linked, relative to the root.     |
| `dest-dir`   | string  |   yes    | The directory the links are made in. Created if it is missing.            |
| `dot-prefix` | boolean |    no    | Prefix each installed name with `.`. Defaults to `false`.                 |

This is the action for a directory whose contents you do not want to enumerate.
Adding a file to `source-dir` installs it on the next `sync` with no change to
the manifest, which is the whole reason it exists rather than one `symlink` per
file.

`source-dir` follows the [`source` rules](#sources-and-destinations) and
`dest-dir` the `dest` rules; both are decided while the manifest is read. A
`source-dir` naming the repository root is refused by the rule that already
covers it, which matters more here than for `symlink` — it would link
`batfiles.toml` and `.git` into the home rather than install one of them.

**One link per direct child, whatever the child is.** Nothing descends. A child
that is itself a directory becomes a single symlink to that directory, exactly
as a directory `source` does for `symlink`, and what is under it is reached
through that one link. So `files/config/` holding `a` and `b` installs as one
link at `~/.config`, not as a directory holding two.

**The children are linked in sorted order**, because the order a filesystem
happens to hold them in is not one anybody can diff.

**`dest-dir` is a container, not a destination**, so it is created where it is
missing and followed where it is a symlink, under
[Replacing what is already there](safety.md#replacing-what-is-already-there) — unlike the
destination of each individual link, which is judged without following one. It
may hold entries batfiles did not put there, and those are left alone. Creating
it is reported, because a directory that appeared in the home is worth a line
whichever action made it.

Each child's own destination is then decided by that same section, one at a
time. A child link batfiles owns is repaired; anything else stops the action
where it stands, so the children before it stay installed and the ones after it
are not attempted. That is what stopping at the first failure already means
across a manifest, applied within one action.

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

A `dest-dir` landing inside the `source-dir` is an error, under
[Installing into what you install from](safety.md#installing-into-what-you-install-from),
and is refused before the destination directory is created — creating it is what
would put it among the children about to be linked.

The platform rule is `symlink`'s: where batfiles cannot create a symlink the
action is refused by name, before the source directory is read.

Filtering the children — `include` and `exclude` — is specified in
[`future/repoformat.md`](future/repoformat.md#symlink-dir) and is not built. A
manifest that writes either is rejected rather than linking every child while
looking as though it had linked a chosen few.

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

`dest` follows [Sources and destinations](#sources-and-destinations), and is a
container rather than a destination under
[Replacing what is already there](safety.md#replacing-what-is-already-there). The action
is `mkdir -p`:

| Already at the destination            | Result                                                            |
|---------------------------------------|-------------------------------------------------------------------|
| nothing                               | The directory is created, along with any missing parents.         |
| a directory                           | Nothing, reported only at `-v`.                                   |
| a symlink to a directory              | Nothing, reported only at `-v`; the link is left as it is.        |
| a symlink whose target is not there   | The link is removed and the directory made in its place, naming the target it held. |
| a regular file                        | An error naming the path and what it found, with nothing written. |
| anything else                         | An error naming the path and what it found, with nothing written. |

A directory that is already there is left exactly as it is, contents and all:
the action creates a directory, it does not own one. The only node it removes is
a broken symlink, which holds nothing to keep; that aside it removes nothing,
which is why a symlink to a directory satisfies it where the same node at a
`symlink`'s destination would be refused.

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
| `source` | string |   yes    | The file or directory to copy, relative to the repository root. |
| `dest`   | string |   yes    | Where the copy goes, exactly. Never empty; `~` is the home.     |

The same two fields as [`symlink`](#symlink), installing the same thing in the
same place, and the difference is what the user gets: a detached copy that
editing does not write back into the repository and that a later `sync` will not
undo. This is the action for a file whose *initial* contents a repository wants
to supply — a machine-local override, a template to fill in.

`source` and `dest` follow [Sources and destinations](#sources-and-destinations),
and `dest` is a destination rather than a container, so it is examined without
following a final symlink. Missing parent directories are created.

**A directory source is installed whole.** The destination decides once, for the
action as a whole:

| Already at the destination | Result                                                             |
|----------------------------|----------------------------------------------------------------------|
| nothing                    | The copy is made, with any missing parents. A directory source is reproduced to the bottom. |
| anything at all            | Nothing at all, reported only at `-v`.                             |

So `copy` over an existing directory does nothing — it does not seed into it.
Filling in around what someone already has is [`copy-dir`](#copy-dir), and
choosing between them is the whole of the difference between the two.

Copies preserve source permissions under the shared [permission and staging
rules](safety.md#installed-permissions). A destination inside the source
directory is refused under the [containment rule](safety.md#installing-into-what-you-install-from).

Only regular files and directories are copied. Symlinks, sockets, FIFOs, and
devices nested in a copied tree are errors naming the offending path. The
manifest's source itself may resolve through a symlink.

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
| `source-dir` | string  |   yes    | The directory whose direct children are copied, relative to the root. |
| `dest-dir`   | string  |   yes    | The directory the copies are made in. Created if it is missing.       |
| `dot-prefix` | boolean |    no    | Prefix each installed name with `.`. Defaults to `false`.             |

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

Filtering the children — `include` and `exclude` — is specified in
[`future/repoformat.md`](future/repoformat.md#copy) for both `copy` and
`copy-dir` and is not built on either. A manifest that writes one is rejected.

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
| `source` | string |   yes    | An `http://` or `https://` URL. Not a repository path.               |
| `dest`   | string |   yes    | Where the file goes, exactly. Never empty; `~` is the home.          |
| `sha256` | string |    no    | 64 hexadecimal digits: the digest the fetched bytes must have.       |

**The same bargain as [`copy`](#copy)**, with the content coming from a URL
rather than from the repository: the destination decides, missing parents are
created, and what lands is the user's from then on. Anything at all at `dest`
means the action is done — and the check comes first, so a destination that is
occupied costs no transfer.

`source` is a URL and is never resolved against a filesystem root. `dest` follows
[source and destination syntax](#sources-and-destinations). Fetched files use the
shared [staging](safety.md#staging-and-publication) and
[permission rules](safety.md#installed-permissions).

**What arrives is installed as a file, whatever it holds.** A `fetch-file` whose
URL names a tarball installs the tarball. Unpacking one is
[`fetch-archive`](#fetch-archive), a separate action type; `archive-root` is its
field and is unknown here, so a manifest that writes it on a `fetch-file` is
rejected rather than fetching an archive it would not unpack. A `file://`
source is rejected on the same terms.

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
| `source`       | string |   yes    | An `http://` or `https://` URL. Not a repository path.                 |
| `dest`         | string |   yes    | Where the unpacked directory goes, exactly. Never empty; `~` is the home. |
| `sha256`       | string |    no    | 64 hexadecimal digits: the digest the fetched archive must have.       |
| `archive-root` | string |    no    | A prefix every entry is written without, spelled as an entry path is, or `*` for the archive's single top-level directory. |

The sibling of [`fetch-file`](#fetch-file), and [the
transfer](#the-transfer-both-fetching-actions-share) is the same one. What
differs is only what is done with the body.

**`dest` is one name, not a merge root.** The unpacked tree is installed as a
single thing, exactly the way [`copy`](#copy) installs a directory, so anything
at all at `dest` means the action is done and no request is made — including a
directory an earlier `create-dir` left there. A manifest declaring both is
asking for the directory twice.

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

Filtering the entries — `include` and `exclude` — is specified in
[`future/repoformat.md`](future/repoformat.md#fetch-archive-entry-filters) and
is not built. A manifest that writes one is rejected.

### The transfer both fetching actions share

Both fetching actions use these rules:

- HTTP and HTTPS URLs; up to five redirects.
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

The conservative rules below still apply in front of all of it: a worktree with
uncommitted changes is never touched, and a declared branch holding commits the
remote does not is warned about rather than reset. Changing which branch a clone
is on is reported — `switched <dest> to <ref>` — and deletes nothing: the branch
you were on, and its commits, stay where they are.

[Git updates](safety.md#git-updates) specifies dirty-worktree checks,
fast-forwarding, local-file protection, and failure handling.
[Clone validation](safety.md#clone-validation) specifies which existing
checkouts can be updated. Existing clones retain their configured remotes.

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
| `source`   | string |   yes    | The list, relative to the repository root. Never empty.            |
| `dest-dir` | string |   yes    | The directory the clones are made in. Never empty; `~` is the home. |

[`git-clone`](#git-clone) repeated over a file, and the file is the point: a
plugin directory is kept by pasting a URL onto the end of a list, and a format
asking for a whole TOML record per repository would be a worse version of the
file it replaces. What the list may say is the [clone list
format](#the-clone-list-format) below.

Both fields are ordinary. `source` names a file in the repository and follows
[Sources and destinations](#sources-and-destinations) like any other — unlike
`git-clone`'s `source`, which names something off this machine; here that is the
list's job, line by line. `dest-dir` is spelled as it is because this action
installs *into* a directory rather than at a name, exactly as `symlink-dir` and
`copy-dir` do, and each entry contributes one child of it.

**The list is read as the repository is loaded, before any action runs.** It is
a file in the repository, on disk and readable at that point, so the rule the
manifest itself follows extends to it: a document batfiles cannot make sense of
stops the run before it has done any work rather than partway through. One
malformed line is caught while your home is still untouched, which is most of
what a list buys over the same repositories spread through the manifest. The
cost is worth stating: a list *produced* by an earlier action in the same run is
not a list this can read. A list belonging to an action the run passes over —
disabled, or skipped for this run — is not read at all, for the same reason such
an action's `source` need not exist.

**The directory is made before the first entry**, so a list that declares no
repositories still leaves the place they would go — a plugin directory a shell
reads is worth having whether or not anything is in it yet. The entries are then
cloned in list order, each into one child of it, and what a clone does and what
it refuses is [`git-clone`](#git-clone)'s and does not differ here. An entry's
`ref` follows [the same rules](#ref-following-one-branch-tag-or-commit).

#### One entry that fails costs that entry

A list is many repositories, and one of them being unreachable is not a reason
to abandon the rest. So an entry that cannot be cloned is **warned about, and
the entries after it are still installed**:

- a destination holding something batfiles did not put there — a file, a
  directory that is not a clone, a symlink to a checkout elsewhere, or what an
  interrupted clone left;
- a `git` that ran and failed: the clone, the fetch, the fast-forward, or a
  `ref` that resolves to nothing.

The warning names the repository as the list writes it, the list and the line it
is on, and the entry's `id` where it has one, since that is what you have to open
and edit.

**An entry that failed on its `ref` keeps the clone it made.** A `ref` is
resolved after the fetch, so a repository whose `ref` names nothing is already
at its destination by the time the entry fails, sitting on whatever the clone
came down on. It is left there: every later run finds it, tries the `ref` again,
and warns again, so the state is one you are told about on every `sync` rather
than one a run passes over as installed. Fixing the `ref` moves the clone onto
it; removing the entry leaves the directory to you.

What is not survivable is a `git` that could not be run at all, a file that could
not be read or written, and a `dest-dir` that could not be created: none of those
is about one repository, and every entry after would fail the same way.

**So a `sync` that exits 0 may still have entries that did not clone.** The
warnings are the only thing that says so, and they are warnings rather than notes
so that `--quiet` does not take them away.

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

| Key         | Type   | Description                                                |
|-------------|--------|-------------------------------------------------------------|
| `id`        | `ID`   | Makes the entry addressable as `<action>.<entry>`.          |
| `ref`       | string | The branch, tag, or commit to follow.                       |
| `dest-name` | string | What to call the clone, in place of the derived name.       |

`when` and `unless` are specified for an entry and are refused today, naming the
step they arrive at, rather than accepted and never consulted. So is any other
key: a misspelled one is an error, and so is writing a key twice, writing
`key=` with no value, and writing a field after the repository that is not
`key=value` at all — most often a second repository on the same line.

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

`[vars]` is a map from a [variable name](#names-and-ids) to a string:

```toml
[vars]
work = "false"
profile = "personal"
rank = "3"
```

**Variables exist only to feed conditions.** A variable is read by a `when` or an
`unless` and nowhere else: no field of any action interpolates one, and the
format has no interpolation syntax at all. That is why a repository can declare
them long before batfiles can act on them.

**Every value is a string, and only a string.** Booleans, integers, floats,
dates, and arrays are not variable values, so `work = true` and `rank = 3` are
errors naming the line they are written on rather than values converted to
`"true"` and `"3"`. Write the string. An empty value is a legitimate one. A
*table* is not a value either: it is a dynamic-variable declaration, specified in
[`future/repoformat.md`](future/repoformat.md#dynamic-variable-record) and
rejected on the same terms until batfiles can run one.

**Nothing reads the section yet, so declaring it changes no run.** Batfiles
accepts it and checks it as the manifest is read; what consults a variable is a
condition, and conditions are specified in
[`future/repoformat.md`](future/repoformat.md#condition). Until they arrive, a
`sync` over a manifest declaring variables installs exactly what it would have
installed without them.

What is checked is the name and the type of the value, both while the document is
being read. The other three layers that can set a variable — `vars.toml`,
`BATFILES_VAR_*`, and `--var` — are not built, so a name declared here is the
only kind there is today.

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

**Nothing reads the section yet, so declaring it changes no run.** Batfiles
accepts it and checks it as the manifest is read; adopting the candidates
belongs to the bootstrap that sets a machine up for the first time, and is
specified in [`future/repoformat.md`](future/repoformat.md) along with the
enable and disable options that take precedence over them. Until that arrives,
a `sync` over a manifest declaring candidates installs exactly what it would
have installed without them, and creates no `disabled.toml`.

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
against either way. Two fields the full format gives these records are not
built: `when` and `unless` arrive with
[conditions](future/repoformat.md#condition), and are rejected meanwhile by the
records being closed.
