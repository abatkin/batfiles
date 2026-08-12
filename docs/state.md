# Local state and cache files

Batfiles defines three local TOML documents outside the leaf repository:

| File                | Default location                                                                                      | Classification                    | Regenerable? |
|---------------------|-------------------------------------------------------------------------------------------------------|-----------------------------------|--------------|
| `vars.toml`         | `$XDG_CONFIG_HOME/batfiles/vars.toml`, otherwise `<os-home>/.config/batfiles/vars.toml`               | Machine-local user configuration  | No           |
| `disabled.toml`     | `$XDG_CONFIG_HOME/batfiles/disabled.toml`, otherwise `<os-home>/.config/batfiles/disabled.toml`       | Machine-local user configuration  | No           |
| `dynamic-vars.toml` | `$XDG_CACHE_HOME/batfiles/dynamic-vars.toml`, otherwise `<os-home>/.cache/batfiles/dynamic-vars.toml` | Disposable dynamic-variable cache | Yes          |

## Directory selection

The config directory contains the two non-regenerable files, while the cache
directory independently contains `dynamic-vars.toml`. Both default under the
invoking user's OS home (`<os-home>`) and are independent of `--home-dir`. The
environment specification defines the authoritative [location selection
rules](environment.md#location-selection).

## `vars.toml`: machine-local variables

`vars.toml` stores deliberate, non-regenerable variable overrides for one
machine. It may be edited by hand or through `batfiles vars set` and
`batfiles vars unset`.

### Schema

The whole document is a TOML map from user-variable name to string value:

```toml
editor = "nvim"
profile = "work"
work = "true"
```

Top-level keys are data, not a fixed set of schema fields. Each key must follow
the repository format's shared [user-variable name
rules](repoformat.md#names-and-ids), and every stored value follows its
[string-valued variable model](repoformat.md#string-valued-variables).

The document may be empty. Removing the final key leaves a valid empty
`vars.toml`; batfiles does not delete the file.

### Semantics and lifecycle

Machine-local values contribute the persisted layer of the authoritative
[runtime variable precedence](environment.md#runtime-variable-precedence).

- `vars set` validates the key before filesystem access. Setting a key to its
  existing value succeeds without rewriting the file. Any string is a value,
  the empty string included.
- `vars get` reads only the persisted string. It does not resolve repository
  defaults, environment inputs, dynamic cache entries, or one-shot values.
- `vars get` fails when the key has no persisted value, rather than reporting an
  empty value, which would be indistinguishable from a key stored as the empty
  string.
- `vars unset` is idempotent. An absent key does not cause a rewrite, and does
  not create a `vars.toml` that was not there before.
- `vars list --machine-only` reads only this file and bypasses repository and
  cache I/O.
- Normal `vars list` combines this file with leaf `[vars]` and the captured
  process environment. `BATFILES_VAR_*` values participate at their normal
  precedence, and host environment values are available through the read-only
  `env.*` namespace. Remote variables and `--var` are not included.
- `vars refresh` does not read this file because dynamic command arguments are
  not interpolated and the command evaluates no conditions.

Deleting `vars.toml` removes machine-local overrides. It does not remove or
otherwise alter installed home-directory content.

## `disabled.toml`: disabled actions and groups

`disabled.toml` records deliberate, non-regenerable machine-local decisions.
It is maintained by the action/group enable and disable commands and by
bootstrap adoption, and it may also be edited by hand.

### Schema

The document is a closed TOML record:

```toml
actions = ["p10k", "core.zshrc"]
groups = ["work", "core.shell"]
```

- `actions` is an array of action or addressable-child addresses.
- `groups` is an array of group addresses.
- These are the only allowed top-level fields. An unknown field is invalid
  configuration.
- Batfiles writes the logical sets without duplicates and in stable order.
- A missing file is treated as an empty disabled set.
- If a mutation leaves both sets empty, batfiles keeps a canonical empty
  `disabled.toml` rather than deleting it.

### Semantics and lifecycle

Explicit enable and disable commands validate the supplied address for syntax
only. Each dot-separated address segment must match the repository format's
[shared ID syntax](repoformat.md#names-and-ids); the CLI specification defines
the available [address forms](cmdline.md#address-forms). A syntactically valid
address is persisted as given.

Syntax here means only that the address is a nonempty dot-separated list of
valid IDs; the segment count is not constrained. The address forms are the
shapes the current repository model can resolve, and these commands resolve
nothing, so an address with more segments than any listed form is accepted and
recorded.

Address syntax is also validated when the file is read. A `disabled.toml`
containing a malformed address does not load, like any other malformed document
under this specification. Pre-registering an address that matches nothing is
supported and expected; an address that can never match anything, because it is
not an address at all, is a mistake and is reported rather than carried
silently or silently dropped by a later write.

These commands do not load the leaf repository at all, so they perform no
semantic validation and emit no unknown-name warning. An unreadable or invalid
leaf `batfiles.toml` therefore cannot fail an enable or disable. This is
deliberate: a syntactically valid future action, group, inclusion, or
manifest-entry address may be pre-enabled or pre-disabled before a later branch
change or Git update introduces it, and manifest entries are not known until
their action executes.

Bootstrap adoption is the other writer of this file. It is decided during
`clone` planning and persisted as part of the command.

Mutations are idempotent. Adding an existing item or removing an absent item is
a no-op and does not rewrite the document merely to sort or deduplicate it.

Bootstrap decisions follow the environment specification's [bootstrap adoption
precedence](environment.md#bootstrap-enable-and-disable-lists) and persist in
this file. Run-only `BATFILES_SKIP_ACTIONS`, `BATFILES_SKIP_GROUPS`,
`--skip-action`, and `--skip-group` values never do.

Deleting `disabled.toml` re-enables all actions and groups on the next run.

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

Plan-building and execution commands evaluate every reachable, allowed dynamic
declaration eagerly, including declarations shadowed by higher-precedence
values. Successful captures are cached even though the higher-precedence value
remains effective.
`vars list` is the lazy exception and does not execute a dynamic declaration
shadowed by a machine-local value. A remote with
`allow-dynamic-vars = false` never executes or caches that remote's dynamic
declarations.

`vars refresh` targets only leaf dynamic variables. If the leaf declares none,
the command does not load, create, or rewrite the cache file or its directory.

`--dry-run` does not change dynamic-variable cache policy and may therefore
update this file. The command-line specification defines the shared [dry-run
behavior](cmdline.md#dry-run-behavior).

Deleting `dynamic-vars.toml` is safe and does not change persisted user
configuration. The next command that needs a dynamic value may rerun its
producer command.

## Shared read and write rules

All three files use the same state-file write path.

### Reading and validation

- TOML parsing and schema validation happen before applying input precedence or
  running dynamic commands.
- An existing malformed file is a fatal configuration-load error. Batfiles
  reports it and leaves the file untouched rather than replacing it.
- Closed records reject unknown fields. Map containers such as `vars.toml` and
  the top level of `dynamic-vars.toml` accept arbitrary valid data keys, while
  any known record stored beneath them remains strict.
- Diagnostics should identify the source file and TOML path when practical.

### Writing

Each mutation is a whole-document rewrite:

1. Build and serialize the complete replacement in memory.
2. Write it to a temporary file in the destination directory.
3. Atomically rename the temporary file over the destination.

The rename is the commit point. Concurrent readers see either the complete old
document or the complete new document, never a torn write. If a failure occurs
before rename, the temporary file is removed and the destination is unchanged.

Consequences of this policy:

- Comments and original key order are not preserved; rewritten files use the
  serializer's canonical order.
- Batfiles creates no `.bak` or recovery sidecar files.
- There are no file locks. Concurrent read-modify-write operations can lose a
  logical update; the last writer wins, although neither writer publishes a
  partial document.
- Atomicity does not guarantee crash durability. The specs exclude fsync of
  the temporary file and containing directory, so a power loss may lose a
  just-written document.
- New files and directories honor the process umask. Existing file permissions
  are preserved where the platform and atomic-write implementation support it.
