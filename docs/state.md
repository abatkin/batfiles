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
actions = ["p10k", "core.zshrc"]
groups = ["shell", "core.gui"]
```

- `actions` is an array of action [addresses](cmdline.md#addresses).
- `groups` is an array of group addresses, which follow the same rule.
- These are the only allowed top-level fields. An unknown field is invalid
  configuration.
- Batfiles writes the logical sets without duplicates and in stable order. That
  order is the address's dotted text, so `a-c` sorts ahead of `a.b`.
- A missing file is treated as an empty disabled set, so a machine that has
  never disabled anything needs no file.
- If a mutation leaves both sets empty, batfiles keeps a canonical empty
  `disabled.toml` rather than deleting it.

A syntactically valid address is persisted as given, whatever its segment count.
These commands resolve nothing, so an address with more segments than any
resolvable [form](cmdline.md#addresses) — `a.b.c.d.e` — is accepted and recorded.
A qualified address names an action or a group spliced in from an included
remote; no remote exists yet, so one recorded today matches nothing, which is the
same outcome as any other name a manifest does not answer to.

### Semantics and lifecycle

The file stores action and group addresses independently. Unknown addresses are
retained without warning, including when synchronization finds no matching
record. Malformed entries fail the load; they are never silently dropped.

Mutations are idempotent. A no-op does not rewrite the file or create a missing
file or parent directory. Deleting `disabled.toml` clears persistent exclusions;
run-only skips can still exclude actions.

[Enable and disable commands](cmdline.md#enable-and-disable-actions-or-groups)
define argument validation and output.
[Selection](cmdline.md#selecting-what-a-run-does) defines when each command
honors these lists and how they combine with run-only skips.

## Writing

Every mutation of a document batfiles owns is a whole-document rewrite:

1. Build and serialize the complete replacement in memory.
2. Exclusively create a temporary file in the destination directory, apply
   existing destination permissions, and write the replacement.
3. Atomically rename the temporary file over the destination.

The rename is the commit point. Concurrent readers see either the complete old
document or the complete new document, never a torn write. If anything fails
before the rename, cleanup attempts to remove only the temporary file created
by that invocation; the destination keeps what it had. An occupied temporary
path, including a symlink, is refused without truncating or removing it.

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
