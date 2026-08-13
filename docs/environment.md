# Environment variables

Batfiles reads environment variables for tool configuration, one-shot variable
overrides, run-only skips, bootstrap adoption, color selection, and host facts
used by conditions. Command-line arguments take precedence unless a more
specific rule below says otherwise.

## General precedence

The general input order, from highest to lowest precedence, is:

```text
command-line arguments > environment variables > configuration files > built-in defaults
```

The process environment is captured once when batfiles starts. Environment
inputs are parsed and combined with command-line arguments, configuration
files, and defaults as needed. Color is resolved separately because it affects
only presentation.

Environment variable **names** follow the host operating system's case
sensitivity. On Unix they are used verbatim. On Windows, whose environment is
case-insensitive, batfiles uppercases every variable name at capture so that all
lookups and the `env` namespace are deterministic. This applies to the whole
name, including a `BATFILES_VAR_<name>` suffix. Values are never case-folded.

## Batfiles configuration variables

| Variable                   | Equivalent option        | Effect                                                                                              |
|----------------------------|--------------------------|-----------------------------------------------------------------------------------------------------|
| `BATFILES_DIR`             | `--batfiles-dir`         | Selects the leaf repository.                                                                        |
| `BATFILES_HOME`            | `--home-dir`             | Selects the destination home directory and the home used to expand `~` and home-relative defaults.  |
| `BATFILES_CONFIG_DIR`      | `--config-dir`           | Selects the directory containing `vars.toml` and `disabled.toml`.                                   |
| `BATFILES_CACHE_DIR`       | `--cache-dir`            | Selects the directory containing the disposable dynamic-variable cache, `dynamic-vars.toml`.        |
| `BATFILES_VAR_<NAME>`      | `--var <NAME>=<value>`   | Defines a one-shot user variable for the invocation.                                                |
| `BATFILES_SKIP_ACTIONS`    | `--skip-action`          | Supplies action or addressable-child addresses to skip for the current run.                         |
| `BATFILES_SKIP_GROUPS`     | `--skip-group`           | Supplies group addresses to skip for the current run.                                               |
| `BATFILES_ENABLE_ACTIONS`  | `clone --enable-action`  | Removes action addresses from persisted bootstrap-disabled state.                                   |
| `BATFILES_DISABLE_ACTIONS` | `clone --disable-action` | Adds action addresses to persisted bootstrap-disabled state.                                        |
| `BATFILES_ENABLE_GROUPS`   | `clone --enable-group`   | Removes group addresses from persisted bootstrap-disabled state.                                    |
| `BATFILES_DISABLE_GROUPS`  | `clone --disable-group`  | Adds group addresses to persisted bootstrap-disabled state.                                         |
| `BATFILES_COLOR`           | `--color`                | Selects `auto`, `always`, or `never` color output.                                                  |
| `NO_COLOR`                 | none                     | Disables color when present with a non-empty value and no higher-precedence color selection exists. |

The four location variables are global because their corresponding CLI options
are global. A command still reads or acts on only the roots it needs. Commands
such as `init` and `version`, which need no resolved roots, skip location
resolution.

### Location selection

The destination home is selected in this order:

```text
--home-dir > BATFILES_HOME > current user's OS home directory
```

The OS home directory is the one the platform reports for the invoking user: on
Unix `$HOME` when it is set and non-empty, otherwise the current user's passwd
entry; on Windows `%USERPROFILE%` when it is set and non-empty, otherwise the
user's profile directory as reported by the OS.

