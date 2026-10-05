# Local state files

Batfiles keeps machine-local state outside the leaf repository, in documents it
owns and rewrites. Three exist:

| File                | Default location                                                                                      | Classification                    | Regenerable? |
|---------------------|-------------------------------------------------------------------------------------------------------|-----------------------------------|--------------|
| `disabled.toml`     | `$XDG_CONFIG_HOME/batfiles/disabled.toml`, otherwise `<os-home>/.config/batfiles/disabled.toml`       | Machine-local user configuration  | No           |
| `vars.toml`         | `$XDG_CONFIG_HOME/batfiles/vars.toml`, otherwise `<os-home>/.config/batfiles/vars.toml`               | Machine-local user configuration  | No           |
| `dynamic-vars.toml` | `$XDG_CACHE_HOME/batfiles/dynamic-vars.toml`, otherwise `<os-home>/.cache/batfiles/dynamic-vars.toml` | Disposable dynamic-variable cache | Yes          |

The config directory holds the non-regenerable state, and the cache directory,
apart from it, the one document that can be deleted at no cost. Both default
under the invoking user's OS home and are independent of `--home-dir`, so
selecting a different home moves the repository and the destinations but not
these files. The authoritative rules are in [location
selection](environment.md#location-selection).

## `disabled.toml`: disabled actions and groups

`disabled.toml` records deliberate, non-regenerable machine-local decisions. It
is maintained by the four enable and disable commands, written once more by the
[bootstrap](#bootstrap-adoption) that sets a machine up, and it may also be
edited by hand.

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
A qualified address names an action or a group an
[`include-remote`](repoformat.md#include-remote) spliced in, or an entry of a
[clone list](repoformat.md#the-clone-list-format); `actions = ["zsh-plugins.p10k"]`
leaves one plugin out. One naming an
inclusion that contributes nothing by that name matches nothing, which is the
same outcome as any other name a manifest does not answer to — and one recorded
before the inclusion existed starts matching when it does.

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

### Bootstrap adoption

The document's other writer. A bootstrap — [`clone`](cmdline.md#clone), or
[`sync --bootstrap`](cmdline.md#sync) — settles this machine's starting point
before its first action, from the leaf's [default-disabled
candidates](repoformat.md#default-disabled-bootstrap-entries), the four
`BATFILES_*` bootstrap lists, and its own enable and disable options, in the
[adoption precedence](environment.md#bootstrap-adoption-precedence) the
environment reference owns. What comes out is persisted here, by the rules
above: the same sets, the same canonical order, and the same idempotence, so a
bootstrap that decides nothing creates no file.

The candidates are offered only where no `disabled.toml` exists at all. Once one
does, this machine has said something of its own, and a later `clone` — of
another repository, or of the same one again — adds only what its variables and
options ask for.

Run-only `BATFILES_SKIP_ACTIONS`, `BATFILES_SKIP_GROUPS`, `--skip-action`, and
`--skip-group` never persist here. They leave something out of one run; these
lists say what the machine starts with.

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
and nothing else. A value stored here also keeps `vars list` from running the
dynamic variable it overrides. [`vars refresh`](cmdline.md#vars-refresh) reads
it only as a layer of the leaf scope that decides [which remotes are in
play](#when-declarations-are-evaluated), and a value stored here does not keep
it from running the declaration the value overrides.

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
  reserved identifiers — makes the file fail to load.
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

## `dynamic-vars.toml`: dynamic-variable cache

`dynamic-vars.toml` holds what [dynamic
variables](repoformat.md#dynamic-variables)' commands captured, so that a run
need not repeat a command whose answer is still fresh. It is named unlike
`vars.toml` so that cache is never mistaken for configuration someone wrote.

### Schema

The document is a map keyed by the declaration a value came from. A leaf
repository's variable uses its bare name; a remote's uses
`remote:<remote-id>.<name>`, where `<remote-id>` is the remote's key in the
leaf's `[remotes]`:

```toml
[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
```

- The `remote:` prefix keeps a leaf's and a remote's variables of one name
  apart. The key is quoted so the dot stays inside one key.
- A remote's key names the declared remote, not an `include-remote`, so every
  inclusion of one remote shares one entry per variable, whatever its `id` or
  `vars`.
- An entry is a closed record of two required fields: `value`, the captured
  string (a status capture is `"true"` or `"false"`), and `captured-at`, an RFC
  3339 timestamp written as a string. An unknown field, or a TOML datetime in
  place of the string, is invalid.

### Freshness and refresh behavior

An entry is **fresh** while less time than its declaration's `cache` (default
`1d`) has passed since `captured-at`, and **stale** after that. A `captured-at`
in the future counts as captured now, so a skewed clock does not make every
command run. How a run treats an entry depends on the command:

- **By default**, a fresh entry is used and a stale or absent one runs its
  command. This is every command that executes actions — `sync`, `clone`,
  `apply-action`, and `apply-group` — and `vars list`.
- **`--refresh-vars`** runs every command, fresh entry or not.
- **`vars list --no-refresh`** runs nothing and writes nothing: it reports a
  fresh entry, a stale one as stale, and an absent one as having no value.
- **`vars refresh`** runs every command it refreshes, fresh entry or not, and
  treats any other leaf declaration it needs as a run does.

A command that succeeds writes its entry, with `captured-at` taken as it
finished. One that fails — by exiting non-zero under `capture = "stdout"`, by
being killed at its `command-timeout`, by leaving its output open past that
timeout, by writing too much or writing something that is not UTF-8 — keeps whatever entry there was, fresh or stale, and the run
uses it, with a warning that says how old it is. With nothing cached, the
variable has [no value](repoformat.md#dynamic-variables) and the warning says
so.

A `capture = "status"` command that cannot be started at all is different: the
run uses `"false"` whether or not a value is cached, warns, and writes nothing,
so installing the missing program is noticed on the next run rather than after
the cache expires. A `capture = "stdout"` command that cannot be started is an
ordinary failure.

### When declarations are evaluated

A command that executes actions evaluates every declaration in the leaf, then
every declaration in each remote an inclusion opens that the leaf
[allows](repoformat.md#git) to run them — including one a higher layer
overrides, so its entry stays current. An inclusion that is not opened runs
nothing. `vars list` resolves the leaf alone, and leaves unrun a declaration a
`vars.toml` value overrides.

`vars refresh` has no selection to open inclusions with, so it refreshes the
declarations **in play** instead. Every declaration in the leaf is in play, and
a remote's are when an `include-remote` names that remote and a run on this
machine would open it:

- Neither the inclusion's address nor its group is in
  [`disabled.toml`](#disabledtoml-disabled-actions-and-groups). Disabling an
  inclusion is how a machine keeps a remote's commands from running, so a
  refresh runs nothing that no `sync` here would. A run-only skip does not
  count: it belongs to one action run.
- The inclusion's own `when` or `unless` passes, and so does the one on the
  leaf's [`[remotes]`](repoformat.md#a-remotes-condition) entry. Either closing
  excludes that inclusion, and a remote no remaining inclusion names is not in
  play.

Both gates are leaf records, decided against the **leaf scope**, which needs
nothing but the leaf repository: nothing in it depends on a remote, and an
inclusion's own `vars` play no part in its gate. So the leaf's declarations
resolve first, the leaf scope decides which remotes are in play, and only then
are those remotes' manifests read and their declarations run, where the leaf
[allows](repoformat.md#git) them. A remote not in play is never read and runs
nothing, whatever its `allow-dynamic-vars`; a malformed manifest in one that is
in play fails the command before anything in that remote runs.

Neither `disabled.toml` nor a run-only skip changes what a declaration does
once its inclusion is opened. [Dry-run](cmdline.md#dry-run-behavior) is not an
exception either: commands run and captures are written in both modes.

### Lifecycle

- A missing file is an empty cache, and a command with no dynamic declaration to
  resolve neither reads nor creates it, nor its directory.
- A malformed file fails the command before any declaration runs, and is left
  untouched. Deleting it is the remedy, and is always safe: the next run
  captures again.
- The document is written only when a command captured something, by the rules
  below. One that cannot be written warns rather than fails; the values captured
  still decide that run.

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
a **missing** state file is an empty document rather than an error.

A dry run does not change any of this. `--dry-run` promises that the plan is not
carried out, not that the process writes nothing anywhere — batfiles' own
bookkeeping is not part of the plan. See
[dry-run behavior](cmdline.md#dry-run-behavior).
