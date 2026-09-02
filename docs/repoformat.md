# Batfiles Repository Format

The part of the repository format that runs today: where the manifest lives, how
it is read, and the six kinds of action it can declare. The rest of the
schema — remotes, variables, conditions, and the other action types — is in
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

There is no format-version field, and every top-level section is optional. Two
sections exist:

```toml
[[actions]]                # ordered list<Action>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]
```

Known records are closed: an unknown key, in the document or in an action, is
invalid. That is what a section from an unbuilt part of the format runs into.
`[remotes]` and `[vars]` are specified in
[`future/repoformat.md`](future/repoformat.md) and are rejected until the code
that reads them exists, so a manifest declaring one fails rather than appearing
to have been understood.

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

User variable names follow a deliberately different rule, which arrives with
variables.

## Sources and destinations

A `source` names a path in a repository, and a `dest` names a path on the
machine. Which of the two an action takes is a property of the action type, and
the tables below say; these rules decide what the field means wherever one
appears.

The two happen at different moments, and the split is worth stating once:

- **What a path may say is settled when the manifest is read.** Every rule below
  about the shape of a written value is decidable from the document alone, with
  no root selected and no filesystem consulted, so a manifest that says
  something batfiles cannot honor is refused whole rather than partway through
  executing it.
- **What a path resolves to is settled when the action runs.** Anchoring to
  absolute paths, and the one rule that genuinely needs a filesystem — whether a
  `source` exists — cannot be answered earlier and are not attempted earlier. A
  selected root may be written relative to wherever batfiles was invoked, and a
  path batfiles stores on disk, such as the target of a symlink, is read back
  relative to its own location rather than to that working directory.

**Composing a path is lexical.** `.` and `..` are cancelled textually, and
batfiles does not canonicalize every component to prove where a path ends up. A
parent component that is itself a symlink is followed by ordinary
operating-system path resolution, because that link is something the user put
there deliberately. `~/.config/nvim` means those components joined to the
selected home, whatever `~/.config` turns out to be.