Failure to determine a home directory for a command that needs one is fatal.
Batfiles does not silently use the current directory. The selected home also
controls `~` expansion and anchors home-relative defaults. Detailed destination
and path-safety rules are defined by the [safety model](safety.md#destination-paths).

The leaf repository is selected in this order, including for the initial
destination of `batfiles clone`:

```text
--batfiles-dir > BATFILES_DIR > <selected-home>/dotfiles
```

The config directory is selected in this order:

```text
--config-dir
> BATFILES_CONFIG_DIR
> $XDG_CONFIG_HOME/batfiles
> <os-home>/.config/batfiles when XDG_CONFIG_HOME is unset
```

The cache directory is selected independently in this order:

```text
--cache-dir
> BATFILES_CACHE_DIR
> $XDG_CACHE_HOME/batfiles
> <os-home>/.cache/batfiles when XDG_CACHE_HOME is unset
```

The config and cache directories hold batfiles' own machine-local state rather
than installed content, so their home-based fallbacks use the invoking user's OS
home directory (`<os-home>`, the same home used when `--home-dir` is absent) and
do **not** follow `--home-dir` or `BATFILES_HOME`. Only the leaf-repository
default (`<selected-home>/dotfiles`), `~` expansion, and home-relative
destinations track the selected home. To root config or cache under an alternate
install home, set `--config-dir`/`--cache-dir` or the corresponding
`XDG_*`/`BATFILES_*` variable explicitly.

An absent or empty location variable is treated as unset. Location values are
not trimmed; whitespace is part of the path value.

### One-shot variables: `BATFILES_VAR_<NAME>`

Every environment key beginning with `BATFILES_VAR_` defines a candidate
one-shot variable:

- The suffix after `BATFILES_VAR_` is the variable name exactly as written and
  is case-sensitive on Unix. On Windows the whole name is uppercased at capture
  (see above), so `BATFILES_VAR_editor` defines the user variable `EDITOR`; name
  the matching `[vars]`/`vars.toml` keys in uppercase for Windows.
- A bare `BATFILES_VAR_` with an empty suffix is ignored.
- An empty value is significant: `BATFILES_VAR_PROFILE=` defines `PROFILE` as
  the empty string.
- Names must follow the repository format's shared [user-variable name
  rules](repoformat.md#names-and-ids). An invalid suffix — including a reserved
  identifier, so `BATFILES_VAR_env` on a case-sensitive system — is reported as
  a warning naming the whole environment variable, and that one variable is
  ignored; the command continues. This is deliberately not the rule for an
  invalid [`--var` key](cmdline.md#shared-action-execution-options), which fails
  the command: an environment variable is ambient and may predate any interest
  in batfiles, while a `--var` was typed for this run.
- The override lasts only for the current invocation and is not written to
  `vars.toml`.

#### Two distinct destinations

`BATFILES_VAR_<NAME>` and the `env` namespace are separate channels, and the
same underlying environment variable can appear in both:

- `BATFILES_VAR_FOO` defines the **user variable** `FOO`, referenced in
  expressions as a bare identifier (`FOO`) and subject to the runtime variable
  precedence below.
- The read-only [`env` namespace](#host-environment-in-conditions) exposes the
  *raw* process environment under `env.*` (for example `env.FOO`, and also
  `env["BATFILES_VAR_FOO"]`). It does not participate in user-variable
  precedence.

Only the `BATFILES_VAR_` prefix creates a user variable. A raw `FOO` in the
environment is reachable as `env.FOO` but does **not** become the user variable
`FOO`. Environment keys in the `env` namespace follow the host operating
system's case sensitivity: on Unix `env.FOO` and `env.foo` are different keys,
while on Windows names are uppercased at capture, so reference them as `env.FOO`
(a lowercase reference resolves to the empty string like any absent key).

#### Runtime variable precedence

Leaf variable precedence is:

```text
leaf [vars]
< persisted machine-local vars.toml
< BATFILES_VAR_*
< one-shot --var
```

For actions spliced from an included remote, precedence is:

```text
remote [vars]
< leaf [vars]
< include-remote vars overrides
< persisted machine-local vars.toml
< BATFILES_VAR_*
< one-shot --var
```

All values follow the repository format's shared [string-valued variable
model](repoformat.md#string-valued-variables). Any coercion during condition
evaluation is performed by the expression language rather than by batfiles.

Declarations, not values, participate in precedence, which decides two cases the
lists above do not:

- A dynamic declaration a remote is not allowed to run contributes no variable
  at all, so a lower layer's value stands. This is what
  [`allow-dynamic-vars = false`](state.md#freshness-and-refresh-behavior) leaves
  behind: the command never runs, and the variable is not merely valueless but
  absent from that layer.
- A dynamic declaration that does run and produces no value still overrides the
  layers beneath it. The variable has no value rather than the lower layer's:
  the higher declaration won, and it produced nothing.

### Run-only skips

`BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS` are comma-separated lists.
Each item is trimmed, empty items are discarded, and the remaining items are
unioned with `--skip-action` or `--skip-group` values. Skips apply only to the
current run and are never persisted to `disabled.toml`.

### Bootstrap enable and disable lists

The following variables are comma-separated lists parsed with the same
trim-items-and-drop-empties rule:

- `BATFILES_DISABLE_ACTIONS`
- `BATFILES_ENABLE_ACTIONS`
- `BATFILES_DISABLE_GROUPS`
- `BATFILES_ENABLE_GROUPS`

They are honored only by bootstrap commands that accept the corresponding
options, such as `clone`. They are intended for generated installers and
fresh-machine automation, not as ambient controls for later `sync` runs.

Bootstrap adoption applies decisions in this order:

```text
leaf repo default-disabled candidates
< environment disables
< environment enables
< command-line disables
< command-line enables
```

Consequently, command-line decisions override environment decisions. Within
either source, enable is applied after disable, so enable wins when the same
action or group appears in both lists. Unlike run-only skips, bootstrap enable
and disable decisions update `disabled.toml`.

### Color

Color selection follows this precedence:

```text
--color > BATFILES_COLOR > non-empty NO_COLOR > auto
```

`BATFILES_COLOR` accepts `auto`, `always`, or `never`. `auto` enables color when
stdout is a terminal. An absent or empty `BATFILES_COLOR` is treated as unset,
as it is for the location variables, and the next input in precedence decides.
Any other unrecognized value produces a diagnostic and falls back instead of
silently selecting a different color mode.

`NO_COLOR` follows the cross-tool convention: presence alone is insufficient;
its value must be non-empty. It acts as `never` only when neither `--color` nor
`BATFILES_COLOR` supplies a higher-precedence choice. Color inputs affect only
presentation.

## Host environment in conditions

The `env` namespace exposes arbitrary host environment variables to `when`
expressions. These values are separate from `BATFILES_*` configuration inputs.

```toml
when = "env.HOME != ''"
when = "env[\"XDG_CONFIG_HOME\"] != ''"
```

Rules for `env` values:

- Names follow the host operating system's case sensitivity: verbatim on Unix,
  uppercased at capture on Windows. Reference Windows host variables by their
  uppercase form (`env.PATH`); a lowercase reference resolves to the empty string.
- A set variable resolves to its string value and is never re-typed as a boolean
  or number.
- An unset variable resolves to the empty string. Resolver lookups never fail or
  make a condition `unknown` merely because a key is absent.
- Environment variables with identifier-compatible names may use member access,
  such as `env.HOME`. Indexing is also available for those names and is required
  for other keys, such as `env["XDG_CONFIG_HOME"]`.
- The namespace is read-only and does not participate in user-variable
  precedence.
- `batfiles vars list` includes captured host values in the `env.*` namespace
  when it resolves the effective variable set.

## How dynamic commands are run

Dynamic variable commands inherit the `batfiles` process environment. Batfiles
does not export its resolved user-variable scope as environment variables. A
dynamic command that needs an environment input must read it from the normal
process environment; otherwise it should read files under the repository that
declared it.

Each command runs with its working directory set to the root of the repository
that declared it — the leaf repository, or the materialization of the remote
that declared the variable. A relative path in the command therefore resolves
there rather than in whatever directory `batfiles` was invoked from.

A `command` written as a string runs under `sh -c` on Unix and `cmd /C` on
Windows. It is never run under the user's login shell: a login shell runs that
user's startup files, so the same manifest would capture different values on two
machines whose owner happens to prefer a different interactive shell. A `command`
written as a list is executed directly, with no shell at all.

The child's three standard streams are connected as follows:

- **Standard input** is connected to nothing. A dynamic command that reads it
  sees end of input immediately rather than blocking on a terminal that may have
  nobody watching it.
- **Standard output** is captured for `capture = "stdout"` and discarded for
  `capture = "status"`. It is never inherited, so a dynamic command cannot write
  into the data channel described in [Output Streams](cmdline.md#output-streams).
  Only what the command had written when it exited becomes the value: a process
  it leaves running in the background neither extends the value nor delays the
  capture.
- **Standard error** is inherited, so the command's own diagnostics reach the
  user verbatim. `--quiet` disconnects it instead; batfiles still reports a
  failed capture with a warning of its own, but the command's explanation of the
  failure is lost until the same command is run again without the flag.

The `command-timeout` field bounds the whole run and defaults to `5s`. It must be
greater than zero. When it expires, batfiles kills the command and the run is a
**failure** for both capture modes: a command that was cut off never answered the
question, so `capture = "status"` reports a failure rather than the string
`"false"`. Only the command batfiles started is killed; a command string that
backgrounds a further process of its own leaves that process running.

A captured value is bounded as well as timed: a `capture = "stdout"` command that
writes more than **1 MiB** is stopped and its refresh fails. The output is not
truncated to fit, because a value cut in half is worse than no value at all, and
the limit applies while the command runs so that a command writing without end
cannot fill the temporary directory before its timeout expires.

## Bootstrap use of `PATH`

A generated `install.sh` uses a `batfiles` binary found on `PATH`. The product
goals describe the remaining [bootstrap model](goals.md#product-model).

## Deliberate exclusions

The specs intentionally define no environment-variable equivalent for:

- `--dry-run`;
- `--refresh-vars`;
- `--refresh-remotes`;
- `--refresh-content`;
- explicit apply commands; or
- verbosity (`--verbose`/`--quiet`).

General persisted enable/disable commands also have no ambient environment
equivalent. The `BATFILES_ENABLE_*` and `BATFILES_DISABLE_*` variables are the
narrow exception: they are consumed only during bootstrap adoption.
