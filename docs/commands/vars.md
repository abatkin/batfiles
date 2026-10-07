# Machine variables

## `vars set`

```text
batfiles vars set <key> <value>
```

Store a machine-local string verbatim, including an empty string. Output names
only the key:

| Change | Report |
| --- | --- |
| New key | ``set `editor` `` |
| Different value | ``changed `editor` (it had a different value)`` |
| Same value | ``` `editor` was already set to that value ``` |

## `vars get`

```text
batfiles vars get <key>
```

Print the persisted machine-local string on standard output. No other variable
layers or host facts are resolved. An absent key exits 1, names the key on
standard error, and prints nothing on standard output.

## `vars unset`

```text
batfiles vars unset <key>
```

Remove a machine-local value. An absent key succeeds and reports
``` `editor` was not set ```.

## What the three of them share

These commands operate on [`vars.toml`](../state.md#varstoml-machine-local-variables)
without loading a repository. Invalid [variable names](../repoformat.md#names-and-ids)
exit 1 before the document is opened. See the state reference for idempotence,
empty documents, and writes, and [output streams](../cmdline.md#output-streams) for quiet mode.

## `vars list`

```text
batfiles vars list [--machine-only] [--no-refresh]
```

Print effective leaf variables, their origins, and shadowed origins to standard
output. Resolve `[vars]`, `vars.toml`, and `BATFILES_VAR_*` using
[variable precedence](../environment.md#variable-precedence). `--var` is not
accepted. Host `facts`/`env` and inclusion-local scopes are not listed.

```text
editor  = "nvim" (vars.toml; over batfiles.toml)
profile = "work" (vars.toml; over batfiles.toml)
rank    = "9" (BATFILES_VAR_*; over batfiles.toml)
```

| Option | Effect |
| --- | --- |
| `--machine-only` | List only persisted `vars.toml` values; read no repository, variable environment layer, or dynamic cache |
| `--no-refresh` | Run no dynamic commands and write no cache; report cached values |

Dynamic-variable evaluation follows the [cache rules](../state.md#when-declarations-are-evaluated).
`--no-refresh` adds nothing to `--machine-only`. A normal listing requires a
manifest; `--machine-only` works without one.

Names are sorted and aligned. Values are quoted, including `""`, with control
characters escaped to keep each entry on one line. Shadowed origins run from
highest to lowest precedence. An empty set writes nothing to standard output
and reports that there is nothing to list on standard error.

Action commands at `-vv` use this format on standard error, indented under
`variables:` and including the run's `--var` overrides. Opened inclusions add a
block naming only variables declared by their overrides or included manifest:

```text
variables:
  profile = "personal" (batfiles.toml)
include-remote `corp` variables:
  editor  = "vim" (batfiles.toml of include-remote `corp`)
  profile = "work" (include-remote `corp`; over batfiles.toml)
```

Blocks appear during assembly, before action output. An unopened inclusion or
one declaring no variables has no block. An unnamed inclusion uses its
[reporting name](../actions/include-remote.md#include-remote). Values overridden by a higher
layer still appear with the winning origin:

```text
profile = "lab" (vars.toml; over include-remote `corp`, batfiles.toml)
```

A winning dynamic declaration includes its capture state:

```text
email  = "me@corp.example" (batfiles.toml, command)
has_op = "false" (batfiles.toml, command could not start)
shell  = "zsh" (batfiles.toml, cached 3h ago)
team   = "platform" (batfiles.toml, command failed, cached 2d ago)
token  = no value (batfiles.toml, command failed)
```

`command` means captured now; cache ages use the largest whole unit. Under
`--no-refresh`, stale entries say `stale, cached 2d ago` and missing entries say
`no value (batfiles.toml, not cached)`. Overridden declarations have no separate
capture state. Listings explicitly expose values; mutation reports name only
keys, and `vars get` supplies a bare persisted value for scripts.

## `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Run dynamic commands regardless of cache freshness and save their captures.
With no keys, refresh every declaration [in play](../state.md#when-declarations-are-evaluated),
including ones overridden by machine values. With keys, refresh only those
named, plus leaf resolution needed to decide remote eligibility.

A leaf key is its variable name; a remote key is `<remote-id>.<name>` using the
leaf's `[remotes]` ID, not an inclusion ID. Other syntax, including the cache's
`remote:corporate.has_op`, is a usage error before any file is read.
`--var` is not accepted.

Remote eligibility uses the leaf scope. Its declarations resolve first, reusing
fresh values and running stale ones; explicitly named leaf keys are refreshed
before deciding eligibility. Naming only leaf keys reads no remote.

Keys are checked before their commands run. All invalid requests are collected
in one error with a reason per key; exit status is 1. A key is refused if it:

- Is undeclared or static.
- Names an undeclared remote, one out of play, one not allowed to run commands,
  or one not materialized.

Leaf values already refreshed to decide remote eligibility remain refreshed
when a remote key is refused. With no keys, an unmaterialized remote in play
warns and is skipped. This command fetches nothing; use `sync` first.

Each capture reports a line such as ``refreshed `email` `` on standard error, without its value;
`--quiet` suppresses it. A command with nothing to refresh says so. Capture
failures warn and retain cached values; other captures are saved, then the
command exits 1 with the failure count. A status command that could not start
counts as failed because its assumed `"false"` was not captured.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
