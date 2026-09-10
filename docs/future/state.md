# Local state and cache files that are not built yet

`disabled.toml`, `vars.toml`, and the shared write path are built, and are
specified in [`docs/state.md`](../state.md). Two things stay here: the one
document that does not exist yet, and the parts of the two that do which need
remotes, conditions, or bootstrap to mean anything.

| File                | Default location                                                                                      | Classification                    | Regenerable? |
|---------------------|-------------------------------------------------------------------------------------------------------|-----------------------------------|--------------|
| `dynamic-vars.toml` | `$XDG_CACHE_HOME/batfiles/dynamic-vars.toml`, otherwise `<os-home>/.cache/batfiles/dynamic-vars.toml` | Disposable dynamic-variable cache | Yes          |

## Directory selection

The cache directory independently contains `dynamic-vars.toml`, apart from the
config directory holding the non-regenerable files. Both default under the
invoking user's OS home (`<os-home>`) and are independent of `--home-dir`. The
environment specification defines the authoritative [location selection
rules](environment.md#location-selection).

## `vars.toml`: the parts that are not built

The document, its schema, and the three commands that maintain it are specified
in [`docs/state.md`](../state.md#varstoml-machine-local-variables). Stored values
already contribute the persisted layer of the built [variable
precedence](../environment.md#variable-precedence) and are read by every
[condition](../repoformat.md#conditions) a run decides. One command that reads
them is missing.

**`vars list`**, the one `vars` command that reads more than this file.
`--machine-only` reads only this file and bypasses repository and cache I/O.
Normal `vars list` combines it with leaf `[vars]` and the captured process
environment. `BATFILES_VAR_*` values participate at their normal precedence, and
host environment values are available through the read-only `env.*` namespace.
Remote variables and `--var` are not included.

**`vars refresh`** reads this file, but only as an input to
[reachability](#reachability). It evaluates the `[remotes]` and `include-remote`
gates to decide which remotes are in play; those gates read the leaf scope, and
machine-local values are one of that scope's layers. Reachability has one
implementation for every command, so a `vars refresh` that skipped this file
could refresh a different set of remotes than the `sync` it is meant to prepare
for. The file has no other role here: dynamic command arguments are not
interpolated, and — unlike `vars list` — a machine-local value does not suppress
the refresh of the declaration it shadows.

## `disabled.toml`: the parts that are not built

The document, its schema, and the four commands that maintain it are specified
in [`docs/state.md`](../state.md), including the addresses both lists hold and
the rules a `sync` applies to them. Three things about it are still unbuilt.

**Resolving a qualified address.** An address naming an included remote's action
or group is recorded today and matches nothing, since no remote can contribute
one. What arrives with `include-remote` is the lookup that makes such an entry
live; see the [address forms](cmdline.md#address-forms) that need it.

**What a disable does to a remote.** How a disabled `include-remote` interacts
with materialization belongs to the step that builds it. One part is settled
here already: `disabled.toml` decides which actions are *planned* and never what
is reachable, fetched, or resolved, so disabling an `include-remote` does not
stop that remote being materialized or its variables resolved. See
[reachability](#reachability).

**Bootstrap adoption**, the other writer of this file. It is decided during
`clone` planning and persisted as part of the command, following the environment
specification's [bootstrap adoption
precedence](environment.md#bootstrap-enable-and-disable-lists). Run-only
`BATFILES_SKIP_ACTIONS`, `BATFILES_SKIP_GROUPS`, `--skip-action`, and
`--skip-group` values never persist here.

## `dynamic-vars.toml`: dynamic-variable cache

`dynamic-vars.toml` is disposable cache data derived from dynamic variable
declarations in leaf and included remote `batfiles.toml` files. It is named
differently from `vars.toml` to prevent confusion between regenerable cache and
non-regenerable machine-local configuration.

### Schema

The top-level document is a map keyed by dynamic-declaration cache identity.
Leaf variables use their bare names. Remote variables use
`remote:<remote-id>.<name>`, where `<remote-id>` is the key of the remote in the
leaf repository's `[remotes]` map:

```toml
[work_email]
value = "me@work.com"
captured-at = "2026-06-19T12:00:00Z"

["remote:core.has_op"]
value = "true"
captured-at = "2026-06-19T12:00:00Z"
```

The `remote:` prefix keeps leaf and remote variables in distinct cache
namespaces. The remote ID in this key identifies the declared remote, not the
optional `id` of an `include-remote` action. Every inclusion of the same
declared remote therefore shares one cache entry for a given dynamic
declaration, regardless of the inclusion's ID, action/group selections, or
`vars` overrides. The cache contains the declaration's captured output rather
than an inclusion's final effective value; dynamic commands do not receive the
resolved variable scope in their environment.

Top-level keys are data, not fixed schema fields. Each value is a closed record
with exactly two fields:

| Field         | Type                      | Meaning                                                                                         |
|---------------|---------------------------|-------------------------------------------------------------------------------------------------|
| `value`       | string                    | The captured value. Status captures are stored as the strings `"true"` or `"false"`.            |
| `captured-at` | RFC 3339 timestamp string | When the value was captured, used with the declaration's cache duration to determine freshness. |

Both fields are required by the record shape, and unknown entry fields are
invalid. A remote cache key is quoted in TOML so the dot remains part of one
map key rather than creating nested tables.

### Freshness and refresh behavior

The dynamic declaration supplies the cache duration; it defaults to `1d` when
omitted. Batfiles compares that duration with `captured-at` to classify an
entry as fresh or stale.

The resolver has three cache policies:

- **Auto:** use a fresh entry; run a stale or absent variable's command; write
  a successful capture. This is the default for ordinary plan-building and
  execution commands, including `sync`, `apply-*`, and `vars list`.
- **Force:** run selected dynamic commands even when their entries are fresh;
  write successful captures. Used by `--refresh-vars` and `vars refresh`.
- **Never:** run no commands and write no cache; return a fresh entry, report a
  stale entry as stale, and report an absent entry as missing. Used by
  `vars list --no-refresh`.

For `capture = "stdout"`, a successful zero-status command stores trimmed
stdout. If refresh fails, batfiles retains an available cached value—even a
stale one—and warns; without a cached value, the variable is missing.

For `capture = "status"`, exit status zero produces the string `"true"` and
non-zero produces the string `"false"`. If the command cannot be started, the
runtime result is a transient `"false"`, a warning is emitted, and no new cache
entry is written. The transient `"false"` takes effect even when a cached value
is available, and the existing entry is left untouched: the retain-a-cached-value
rule above applies to a refresh failure, and a command that could not be started
is not one.

A command killed at its `command-timeout` is a refresh failure under both capture
modes, so the transient `"false"` above covers only a command that could not be
started. See [how dynamic commands are run](environment.md#how-dynamic-commands-are-run).

### Reachability

A dynamic declaration is *reachable* when the repository declaring it is in
play. For the leaf that is unconditional; for a remote it is a conditional
question, and this is the definition the rest of this section rests on.

- Every declaration in the leaf repository's `[vars]` is reachable.
- A remote's declarations are reachable when an `include-remote` action selects
  it and **both** gates pass: the action's own `when`/`unless`, and the
  `when`/`unless` on the leaf's `[remotes]` entry for that remote. Either gate
  closing excludes that inclusion, and a remote no surviving inclusion selects
  is not in play at all.
- Both of those records are *leaf* records, so both evaluate against the **leaf
  scope**, which needs nothing but the leaf repository on disk. Only actions
  *inside* an included remote evaluate against that inclusion's scope, and only
  that scope needs the remote materialized. The layering is what keeps
  reachability acyclic: nothing in the leaf scope depends on any remote, and a
  remote-declared variable only ever affects scopes inside its own inclusion.
- An inclusion's own `vars` overrides do not participate in evaluating that
  inclusion's `when`. They are inputs to the scope it creates, not to the
  decision to create it.
- **Declarations resolve before the conditions that read them.** Within a scope
  layer: that layer's dynamic declarations are evaluated, its scope is built,
  and only then are the conditions reading it evaluated. So the leaf's
  declarations run first, the leaf scope decides which remotes are included, and
  only the surviving remotes' declarations run after that.
- A remote excluded by a condition executes none of its dynamic commands and
  writes no cache entries, even with `allow-dynamic-vars = true`.
- **`disabled.toml` never participates.** It decides which actions are planned,
  and never what is reachable, fetched, or resolved. Disabling an
  `include-remote` therefore does not stop that remote being materialized, nor
  its variables from being resolved.

What a condition that *cannot be evaluated* does — it closes its gate, and warns
— is specified with the [condition
grammar](repoformat.md#condition).

### Evaluation and refresh scope

Plan-building and execution commands evaluate every reachable, allowed dynamic
declaration eagerly, including declarations shadowed by higher-precedence
values. Successful captures are cached even though the higher-precedence value
remains effective.
`vars list` is the lazy exception and does not execute a dynamic declaration
shadowed by a machine-local value. A remote with
`allow-dynamic-vars = false` never executes or caches that remote's dynamic
declarations.

`vars refresh` targets the leaf's dynamic variables together with those of every
remote that is in the effective inclusion set, is allowed to run commands, and
is materialized. If neither the leaf nor any such remote declares a dynamic
variable, the command does not load, create, or rewrite the cache file or its
directory.

`--dry-run` does not change dynamic-variable cache policy and may therefore
update this file. The command-line specification defines the shared [dry-run
behavior](../cmdline.md#dry-run-behavior).

Deleting `dynamic-vars.toml` is safe and does not change persisted user
configuration. The next command that needs a dynamic value may rerun its
producer command.

## Shared read and write rules

`dynamic-vars.toml` uses the same state-file write path as the two documents
that are built, which is specified in
[`docs/state.md`](../state.md#writing).

### Reading and validation

The rules that run — parse the whole document before using any of it, treat a
malformed one as fatal and leave it untouched, treat a missing one as empty, and
name the file and the position in the diagnostic — are specified in
[`docs/state.md`](../state.md#writing) and
[`docs/repoformat.md`](../repoformat.md#reading-the-manifest). They apply to the
cache too once anything reads it. What stays here is the ordering between
documents, which needs remotes and dynamic variables to mean anything.

- A manifest is fully parsed and schema-validated before anything in it is used
  — before its values enter input precedence, and before any dynamic command it
  declares is run. The leaf's `batfiles.toml` is therefore parsed and validated
  first, ahead of everything else a command does.
- A remote's manifest is read at the point that remote is known to be in play,
  which is *after* the leaf's dynamic declarations have resolved. The
  [reachability](#reachability) layering makes that ordering unavoidable: the
  leaf scope is what decides which remotes are included, and building it means
  resolving the leaf's declarations. A malformed manifest in an effective remote
  is still fatal and is still rejected before anything in that remote runs, but
  it is reported after the leaf's own commands have already executed. A remote
  no surviving inclusion selects is never read at all.
- An existing malformed file is a fatal configuration-load error. Batfiles
  reports it and leaves the file untouched rather than replacing it.
- Closed records reject unknown fields. Map containers such as `vars.toml` and
  the top level of `dynamic-vars.toml` accept arbitrary valid data keys, while
  any known record stored beneath them remains strict.
- Diagnostics should identify the source file and TOML path when practical.