**Classifying what is already at a path is not.** The two are different
questions, and only the first is about the path batfiles was given. A symlink
already sitting at a destination has a target the operating system reads from
the directory the link is *physically* in — so where `~/bin` is a symlink to
`~/.local/bin`, a link at `~/bin/tool` spelled `../dotfiles/bin/tool` points
into `~/.local/`, not into `~/`. Composing that answer lexically judges the link
against a directory it is not in, which is how a link pointing outside a
repository comes to look like one batfiles owns. So an existing link's target,
and the repository the result is tested against, are both resolved before they
are compared. See [Replacing what is already there](#replacing-what-is-already-there).

This affects only what batfiles *concludes* about a node it found. The path it
writes into a link is still the anchored, lexical one, spelled from the
repository root as selected — a repository chosen as `~/dotfiles` is not
rewritten to some other route to the same directory.

**A `source` is contained by its repository.** It resolves from the repository
that declared it, and the result must stay inside that repository. A source that
names its own starting point is invalid — a leading `/`, a leading `\`, or a
drive letter such as `C:config` — and so is a relative one that climbs out, even
where the outside path exists; an internal `.` or `..` is fine as long as the
result stays in. How far a relative source climbs is a property of the source
itself, so this is decided while the manifest is read, before any repository is
selected. The containment check is lexical, so a symlink deliberately stored
inside the repository may point anywhere and is followed like any other.

The three anchored spellings are named individually because "absolute" does not
cover them on every platform: Windows treats a path as absolute only when it
carries both a drive and a root, so `/etc/hosts` and `C:config` are absolute by
neither that definition nor any useful one, while both still start somewhere
outside the repository. A manifest is meant to be shared between machines, so
the rule is the same everywhere: a `source` is written relative to the
repository root, with `/` separators.

**A `source` names a path *within* the repository, not the repository itself.**
An empty `source` is invalid, and so is one that lands on the repository root —
`.`, `./`, and `shell/..` all do. Such a source would install the whole
repository, `batfiles.toml` and `.git` along with it, which is what a manifest
that lost a value looks like rather than what one asking for that looks like.
The two are reported differently, because an empty field is something left blank
while the others named a real path and need to say which part was meant.

**A `source` must exist.** A repository naming a file it does not contain is a
mistake batfiles can see, and the alternative is installing something that
points at nothing. This is the one path rule that needs a filesystem, so it is
the one checked when the action runs rather than when the manifest is read.

**A `dest` is anchored to the selected home, which is not a boundary.** A `dest`
beginning with `~` uses the selected home rather than an independently discovered
shell home, and `~user` is not expanded and is an error. A relative `dest`
resolves from the selected home, and an absolute one is used as written. Most
destinations sit in the home by convention rather than by rule: `--home-dir`
selects the base for home-relative behavior, it does not create a jail. People
symlink parts of their home onto other volumes, and an explicit absolute or
traversing destination has to keep working.

**An empty `dest` is invalid; write `~` for the home directory itself.** The two
would otherwise mean the same thing, and only one of them says so on purpose. A
`dest` that has gone missing — a field left blank, a value a template never
filled in — looks exactly like the empty one, so batfiles refuses it and names
the spelling that is deliberate.

That and `~user` are the two things a `dest` may not say. There is no
containment rule here to match the one on `source`, because the home is a base
rather than a boundary.

Where the selected home cannot be determined, batfiles reports that rather than
silently substituting the working directory. See
[location selection](environment.md#location-selection).

### Installing into what you install from

**A destination that lands inside the source it installs from is an error**, for
every action that installs anything. `~/dotfiles` is an ordinary place for a
repository and a `dest` may point anywhere, so this is a manifest batfiles
accepts and an action it cannot carry out.

It is refused rather than merely allowed to fail, because for a directory it
does not fail: the destination becomes a child of the source, and the action
then works on what it is writing. `copy` and `copy-dir` descend into the tree
they are producing; `symlink-dir` creates its destination directory, enumerates
the source, finds that directory among the children, and links it into itself.
Every one of them writes into the repository, which `sync` otherwise never does.

Decided by where the two paths resolve rather than by how they are spelled, as
everywhere else, and refused before anything is created — for the `-dir`
actions, before the destination directory that would join the children exists.

The one action that cannot ask this question up front is `symlink` for a single
link, because a link that is already correct *resolves into* its own source: a
converged destination is indistinguishable from an offending one until what is
already there has been inspected. So it is asked at the moment the link would be
written — which is both the case where nothing is there and the case where a
link batfiles may replace is, since repairing writes a link just as creating one
does, and asks before removing what it found rather than after. `copy` and
`copy-dir` ask it per destination for the same reason, after the seed's
occupancy check — see
[Seeds do not replace](#seeds-do-not-replace-and-so-do-not-refuse).

## Replacing what is already there

Creating something where nothing exists is safe. Replacing a node that is already
there is destructive, so an action that installs something at a `dest` inspects
that destination first and decides from what it finds.

**The destination is examined without following a final symlink**, so a link is
judged by where it points rather than by what it reaches. Its target is read as
the operating system would read it, with a relative target resolved from the
directory the link is physically in — which is not always the directory the
destination path names, since any parent may itself be a symlink. A link spelled
`../dotfiles/zshrc` may point exactly where an action wants it to, and one
spelled `<repository>/../elsewhere` leaves the repository despite beginning
inside it; judging the spelling gets both backwards.

Every check in this section is decided in that resolved form, on both sides:
whether the link is already right, and whether it lands inside the repository.
A repository selected by one route and a link resolving through another are the
same repository, and mixing the two forms is what makes an unmanaged link look
owned.

What is found there is one of:

- **Nothing**, in which case the action creates what it was asked to, along with
  any missing parent directories.
- **A symlink batfiles owns** — one whose target resolves inside the selected
  leaf repository. Replacing it destroys nothing: the link holds no content of
  its own, and what it pointed at is left alone.
- **A broken symlink** — one whose target is not there, wherever it names. It
  reaches no content and gives access to none, so replacing it destroys nothing
  either, and where it pointed is not consulted: batfiles will not create the far
  end of a link somebody else made, inside the repository or out.
- **A regular file, a directory, a symlink that leaves the repository and lands
  on something, or none of those** — a socket, a fifo, a device. This is
  someone's data.

The dividing line is whether the node holds content, not who put it there. A link
that leaves the repository is unmanaged when it reaches something, even where its
name is exactly the one an action would install; the same link reaching nothing
is replaceable.

An action decides in this order:

1. Determine what exists, without treating a final symlink as its target.
2. If the requested result is already there, do nothing, and say so only at `-v`.
   This is decided before brokenness, so a repository that deliberately names a
   source which is itself a broken link converges rather than relinking on every
   run.
3. If it is a symlink batfiles owns, or one that is broken, replace it directly.
   Replacing a broken link is reported at normal verbosity rather than at `-v`:
   something was removed, and a broken link may be one the user meant to fix.
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

**A directory an action puts things into is a container, not a destination, and
is judged by a shorter rule.** `symlink-dir`'s `dest-dir` and `create-dir`'s
`dest` are both `mkdir -p`: an existing directory satisfies them, missing parents
come with the one they name, and what is already inside is left alone. A final
symlink *is* followed there, unlike everywhere else in this section — a home
whose `~/.config` is a link onto another volume is an ordinary arrangement, and
the directory the action wants is at the far end of it. Following it is safe
precisely because neither action replaces what it finds; the rules above are for
a node an action installs *over*, and that is the case where following the last
link would judge a node by what it reaches rather than by what it is.

A container still refuses a non-directory — a regular file, a socket, or a
symlink resolving to one — under rule 4 above. A *broken* symlink is not one of
those: it is replaced here on exactly the terms rule 3 replaces one at a
destination, removed and the directory made in its place, and the line saying so
names both the target it held and the path it was at. Nothing else in this
section would have been able to say anything useful about it — `mkdir -p`
reports a bare `EEXIST` naming nothing where a broken link is in the way — and
treating it as a container's own special refusal would have made the same node
mean two different things depending on which action reached it.

**That applies to the missing parents a container brings with it, not only to
the container itself.** A `dest-dir` of `~/a/b/c` creates `~/a` and `~/a/b` on
the way, and each is judged as a container in its own right: a directory
satisfies it, a broken symlink is cleared and reported by the path it was at, and
a regular file is refused and named. The removal lines therefore mention paths
the manifest never wrote, which is the point — a link at `~/a` is where the
problem is, and reporting it against `~/a/b/c` would name the one path in the
chain that does not exist.

**A symlink resolves nowhere whether the path it names is absent or runs through
something that is not a directory.** `<some-file>/child` is broken in exactly the
sense that a link to a deleted file is. A symlink loop is not: it is refused and
named like any other node batfiles cannot account for.

### Seeds do not replace, and so do not refuse

`copy`, `copy-dir`, `fetch-file`, and `fetch-archive` install content the user
then owns, and
they install it **only where nothing is**. That makes an occupied destination their ordinary
steady state rather than an obstruction, so they do not apply the four steps
above at all. A seed asks one question — is anything there? — and where the
answer is yes it keeps what it found and reports it at `-v`.

This is the same rule 4 in different circumstances, not an exception to it. A
`symlink` refuses because it *wants* to write and may not; a seed does not want
to write, because a copy that has been edited since it was installed is the
point of copying rather than a state to converge away from. Nothing is examined
beyond whether something is present: a file, a directory, or a link, broken or
not, all end the question, and who put them there does not matter.

Consequently a seed never reports an error for a destination it found occupied,
never repairs anything, and never removes anything it did not just create. Where
a manifest changes an action from `symlink` to `copy`, the old link stays and the
copy is not made; the `-v` line says so. Re-seeding over content that is already
there is [`--refresh-content`](future/safety.md#seed-actions-and-deletion)'s job
and is not built.

**Nothing is at the destination until the copy is whole.** Every copy, of a
file or of a directory, is built *beside* where it is going and moved into place
in one step at the end. Until then the destination is exactly as it was, which
for a seed means absent. A `fetch-archive` is unpacked the same way, and it
needs one more sibling than the others: the archive has to arrive whole before
it can be read at all, so it is downloaded to
`<destination>.batfiles-download`, checked against its digest there, and
unpacked from there into the staging tree. Both siblings are taken away
afterwards, and both follow every rule below.

That is what a run which does not finish depends on. A half-written file, a
half-filled directory, or an empty placeholder standing in for one would all be
found by the next run, called occupied, kept, and reported as success over a
seed that never finished — a broken state that converges rather than one that
gets noticed. A run that *fails* could tidy that up itself, but a run that is
interrupted cannot, and a copy that is somewhere else until it is complete needs
no tidying to be correct.

Removing what an unfinished run left behind is therefore a separate matter, and
is allowed to fail. Taking back a copied tree needs write permission on every
directory in it, and a copy carries the source's permissions, so a repository
holding a read-only directory produces a copy batfiles cannot remove. What
survives is named `<destination>.batfiles-incomplete` — or
`<destination>.batfiles-download` for an archive that was being fetched — sits
next to where the install was going, and is reported when batfiles is still
running to report it. Nothing ever reads either: they are litter, and deleting
them is safe.

**A leftover stops the next run rather than being cleared.** That path is
somebody's, and "it is probably ours" is not something batfiles acts on —
clearing a directory it did not create is exactly the thing rule 13 forbids, and
a recursive removal of the wrong one takes the whole tree. So a copy whose
staging path is occupied fails, names the path, and says to remove it. Deciding
that is the user's.

**Publishing tries not to replace a destination that appeared meanwhile.** A
copy can take a while, and the destination was checked before it started. A file
is therefore published by a link, which is the one move the standard library
offers that *refuses* to replace what is already there: a destination that
appeared during the copy is kept and reported as kept, exactly as one that was
there from the start.

**Directories are the case this does not close, and it is a gap rather than a
technicality.** There is no portable atomic move that refuses to replace, so
`dest-dir` is checked again immediately before the move — two adjacent
operations, with no way to make them one. A directory created in between is
replaced if it is empty; one holding anything fails the move instead. An empty
directory is still a node somebody made, so this does not meet the standard the
rest of this section is written to. A file falls into the same gap on a
filesystem with no links, where the move is all that is left.

Closing it needs a platform-specific call — `renameat2` on Linux, `renamex_np`
on macOS, neither on the BSDs, and not guaranteed by every filesystem on the two
that have them. **The same gap, considerably wider, is in every other action:**
`symlink` inspects a destination, removes what it finds, and creates the
replacement, which is three steps with a deletion in the middle. Closing it here
alone would buy nothing, so it is recorded rather than patched, and batfiles
does not claim to be safe against another process writing to a destination while
a run is in progress. Nothing in a run takes a lock. Do not run two at once.

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
| `type`  | action-type string |   yes    | Selects the action variant. `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, and `fetch-archive` are the ones that exist. |
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
[Replacing what is already there](#replacing-what-is-already-there):

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
[Installing into what you install from](#installing-into-what-you-install-from).
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
[Replacing what is already there](#replacing-what-is-already-there) — unlike the
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
[Installing into what you install from](#installing-into-what-you-install-from),
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
[Replacing what is already there](#replacing-what-is-already-there). The action
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

**A copy carries the permissions of what it copied**, including whether a file
is executable, and a copied directory arrives with the source directory's
permissions rather than more broadly readable. Ownership is not copied: the copy
belongs to whoever ran the command. Directories created only to *reach* a
destination correspond to nothing in the repository and take the platform
default subject to the umask.

Those permissions are set once the copy is whole, because a source directory its
owner cannot write into would otherwise lock batfiles out of the copy it is
still filling. **So a copy is made closed and opened up at the end, never the
other way round**: while it is being built it is reachable by its owner and
nobody else, whatever the source's mode turns out to be. A copy of a private
file is never briefly a public one — which matters most exactly when a run does
not finish, since what it was building is deliberately left where it is.

**A destination inside the directory being copied is an error**, under the rule
[every install action shares](#installing-into-what-you-install-from). For a
copy the consequence is the worst of the four: the destination would become a
child of the source, enumerating the source would find it, and the copy would
descend into what it was writing until the filesystem refused a longer path.

**Only files and directories are copied.** A symlink found inside a directory
being copied is an error naming it, not something to follow or to recreate:
following it would turn a link the repository chose into a detached file with
nothing said about it, and recreating it would re-read a relative target from a
directory it is no longer in. A socket, a fifo, or a device is an error on the
same terms. The `source` the manifest *named* is not covered by this — like
every other action's source it resolves through a final symlink, because naming
a thing through a link the repository stores is naming that thing.

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

`source` is the one path-shaped field in the format that is not a path. It is
never resolved against a root and follows none of [Sources and
destinations](#sources-and-destinations); `dest` follows all of it.

**A fetched file arrives readable** — mode `0644` on unix — rather than with the
private mode it is written under. It is built closed and widened once complete,
so an interrupted run leaves nothing readable behind, and there is no source on
this machine whose permissions it could carry instead.

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

**An entry that would be written outside `dest` fails the whole action.** Every
entry's path is read before any of them is created, so nothing has been written
when one of these is found, and skipping the entry is not on offer: an archive
carrying one is not an archive to install part of. Four rules, and the last two
exist because the first two are not enough on their own.

- An absolute entry path, and a hardlink naming something outside the tree.
- **A `..` anywhere in an entry path is refused rather than cancelled.**
  Cancelling it on paper says `a/../b` means `b`, which is true only when `a` is
  a real directory — and an archive is free to declare `a` a symlink. No archive
  worth installing writes one, so there is nothing to weigh against refusing it.
- **Nothing is written under a symlink the archive itself declares.** The
  operating system follows a link before it creates what is below it, so an entry
  under one does not land where the archive says it does.
- **A symlink target may climb past a directory and not past a link.** A target
  needs `..` — `../lib/libfoo.so` is ordinary, and so is a link to another link
  — so unlike an entry path it cannot simply be refused. What is refused is the
  one case where cancelling is wrong: a `..` that would cancel a component the
  archive declares as a symlink. That closes an escape no check on a single path
  finds, because it takes two entries to build: `a/b -> ../x` is honest and stays
  inside, and `escape -> a/b/../../outside` cancels on paper to a path inside
  while the kernel resolves `a/b` first and lands beside the destination.

An entry that is neither a file, a directory, nor a link — a device node or a
fifo — is refused on the same terms.

**Unpacked entries carry the archive's permissions, minus the dangerous ones.**
The executable bit comes across, and setuid, setgid, and the sticky bit do not:
what is being installed came from a URL and is going into the home. Directories
take their mode after their contents are written, so an archive that marks a
directory read-only still gets its children. `dest` itself takes the mode of
whatever `archive-root` stripped, and `0755` where the archive names no
directory to take it from.

Filtering the entries — `include` and `exclude` — is specified in
[`future/repoformat.md`](future/repoformat.md#fetch-archive-entry-filters) and
is not built. A manifest that writes one is rejected.

### The transfer both fetching actions share

Everything below governs `fetch-file` and `fetch-archive` alike. Neither the
schemes, the digest, the redirect and timeout rules, nor what counts as an
answer depends on what the body turns out to be.

**A `sha256` is optional because a URL is often a moving target.** The
`fetch-file` example above names a file on a branch, where a pinned digest would
fail on every upstream change. Where a repository does pin one, the bytes are
hashed as they arrive and a mismatch installs nothing, naming both digests so
the manifest can be corrected when the change upstream was the expected one. For
an archive the digest is checked against the archive's own bytes, and it is
checked before a single entry is unpacked.

**Nothing incomplete is ever installed.** What is fetched is built beside its
destination and moved there in one step once it is whole, so a transfer that
stops early, a server that answers with something other than the file, a digest
that does not match, and — for an archive — an entry that cannot be written all
leave the destination as they found it. Not a half-file and not a half-tree: a
later run would find either occupied and mistake it for finished work.

**Only a `200 OK` is a body.** Batfiles asks for neither a byte range nor a
conditional response, so an answer that is neither content nor a refusal — a
`204` with nothing in it, a `206` holding one range, a `304` naming a cache
batfiles does not keep — is an error rather than something to install.
Installing one would occupy the destination with something that is not what was
asked for, which every later run would then find and call done.

Batfiles follows up to five redirects, sends no `Accept-Encoding`, and honors
the usual proxy environment variables. Certificates are checked against the
operating system's trust store, so a corporate CA that the machine already
trusts is trusted here. A server that takes the connection and then says
nothing is given 30 seconds, and a body that stalls is given ten minutes in
total — enough that a large download on a slow link is never the thing that
runs out.

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
