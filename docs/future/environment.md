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

Environment inputs are parsed and combined with command-line arguments,
configuration files, and defaults as needed. Color is resolved separately
because it affects only presentation.

Capturing the environment once at startup, and the name case-sensitivity rule
that goes with it, are built and specified in
[`docs/environment.md`](../environment.md). Two consequences of that rule apply
to variables that do not exist yet: Windows uppercasing applies to the whole
name, including a `BATFILES_VAR_<name>` suffix, and it is what keeps the `env`
namespace deterministic there.

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

### Location selection

Location selection is built. The four location variables, their precedence, and
the OS-home rules are specified in
[`docs/environment.md`](../environment.md#location-selection).

Two parts of it are not built and stay here. The selected home is also intended
to control `~` expansion and to anchor home-relative destinations, whose
detailed path-safety rules are defined by the
[safety model](safety.md#destination-paths); there are no destinations yet. And
the leaf-repository selection is intended to supply the initial destination of
`batfiles clone`, which does not exist yet.

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

Run-only skips are built. `BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS`,
their comma-separated list rule, and the way they union with `--skip-action` and
`--skip-group` are specified in
[`docs/environment.md`](../environment.md#run-only-skips).

What is not built is what they can *reach*. A qualified address naming an
included remote's action or group parses and matches nothing today, for want of
a remote to resolve it against; see the [address forms](cmdline.md#address-forms)
below.

### Bootstrap enable and disable lists

The following variables are comma-separated lists parsed with the same
trim-items-and-drop-empties rule as the run-only skips:

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

Color selection is built. It is specified in
[`docs/environment.md`](../environment.md#color).

## Host facts in conditions

The `facts` namespace exposes what batfiles knows about the machine it is
running on. It is string-valued and read-only, like `env`, and it does not
participate in user-variable precedence.

```toml
when = "facts.os == 'macos'"
unless = "facts.family == 'windows'"
```

The namespace contains exactly these keys:

| Key | Value |
| --- | --- |
| `facts.os` | The operating system: `linux`, `macos`, `windows`, and so on. |
| `facts.arch` | The target architecture: `x86_64`, `aarch64`, and so on. |
| `facts.family` | The operating-system family: `unix` or `windows`. |
| `facts.hostname` | The host's configured name. |

Rules for `facts` values:

- A key batfiles does not define resolves to the empty string rather than
  failing, matching `env` and the rule in
  [the condition section](repoformat.md#condition). This is what makes the set
  safely extensible — and equally what makes a typo quiet, since `facts.arhc ==
  'arm64'` is simply false. The set above is enumerated so that there is
  something to check a spelling against.
- The set is extensible. A later batfiles may define additional keys; adding one
  is a non-breaking change, because a manifest cannot have been relying on it
  resolving to the empty string in any way that mattered.
- Every key name is identifier-compatible, so member access always works.
  Indexing (`facts["os"]`) is accepted for symmetry with `env` but is never
  required.

**macOS is `macos`, not `darwin`.** This is the value most likely to be guessed
wrong: `uname -s` prints `Darwin`, and the Rust target triple is
`aarch64-apple-darwin`, but `facts.os` is `macos` on every Apple platform. A
condition written as `facts.os == 'darwin'` is not an error — it is simply never
true, so the record it gates is silently skipped on exactly the machines it was
written for. The same shape of mistake applies to any misspelling; see the
missing-key rule above.

**`facts.hostname` is the host's configured name verbatim, and is not truncated
at the first dot.** On a machine configured with a fully qualified name it is
`silver.example.net`; on one configured with a short name it is `silver`. The
cost is real: `facts.hostname == 'silver'` works on the second machine and
silently fails on the first, because a mismatch is a false condition rather than
an error. It is stated rather than fixed because truncating would discard the
domain, which is what distinguishes work from home on some fleets, and would
lose it just as silently. Write the name your machines actually report, or
compare against the qualified form.

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
- An unset variable resolves to the empty string. Resolver lookups never fail
  merely because a key is absent.
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
goals describe the remaining [bootstrap model](../goals.md#product-model).

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
