# Local state files

Batfiles keeps machine-local state outside the leaf repository, in documents it
owns and rewrites. Two exist today:

| File            | Default location                                                                                | Classification                   | Regenerable? |
|-----------------|-------------------------------------------------------------------------------------------------|----------------------------------|--------------|
| `disabled.toml` | `$XDG_CONFIG_HOME/batfiles/disabled.toml`, otherwise `<os-home>/.config/batfiles/disabled.toml` | Machine-local user configuration | No           |
| `vars.toml`     | `$XDG_CONFIG_HOME/batfiles/vars.toml`, otherwise `<os-home>/.config/batfiles/vars.toml`         | Machine-local user configuration | No           |

The `dynamic-vars.toml` cache is specified in
[`future/state.md`](future/state.md) and is not built.

The config directory holds the non-regenerable state. It defaults under the
invoking user's OS home and is independent of `--home-dir`, so selecting a
different home moves the repository and the destinations but not these files. The
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

## `vars.toml`: machine-local variables

`vars.toml` stores deliberate, non-regenerable variable values for one machine.
It is maintained by `vars set` and `vars unset`, and it may also be edited by
hand.

Every command that executes actions reads it, as the second of the four layers
[variable precedence](environment.md#variable-precedence) merges: a value stored
here overrides the repository's `[vars]` and is overridden by `BATFILES_VAR_*`
and `--var`. A malformed or unreadable document therefore fails those commands
as a malformed manifest does.

A value stored here is read by every [condition](repoformat.md#conditions) a run
decides, which is how one machine says it wants what a shared repository declares
conditionally. It is also what `vars get` answers, what a run reports at `-vv`,
and what [`vars list`](cmdline.md#vars-list) shows in its place among the layers
— alone, under `--machine-only`, which is the one listing that reads this file
and nothing else. The parts of this document that remain unbuilt are specified in
[`future/state.md`](future/state.md#varstoml-the-parts-that-are-not-built).

### Schema

The whole document is a TOML map from user-variable name to string value:

```toml
editor = "nvim"
profile = "work"
work = "true"
```

- Top-level keys are data, not a fixed set of schema fields. Each key must
  follow the repository format's shared [user-variable name
  rules](repoformat.md#names-and-ids), which are not the rules an ID follows: an
  underscore may start a name and not an ID, a hyphen may appear in an ID and not
  a name.
- A key that breaks that rule fails the whole document rather than just its own
  entry, so a hand-written `has-dash` or a key named `vars` — one of the five
  reserved identifiers — makes the file fail to load. An entry that can never
  become live is not junk worth preserving.
- Every value is a string. A bare `work = true` is invalid rather than coerced,
  the same way it is under a manifest's `[vars]`.
- The document may be empty. Removing the final key leaves a valid empty
  `vars.toml`; batfiles does not delete the file.
- Batfiles writes the map sorted by key, which is the serializer's order rather
  than the order the values were set in.

### Semantics and lifecycle

The three commands that maintain it are specified in
[`vars set`, `vars get`, and `vars unset`](cmdline.md#vars-set); what is here is
the document.

- A key is validated before the file is opened, so an invalid name reads and
  writes nothing — including when the existing document is malformed.
- Any string is a value, the empty string included. `vars get` fails on a key
  with no value rather than reporting an empty one, which would be
  indistinguishable from a key stored as the empty string.
- Setting a key to the value it already holds succeeds without rewriting the
  file. Unsetting an absent key is likewise idempotent: it does not rewrite the
  document, and does not create a `vars.toml` that was not there before.
- Deleting `vars.toml` removes the machine-local values. It does not remove or
  otherwise alter installed home-directory content.

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
