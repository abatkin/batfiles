# Local state files

Machine choices, dynamic captures, and run-action history live in these files. Directory selection
and defaults are defined by [location precedence](environment.md#location-selection).

| File | Directory | Contents | Safe to discard? |
| --- | --- | --- | --- |
| [`disabled.toml`](#disabledtoml-disabled-actions-and-groups) | Config | Persistent action/group choices | Clears choices |
| [`vars.toml`](#varstoml-machine-local-variables) | Config | Machine variable values | Clears values |
| [`dynamic-vars.toml`](#dynamic-varstoml-dynamic-variable-cache) | Cache | Captured command results and release tags | Yes; regenerated |
| [`run.lock`](#run-lock) | Cache | Empty advisory-lock file | Only while no run is active |
| [`runs.toml`](#runstoml-run-action-history) | State | When actions and remotes last changed, and when each stateful `run` action last succeeded | Yes; stateful `run` commands run again and `every` intervals restart |

Documents follow [manifest reading rules](repoformat.md#reading-the-manifest),
except that missing state files are empty documents. Mutations use the shared
[atomic writing policy](#writing).

## `disabled.toml`: disabled actions and groups

Maintained by [enable/disable commands](commands/enable-disable.md#enable-and-disable-actions-or-groups),
bootstrap, or hand edits. [Selection](cmdline.md#selecting-what-a-run-does)
defines how commands use it.

### Schema

```toml
actions = ["p10k", "core.zshrc"]
groups = ["shell", "core.gui"]
```

These are the only fields, each an array of [addresses](cmdline.md#addresses).
Malformed entries fail loading. Syntactically valid addresses are retained
without resolution or warnings, even if no current record can match them.
The two namespaces are independent.

### Semantics and lifecycle

Writes deduplicate and sort each set by dotted address text (`a-c` before `a.b`).
A mutation leaving both sets empty keeps a canonical empty file. A no-op neither
rewrites nor creates the document or its parent directories. Deleting the file
clears persistent exclusions but leaves installed content and run-only skips
unaffected.

### Bootstrap adoption

Before the first action, `clone` and `sync --bootstrap` adopt eligible leaf
[default-disabled candidates](repoformat.md#default-disabled-bootstrap-entries)
and explicit inputs in [precedence order](environment.md#bootstrap-adoption-precedence).

Candidates are offered only if no `disabled.toml` exists, including no empty
one. Explicit environment and CLI choices apply either way. A bootstrap with
no changes creates no file; changed state uses the same sets, ordering, and
writing policy as other mutations. Dry-run bootstrap uses the computed choices
without persisting them.

Run-only skip variables and options never persist here. See
[bootstrap command behavior](commands/clone.md#what-the-bootstrap-decides) for
conditions, reporting, and failures.

## `vars.toml`: machine-local variables

Maintained by [`vars set` and `vars unset`](commands/vars.md#vars-set), or hand edits.
See [variable precedence](environment.md#variable-precedence) for how stored
values combine with other layers.

### Schema

```toml
editor = "nvim"
profile = "work"
work = "true"
```

Keys follow [variable-name syntax](repoformat.md#names-and-ids); all values are
strings, including the empty string. Invalid keys or non-string values fail
the whole document. Writes sort by key.

### Semantics and lifecycle

Setting an identical value or unsetting an absent key succeeds without rewriting
or creating the document or its directory. Removing the last key keeps an empty file. Deleting
`vars.toml` removes machine values without altering installed content.
Argument validation and output belong to the [command reference](commands/vars.md#what-the-three-of-them-share).

## `dynamic-vars.toml`: dynamic-variable cache

Stores [dynamic-variable](repoformat.md#dynamic-variables) captures, of commands
and of `github-release` lookups alike, for reuse.

### Schema

Leaf keys are variable names; remote keys are `remote:<remote-id>.<name>`, using
the leaf's `[remotes]` ID. All inclusions of a remote share one entry per variable,
regardless of their IDs or overrides.

```toml
[fzf_tag]
value = "v0.75.0"
captured-at = "2026-11-20T12:00:00Z"
from = { github-release = "junegunn/fzf", github-url = "https://github.com" }

[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"
from = { command = ["git", "config", "user.email"], capture = "stdout" }

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
from = { command = "command -v op >/dev/null", capture = "status" }
```

| Field | Required | Holds |
| --- | :---: | --- |
| `value` | yes | The captured string |
| `captured-at` | yes | When it was captured, as an RFC 3339 timestamp string |
| `from` | no | The declaration that captured it, as an inline table of its value-determining fields |

`from` holds a command declaration's `command`, spelled as declared (a string or
a list), and its `capture`, written even when it is the default; or a
`github-release` declaration's repository and its resolved `github-url`, with
no trailing `/`, written even when it is the default, never the template as
written. `cache`, `timeout`, and `command-timeout` are left out, since they do
not change the value. A `from` must hold exactly one of those pairs, both
fields of it, and nothing else. Every capture writes `from`; an entry without
one was written before entries recorded their declarations, when only commands
captured, so it serves any command declaration and no `github-release` one.
Unknown fields, in an entry or in `from`, an incomplete or mixed `from`, and
TOML datetime values are invalid.

### Freshness and refresh behavior

An entry belongs to the declaration its `from` records. One whose `from`
differs from its declaration's counts as **absent** for freshness, refresh, and
failure fallback; the next successful capture overwrites it. Editing a
declaration's command, capture mode, repository, or resolved host therefore
captures again at once, and never serves the old declaration's value.

An entry is fresh while its age is less than the declaration's `cache` duration;
otherwise it is stale. Future timestamps count as age zero.

| Invocation | Cache use |
| --- | --- |
| Action execution or normal `vars list` | Reuse fresh entries; capture stale or missing declarations |
| `--refresh-vars` | Capture every evaluated declaration regardless of freshness |
| `vars list --no-refresh` | Capture and write nothing; report fresh, stale, or missing values, and entries cached for a different declaration |
| `vars refresh` | Force selected declarations; resolve other required leaf values normally |

A successful capture writes its value, completion timestamp, and `from`. A
failed capture, a [command's](environment.md#how-dynamic-commands-are-run) or a
[lookup's](environment.md#how-github-release-lookups-are-made), retains and
uses any previous value of the same declaration, fresh or stale, warning with
its age. Without one, the variable has
[no value](repoformat.md#variables-with-no-value) and warns. A `github-url` that
fails to resolve leaves nothing to match an entry against, so the variable
always has no value.

A status command that cannot start is an exception: use `"false"` for this run
regardless of cached contents, warn, and write nothing. Failure to start a
stdout command uses the ordinary fallback.

### When declarations are evaluated

Declarations are captured before any [template](repoformat.md#variables) that
reads them is evaluated, and templates read what they resolved to: a value, or
no value. Execution commands resolve all leaf declarations, then those in each
opened remote: its `github-release` declarations always, and its commands if the leaf
[allows](repoformat.md#git) them. Higher-layer overrides do not suppress
evaluation. Unopened inclusions capture nothing; shared remote declarations
resolve once per command. `vars list` resolves only the leaf and skips
declarations whose values `vars.toml` overrides.

For `vars refresh`, every leaf declaration is **in play**. A remote is in play
if at least one inclusion naming it would open on this machine:

- Neither the inclusion's address nor its group is persistently disabled.
- Both its own condition and the remote's condition pass.

Run-only skips do not apply. Resolve leaf values first, evaluate those gates
in the [leaf scope](environment.md#variable-precedence), then read admitted
remote manifests and resolve their declarations, commands only where allowed.
A remote out of play is never read. A malformed admitted manifest fails before
its commands run.

Once an inclusion opens, disables and skips do not change declaration behavior.
Dry runs run commands, make lookups' network requests, and save captures on the
same terms.

### Lifecycle

- A missing file is an empty cache. A command with no dynamic declaration to
  resolve neither reads nor creates it; the run lock may still create its directory.
- A malformed file fails the command before any declaration runs, and is left
  untouched. Deleting it is the remedy, and is always safe: the next run
  captures again.
- The document is written only when a command captured something, by the rules
  below. One that cannot be written warns rather than fails; the values captured
  still decide that run.

## `runs.toml`: run-action history

Records what [`run` action triggers](actions/run.md#triggers) read: when each
action and remote last [changed](actions/run.md#what-changed-means), and when
each `run` action whose trigger keeps state last succeeded. Only `sync`,
`clone`, and the apply commands read or write it.

### Generations

Each run that writes takes a generation: one more than the `generation` the
document last saved. Changes and successes record the generation of the run
that made them, so they can be ordered without relying on the clock. A run
holds the [run lock](#run-lock) from reading the document to saving it, so no
two writing runs share a generation. Only `every` reads the success's time.

### Schema

```toml
generation = 12

[actions.mise]
last-changed = 12

[actions.mise-install]
last-success = "2026-06-19T12:00:00Z"
success-generation = 12

[actions."corp.rebuild-cache"]
last-changed = 9

[remotes.mise]
last-changed = 12
```

| Key | Type | Meaning |
| --- | --- | --- |
| `generation` | non-negative integer | The generation of the run that last saved the document. Required. |
| `actions` | table of action records | Keyed by action [address](cmdline.md#addresses): the bare ID for a leaf action, the qualified address for a contributed one. |
| `remotes` | table of remote records | Keyed by the leaf's remote ID. A remote and an action of the same name have separate records. |

| Record field | Type | Meaning |
| --- | --- | --- |
| `last-changed` | non-negative integer | The generation of the last run in which the action or remote changed. Absent means never. |
| `last-success` | RFC 3339 string or offset datetime | Actions only: when the command last exited 0. Absent means never. |
| `success-generation` | non-negative integer | Actions only: the generation of that success. Absent means `after` treats the command as never having succeeded. |

Every record field is optional. Unknown fields, keys that are not addresses or
IDs, and datetimes without an offset are invalid.

Only an action or remote with an ID gets a record. A child an inclusion
contributes is recorded under its qualified address, and its change is also
its inclusion's.

### Semantics and lifecycle

- A missing file is an empty document, at generation 0.
- The run reads the document before it materializes remotes. A malformed file
  fails the command there, and is left untouched.
- The run records each change and success in memory as each remote or action
  completes, and saves the document once, when the run ends, whether it
  succeeded or failed after reading it. A failure in materialization, assembly,
  or preparation still saves the changes made before it. A run that recorded
  nothing does not write. A process killed outright loses what its run
  recorded. A dry run records and writes nothing.
- A write that fails warns; it does not change the command's exit status.
  Losing a success can repeat a command; losing a change can leave an `after`
  command unrun until its target next changes.
- A future `last-success` counts as age zero.
- Records whose address or ID matches nothing current are retained, as in
  `disabled.toml`, so a skipped, excluded, or unreached follower still sees the
  changes it has not handled. Renaming an action's ID starts it fresh. Changing
  its command resets nothing.
- Deleting the file breaks nothing: stateful `run` commands run again, since
  none has succeeded, and `every` intervals restart.

### The earlier layout

A document without a top-level `generation` value is read in the earlier
layout, which keyed `run` actions directly by address:

```toml
[mise-install]
last-success = "2026-06-19T12:00:00Z"

["corp.rebuild-cache"]
last-success = "2026-06-18T08:00:00Z"
pending = true
```

It is read as generation 0. Its successes keep their times, and count as
generation 0 for `after`, which has no change older than them. An entry with
`pending = true` keeps its time but no generation, so an `after` command runs
once more. The next save writes the current layout.

## Run lock

An exclusive advisory lock on `<cache-dir>/run.lock`, held for the command's
whole run, prevents overlapping state operations by commands sharing that cache.

| Takes the lock | Does not take it |
| --- | --- |
| `sync`, apply commands, `clone`, enable/disable commands, `vars set`, `unset`, `refresh`, normal `list` | `vars get`, `list --machine-only`, `list --no-refresh`, `init`, `version`, `update` |

Dry runs take it too. The lock is acquired after resolving roots and before
reading state, creating the directory and file if needed. `clone` does so after
cloning and before bootstrap, allowing a cache inside the new repository.

There is no wait: contention, or failure to create/open/lock the file, exits 1
naming the path before state is read or changed. Any repository already cloned
is kept. The file stays empty and is never removed by batfiles; process exit
releases the lock, so a leftover file blocks nothing.

Different cache directories do not exclude each other, even with shared config.
Editors and other programs are not excluded. Delete the file only when no run
is active.

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
- The documents themselves are not locked. The [run lock](#run-lock) keeps two
  batfiles runs sharing a cache directory from interleaving, but a
  read-modify-write that races any other writer can lose a logical update — the
  last writer wins — although neither publishes a partial document.
- Atomicity is not crash durability. Neither the temporary file nor its directory
  is fsynced, so a power loss may lose a just-written document.
- A new file and its parent directories honor the process umask. A replacement
  keeps the permissions of the document it replaces, and acquires them while the
  temporary file is still empty, so a deliberately restricted document is never
  briefly readable through a world-readable sibling.

Bookkeeping writes follow this policy in [dry runs](cmdline.md#dry-run-behavior)
too; the dry-run exceptions for bootstrap and `runs.toml` are described above.
