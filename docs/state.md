# Local state files

Machine choices and dynamic captures live in these files. Directory selection
and defaults are defined by [location precedence](environment.md#location-selection).

| File | Directory | Contents | Safe to discard? |
| --- | --- | --- | --- |
| [`disabled.toml`](#disabledtoml-disabled-actions-and-groups) | Config | Persistent action/group choices | Clears choices |
| [`vars.toml`](#varstoml-machine-local-variables) | Config | Machine variable values | Clears values |
| [`dynamic-vars.toml`](#dynamic-varstoml-dynamic-variable-cache) | Cache | Captured command results | Yes; regenerated |
| [`run.lock`](#run-lock) | Cache | Empty advisory-lock file | Only while no run is active |

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

Stores [dynamic-variable](repoformat.md#dynamic-variables) captures for reuse.

### Schema

Leaf keys are variable names; remote keys are `remote:<remote-id>.<name>`, using
the leaf's `[remotes]` ID. All inclusions of a remote share one entry per variable,
regardless of their IDs or overrides.

```toml
[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
```

Each entry has exactly two required string fields: `value` and `captured-at`
(an RFC 3339 timestamp). Unknown fields and TOML datetime values are invalid.

### Freshness and refresh behavior

An entry is fresh while its age is less than the declaration's `cache` duration;
otherwise it is stale. Future timestamps count as age zero.

| Invocation | Cache use |
| --- | --- |
| Action execution or normal `vars list` | Reuse fresh entries; run stale or missing declarations |
| `--refresh-vars` | Run every evaluated declaration regardless of freshness |
| `vars list --no-refresh` | Run/write nothing; report fresh, stale, or missing values |
| `vars refresh` | Force selected declarations; resolve other required leaf values normally |

A successful capture writes its value and completion timestamp. A
[capture failure](environment.md#how-dynamic-commands-are-run) retains and uses
any previous value, fresh or stale, warning with its age. Without a cached
value, the variable has [no value](repoformat.md#dynamic-variables) and warns.

A status command that cannot start is an exception: use `"false"` for this run
regardless of cached contents, warn, and write nothing. Failure to start a
stdout command uses the ordinary fallback.

### When declarations are evaluated

Execution commands resolve all leaf declarations, then those in each opened
remote the leaf [allows](repoformat.md#git). Higher-layer overrides do not
suppress evaluation. Unopened inclusions run nothing; shared remote declarations
resolve once per command. `vars list` resolves only the leaf and skips commands
whose values `vars.toml` overrides.

For `vars refresh`, every leaf declaration is **in play**. A remote is in play
if at least one inclusion naming it would open on this machine:

- Neither the inclusion's address nor its group is persistently disabled.
- Both its own condition and the remote's condition pass.

Run-only skips do not apply. Resolve leaf values first, evaluate those gates
in the [leaf scope](environment.md#variable-precedence), then read admitted
remote manifests and resolve their allowed declarations. A remote out of play
is never read. A malformed admitted manifest fails before its commands run.

Once an inclusion opens, disables and skips do not change declaration behavior.
Dry runs resolve commands and save captures on the same terms.

### Lifecycle

- A missing file is an empty cache. A command with no dynamic declaration to
  resolve neither reads nor creates it; the run lock may still create its directory.
- A malformed file fails the command before any declaration runs, and is left
  untouched. Deleting it is the remedy, and is always safe: the next run
  captures again.
- The document is written only when a command captured something, by the rules
  below. One that cannot be written warns rather than fails; the values captured
  still decide that run.

## Run lock

**Unreleased:** run locking requires a build newer than 0.1.0.

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
too; bootstrap's explicit dry-run exception is described above.
