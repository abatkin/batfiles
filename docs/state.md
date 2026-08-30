# Local state files

Batfiles keeps machine-local state outside the leaf repository, in documents it
owns and rewrites. One exists today:

| File            | Default location                                                                                | Classification                   | Regenerable? |
|-----------------|-------------------------------------------------------------------------------------------------|----------------------------------|--------------|
| `disabled.toml` | `$XDG_CONFIG_HOME/batfiles/disabled.toml`, otherwise `<os-home>/.config/batfiles/disabled.toml` | Machine-local user configuration | No           |

`vars.toml` and the `dynamic-vars.toml` cache are specified in
[`future/state.md`](future/state.md) and are not built.

The config directory holds the non-regenerable state. It defaults under the
invoking user's OS home and is independent of `--home-dir`, so selecting a
different home moves the repository and the destinations but not this file. The
authoritative rules are in [location selection](environment.md#location-selection).

## `disabled.toml`: disabled actions and groups

`disabled.toml` records deliberate, non-regenerable machine-local decisions. It
is maintained by the four enable and disable commands, and it may also be edited
by hand.

`sync` reads it along with the manifest and passes over every action either list
names, alongside the run-only skips that select the same way for one invocation.
The rules a run applies are in
[selecting what a run does](cmdline.md#selecting-what-a-run-does); what is here
is the document.

### Schema

The document is a closed TOML record:

```toml
actions = ["p10k", "zshrc"]
groups = ["shell", "gui"]
```

- `actions` is an array of action [IDs](repoformat.md#names-and-ids).
- `groups` is an array of group names, which follow the same rule.
- These are the only allowed top-level fields. An unknown field is invalid
  configuration.
- Batfiles writes the logical sets without duplicates and in stable order.
- A missing file is treated as an empty disabled set, so a machine that has
  never disabled anything needs no file.
- If a mutation leaves both sets empty, batfiles keeps a canonical empty
  `disabled.toml` rather than deleting it.

A dotted, qualified address such as `core.zshrc` is **not** an ID and is
rejected. Qualified addresses name an action spliced in from an included remote,
and no remote exists yet; they are specified in
[`future/cmdline.md`](future/cmdline.md) and arrive with the remotes that give
them something to refer to.

### Semantics and lifecycle

The enable and disable commands validate each supplied name for syntax only.

- **They never load the leaf repository.** An unreadable or invalid
  `batfiles.toml` therefore cannot fail an enable or a disable, and neither can a
  repository that is not there at all.
- **A name matching nothing is recorded without complaint.** That is the point
  rather than a mistake to warn about: these commands resolve nothing, so a name
  a later branch change or Git update introduces can be disabled ahead of time.
  Since they never read the manifest, they could not tell a typo from a
  pre-registration even if they wanted to. **A `sync` that then finds nothing to
  match is silent too**, though it has the manifest open and could tell:
  pre-registration is what the document is for, so a non-match is the expected
  outcome rather than a complaint. That is the one rule separating these lists
  from a run-only `--skip`, which warns.
- **A malformed entry fails the load**, like any other malformed document. It can
  never become live, so carrying it silently would leave a permanently dead
  entry, and dropping it on the next save would make an unrelated `disable-action`
  destructive. Fixing one means editing the file, which is already a supported
  way to maintain it. It fails a `sync` for a further reason: a run that
  installed everything because it could not read the list of what to leave out
  would be doing the opposite of what the document says.
- **A repeated name warns and is applied once.** It names one thing however many
  times it was written, so the invocation still has an unambiguous meaning.
- **An invalid name fails the whole invocation**, before the document is opened.
  The other names on the command line are not applied: an invocation applies in
  full or changes nothing.
- **Mutations are idempotent.** Adding a name already present, or removing one
  that is absent, is a no-op. It does not rewrite the document merely to sort or
  deduplicate it, and it does not create a `disabled.toml` — or its directory —
  that was not there before.
- **Each outcome says whether the state actually moved**, so re-enabling
  something that really was off is never confused with enabling something that
  already was. Those lines are ordinary status output and are suppressed by
  `--quiet`, which does not suppress the edit itself.

Deleting `disabled.toml` re-enables everything on this machine.

## Writing

Every mutation of a document batfiles owns is a whole-document rewrite:

1. Build and serialize the complete replacement in memory.
2. Write it to a temporary file in the destination directory.
3. Atomically rename the temporary file over the destination.

The rename is the commit point. Concurrent readers see either the complete old
document or the complete new document, never a torn write. If anything fails
before the rename, the temporary file is removed and the destination keeps what
it had.

Consequences of this policy:

- Comments and original key order are not preserved; a rewritten file uses the
  serializer's canonical order.
- Batfiles creates no `.bak` or recovery sidecar files.
- There are no file locks. Two concurrent read-modify-write operations can lose
  a logical update — the last writer wins — although neither publishes a partial
  document.
- Atomicity is not crash durability. Neither the temporary file nor its directory
  is fsynced, so a power loss may lose a just-written document.
- A new file and its parent directories honor the process umask. A replacement
  keeps the permissions of the document it replaces, and acquires them while the
  temporary file is still empty, so a deliberately restricted document is never
  briefly readable through a world-readable sibling.

Reading follows the rules the leaf manifest is read by, specified in
[reading the manifest](repoformat.md#reading-the-manifest), with one difference:
a **missing** state file is an empty document rather than an error. A repository
is a repository because it has a manifest, while a machine that has disabled
nothing has nothing to record.

A dry run does not change any of this. `--dry-run` promises that the plan is not
carried out, not that the process writes nothing anywhere — batfiles' own
bookkeeping is not part of the plan. See
[dry-run behavior](cmdline.md#dry-run-behavior).
