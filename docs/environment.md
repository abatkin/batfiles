# Environment variables

The environment inputs batfiles reads today: the four location variables that
select where it works, the two run-only skip lists, the four bootstrap lists,
the one-shot user variables, the color selection, and the release base; and the environment a
[dynamic variable](#how-dynamic-commands-are-run)'s command runs in. There is
also one family it deliberately does *not* pass on to Git, covered at the end.

The process environment is captured once when batfiles starts, so every lookup
during a run sees the same values.

Environment variable **names** follow the host operating system's case
sensitivity. On Unix they are used verbatim. On Windows, whose environment is
case-insensitive, batfiles uppercases every variable name at capture so that all
lookups are deterministic. Values are never case-folded.

## Location variables

| Variable              | Equivalent option | Effect                                                                            |
|-----------------------|-------------------|-----------------------------------------------------------------------------------|
| `BATFILES_DIR`        | `--batfiles-dir`  | Selects the leaf repository.                                                      |
| `BATFILES_HOME`       | `--home-dir`      | Selects the destination home directory.                                           |
| `BATFILES_CONFIG_DIR` | `--config-dir`    | Selects the directory containing `vars.toml` and `disabled.toml`.                 |
| `BATFILES_CACHE_DIR`  | `--cache-dir`     | Selects the directory containing the dynamic-variable cache, `dynamic-vars.toml`. |

The four are global because their corresponding CLI options are global. A
command still reads or acts on only the roots it needs, and a command that needs
none of them — `version`, and `init`, which works on the current directory —
skips location resolution entirely.

All four are live. `sync`, the two apply commands, and a normal `vars list` open
the `batfiles.toml` in the leaf repository and both documents under the config
directory — [`disabled.toml`](state.md) and
[`vars.toml`](state.md#varstoml-machine-local-variables); the first three also
install into the selected home. Each of them reads and writes
[`dynamic-vars.toml`](state.md#dynamic-varstoml-dynamic-variable-cache) under
the cache directory when the manifest declares a dynamic variable. The enable
and disable commands rewrite `disabled.toml` and open nothing else, as the
machine-local variable commands do for `vars.toml`. Which of the four a given
command resolves is settled under [location selection](#location-selection).

## Run-only skips

| Variable                | Equivalent option | Effect                                                   |
|-------------------------|-------------------|----------------------------------------------------------|
| `BATFILES_SKIP_ACTIONS` | `--skip-action`   | Names actions to leave out of the current run.           |
| `BATFILES_SKIP_GROUPS`  | `--skip-group`    | Names groups to leave out of the current run.            |

Both are comma-separated lists. Items are trimmed and empty items discarded.
The lists are combined with the matching CLI options. See
[selection](cmdline.md#selecting-what-a-run-does) for command-specific handling,
precedence of reported exclusions, and unmatched-address diagnostics.

```console
$ BATFILES_SKIP_GROUPS=" gui , fonts" batfiles sync --skip-action p10k
```

## Bootstrap enable and disable lists

| Variable                   | Equivalent option  | Effect                                                       |
|----------------------------|--------------------|--------------------------------------------------------------|
| `BATFILES_DISABLE_ACTIONS` | `--disable-action` | Adds action addresses to the state a bootstrap writes.       |
| `BATFILES_ENABLE_ACTIONS`  | `--enable-action`  | Removes action addresses from the state a bootstrap writes.  |
| `BATFILES_DISABLE_GROUPS`  | `--disable-group`  | Adds group addresses to the state a bootstrap writes.        |
| `BATFILES_ENABLE_GROUPS`   | `--enable-group`   | Removes group addresses from the state a bootstrap writes.   |

All four are comma-separated lists read the same way the run-only skips are:
items trimmed, empty items discarded. They are honored **only by a bootstrap
that accepts the matching options**: [`clone`](cmdline.md#clone), and
[`sync --bootstrap`](cmdline.md#sync). They are meant for generated installers
and fresh-machine automation rather than as ambient controls, so a later `sync`
without `--bootstrap` in a shell that still exports them is unaffected.

Unlike the run-only skips, what these decide is written down: a bootstrap
persists the outcome in [`disabled.toml`](state.md), where it stands until an
enable or disable command changes it.

### Bootstrap adoption precedence

A bootstrap settles what this machine starts with switched off by applying four
sources in order, each over the one before it:

```text
leaf repository default-disabled candidates
< environment disables
< environment enables
< command-line disables
< command-line enables
```

So the command line outranks the environment, and within either source enable is
applied after disable, which is what makes enable win where the same address
appears in both lists. The
[candidates](repoformat.md#default-disabled-bootstrap-entries) are the layer
beneath all four: whatever the repository proposed, this invocation can overturn.

An address is recorded rather than resolved, exactly as the enable and disable
commands record one, so naming something no action answers to is not an error —
see [addresses](cmdline.md#addresses). A malformed address is: one written as an
option fails the command, and one written in a variable warns and is dropped,
for the same reason the run-only skips treat the two differently.

Only a machine with **no `disabled.toml` at all** is offered the candidates. The
document existing means this machine has an opinion of its own, and the section
is a starting point rather than a standing setting; the explicit decisions above
apply either way, because they were written for this invocation. A bootstrap
that decides nothing writes no document, so nothing is latched on a machine that
was never set up.

## One-shot variables: `BATFILES_VAR_<NAME>`

| Variable              | Equivalent option        | Effect                             |
|-----------------------|--------------------------|-------------------------------------|
| `BATFILES_VAR_<NAME>` | `--var <NAME>=<value>`   | Defines a one-shot user variable.  |

Every environment key beginning with `BATFILES_VAR_` defines a candidate
one-shot variable:

- The suffix after `BATFILES_VAR_` is the variable name exactly as written and
  is case-sensitive on Unix. On Windows the whole name is uppercased at capture,
  so `BATFILES_VAR_editor` defines the user variable `EDITOR`; name the matching
  `[vars]` and `vars.toml` keys in uppercase for Windows.
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

A warning names the environment variable and not its value, which may be a
token. The one listing that prints values is the one asked for them: `-vv`,
below.

### Two distinct destinations

`BATFILES_VAR_<NAME>` and the [`env` namespace](#host-environment-in-conditions)
are separate channels, and one environment variable can appear in both:

- `BATFILES_VAR_FOO` defines the **user variable** `FOO`, which a condition
  reads as the bare identifier `FOO`, subject to the precedence below.
- `env` exposes the *raw* process environment, so the same variable is also
  `env["BATFILES_VAR_FOO"]`. It takes no part in user-variable precedence.

Only the `BATFILES_VAR_` prefix creates a user variable. A raw `FOO` in the
environment is reachable as `env.FOO` and does **not** become the user variable
`FOO`; a condition writing a bare `FOO` for it gets an undeclared-identifier
error.

## Variable precedence

Layers can declare a user variable. Each overrides the ones before it:

```text
included remote [vars]
< leaf [vars]
< include-remote vars overrides
< persisted machine-local vars.toml
< BATFILES_VAR_*
< one-shot --var
```

The layers are merged into one flat set for every command that executes actions
— `sync`, `apply-action`, and `apply-group` — in both real and dry runs. Every
value is a string, and an empty string is a value like any other: a higher
layer's empty value overrides a lower layer's non-empty one. Repeating `--var`
for one key is the same rule applied within a layer, so the last value written
wins.

**The first and third layers belong to one inclusion, and reach only what that
inclusion contributed.** The set a leaf repository's own records are decided
against has the other four layers: the leaf's `[vars]`, `vars.toml`,
`BATFILES_VAR_*`, and `--var`. A record an [`include-remote`](repoformat.md#include-remote)
contributed is decided against that set with two more in it — the included
remote's own [`[vars]`](repoformat.md#variables-an-included-remote-declares)
beneath every other layer, and that inclusion's
[`vars`](repoformat.md#variables-for-one-inclusion) above the leaf's `[vars]`.

So a remote says what its own conditions read, a leaf composing it overrides
that without having to know it is there, and this machine overrides both. Two
inclusions of one remote are two scopes and can get different answers out of the
same manifest, while nothing in either scope reaches the leaf's own records, the
inclusion's own condition, or the remote's.

**A [dynamic variable](repoformat.md#dynamic-variables) takes the place of the
manifest that declared it.** A leaf's is in the leaf's `[vars]` layer and a
remote's in that remote's, so every layer above overrides it the same way. Two
cases are the declaration's rather than its value's:

- One a remote is not [allowed](repoformat.md#git) to run declares nothing at
  all, so whatever is beneath it stands, as though it were not written.
- One that ran and has no value still overrides the layers beneath it. The
  variable reads as the empty string rather than as the lower layer's value:
  the higher declaration won, and produced nothing.

Because the merge reads [`vars.toml`](state.md#varstoml-machine-local-variables),
a malformed or unreadable one fails these commands as a malformed manifest does.
So does a malformed `dynamic-vars.toml`, when a declaration needs it.

The merged set supplies [condition](repoformat.md#conditions) values. Use
`vars list` or an action command's `-vv` output to inspect values and origins;
the command reference owns the [listing format](cmdline.md#vars-list). A line
naming a layer one inclusion derived says which inclusion, since `batfiles.toml`
alone names three documents in a run that includes two remotes.

## How dynamic commands are run

A [dynamic variable](repoformat.md#dynamic-variables)'s command is an arbitrary
program. Batfiles runs it as the invoking user, with no sandbox, in both a real
and a [dry](cmdline.md#dry-run-behavior) run, and whatever it does besides
printing is its own business.

It **inherits the batfiles process environment**, and nothing is added to it:
the resolved variables are not exported, so a command that needs an input reads
the ordinary environment, or a file in the repository that declared it.

It **runs in the root of the repository that declared it**: the leaf
repository, or the materialization of the remote whose manifest declared it. A
relative path in the command resolves there, not in the directory batfiles was
started from.

A `command` written as a string runs under `sh -c` on Unix and `cmd /C` on
Windows — never the user's login shell, whose startup files would make one
manifest capture differently on two machines whose owner prefers a different
shell. A `command` written as a list is run directly, with no shell.

Its three standard streams are connected as follows:

- **Standard input** is connected to nothing, so a command that reads it sees
  end of input rather than waiting on a terminal nobody is watching.
- **Standard output** is captured for `capture = "stdout"` and discarded for
  `capture = "status"`. It never reaches batfiles' own [standard
  output](cmdline.md#output-streams). The value is everything written to it
  until it closes, which is normally when the command exits. A process the
  command leaves running that still holds it open delays the capture, and if it
  is still open at `command-timeout` the capture fails: nothing says the value
  is complete. A command that starts something in the background should
  redirect that process's output, as `daemon >/dev/null &` does.
- **Standard error** is inherited, so the command's own diagnostics reach the
  user verbatim. `--quiet` disconnects it; batfiles still warns about a failed
  capture itself.

`command-timeout` bounds the whole run. When it expires the command is killed,
and the run is a failure under either capture, so a status capture that was cut
off does not read as `"false"`. Only the process batfiles started is killed; one
it started in the background keeps running.

A `capture = "stdout"` command is also stopped as soon as it has written more
than 1 MiB. Batfiles holds the output in memory and never more than that, and
once it stops reading, it closes the output: anything still writing to it,
including a process the command left running, has its writes refused rather
than stored anywhere.

## Host facts in conditions

The `facts` namespace is what batfiles knows about the machine it is running on.
It is string-valued and read-only, like `env`, and takes no part in user-variable
precedence.

```toml
when = "facts.os == 'macos'"
unless = "facts.family == 'windows'"
```

It contains exactly these keys:

| Key              | Value                                                        |
|------------------|--------------------------------------------------------------|
| `facts.os`       | The operating system: `linux`, `macos`, `windows`, and so on. |
| `facts.arch`     | The target architecture: `x86_64`, `aarch64`, and so on.     |
| `facts.family`   | The operating-system family: `unix` or `windows`.            |
| `facts.hostname` | The host's configured name.                                  |

- **A key batfiles does not define is the empty string**, not an error, matching
  `env` and the [condition rule](repoformat.md#conditions). That is what makes
  the set safely extensible — and equally what makes a typo quiet, since
  `facts.arhc == 'arm64'` is simply false. The set is enumerated above so that
  there is something to check a spelling against.
- **The set is extensible.** A later batfiles may define more keys; adding one
  is a non-breaking change, because a manifest cannot have relied on it resolving
  to the empty string in any way that mattered.
- Every key name is identifier-compatible, so member access always works.
  Indexing (`facts["os"]`) is accepted for symmetry with `env` and is never
  required.

**macOS is `macos`, not `darwin`.** This is the value most likely to be guessed
wrong: `uname -s` prints `Darwin` and the Rust target triple is
`aarch64-apple-darwin`, but `facts.os` is `macos` on every Apple platform. A
condition written as `facts.os == 'darwin'` is not an error — it is simply never
true, so the record it gates is silently skipped on exactly the machines it was
written for.

**`facts.hostname` is the name the platform reports, and batfiles never truncates
it at the first dot.** On a Unix machine configured with a fully qualified name
it is `silver.example.net`; on one configured with a short name it is `silver`.
The cost is real: `facts.hostname == 'silver'` works on the second machine and
silently fails on the first, because a mismatch is a false condition rather than
an error. Batfiles does not truncate, because the domain is what distinguishes
work from home on some fleets and truncating would lose it just as silently.
Write the name your machines actually report, or compare against the qualified
form.

**Windows reports the short name, even on a domain-joined machine.** The value
comes from `GetComputerNameExW(ComputerNamePhysicalDnsHostname)`, the host
component with the DNS suffix excluded, so a machine whose fully qualified name
is `silver.example.net` has `facts.hostname == 'silver'` there while the same
name on Unix compares equal to the qualified form. A condition that must work on
both writes the short form, or tests the domain separately. Reporting the
qualified Windows name is possible — a different call to the same API — and is
tracked as an enhancement in [the roadmap](future/roadmap.md#enhancements).

## Host environment in conditions

The `env` namespace exposes arbitrary host environment variables to a condition.
These values are separate from the `BATFILES_*` configuration inputs above.

```toml
when = "env.HOME != ''"
when = "env[\"XDG_CONFIG_HOME\"] != ''"
```

- Names follow the host operating system's case sensitivity: verbatim on Unix,
  uppercased at capture on Windows. Reference Windows host variables by their
  uppercase form (`env.PATH`); a lowercase reference is the empty string.
- A set variable resolves to its string value and is never re-typed as a boolean
  or a number.
- An unset variable is the empty string. A lookup never fails merely because a
  key is absent.
- Identifier-compatible names may use member access, such as `env.HOME`. Indexing
  is available for those too and is required for other keys, such as
  `env["XDG_CONFIG_HOME"]`.
- The namespace is read-only and takes no part in user-variable precedence.

The whole environment is captured once, at startup, so every condition in one run
reads the same values.

## Location selection

A command resolves only the roots its own work needs, and there are two sets. A
command that installs nothing and reads no repository resolves the **config and
cache directories alone**: the four enable and disable commands, `vars set`,
`vars get`, `vars unset`, and `vars list --machine-only`. A command that reads
the leaf repository resolves those two and also selects the destination home and
the leaf repository: `sync`, `apply-action`, `apply-group`, `clone`, a normal
`vars list`, and `vars refresh`. `version` and `init` resolve no roots at all.

[`init`](cmdline.md#init) consults the invoking user's OS home for one thing
only: to refuse initializing a repository directly in it. That is not root
selection, and `--home-dir` and `BATFILES_HOME` have no bearing on it — the
point of the check is the home the user would land in from a fresh shell. A home
that cannot be determined is not fatal there.

The destination home is selected in this order:

```text
--home-dir > BATFILES_HOME > current user's OS home directory
```

The OS home directory is the one the platform reports for the invoking user: on
Unix `$HOME` when it is set and non-empty, otherwise the current user's passwd
entry; on Windows `%USERPROFILE%` when it is set and non-empty, otherwise the
user's profile directory as reported by the OS.

Failure to determine a home directory for a command that needs one is fatal.
Batfiles does not silently use the current directory as the destination home.
A command that resolves the config and cache directories alone selects no
destination home, so `--home-dir` and `BATFILES_HOME` do not apply to it and no
home has to be determined for it.

The leaf repository is selected in this order:

```text
--batfiles-dir
> BATFILES_DIR
> current directory when it contains batfiles.toml
> <selected-home>/dotfiles
```

Working-directory discovery checks only `./batfiles.toml`; it does not search
parent directories or test whether the manifest is valid. Once that file
selects the repository, a command that reads it reports an unreadable,
malformed, or invalid manifest instead of falling through to
`<selected-home>/dotfiles`. Changing the working directory can therefore change
the selected repository when neither explicit repository input is set.

Only commands that read the leaf repository perform this discovery. An enable
or disable command, for example, does not inspect the working directory.
Neither does [`clone`](cmdline.md#clone), which is the one command that selects
a leaf repository without reading one: discovery names a directory precisely
because a manifest is already in it, and `clone` creates the repository it
clones into, so it selects from the remaining three entries alone. When a
command does need the repository and has no explicit repository selection,
failure to determine the working directory or inspect `./batfiles.toml` is
fatal: batfiles cannot tell whether the working-directory precedence entry
applies, so it does not silently choose `<selected-home>/dotfiles`.

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
do **not** follow `--home-dir` or `BATFILES_HOME`. The leaf repository's final
fallback, `<selected-home>/dotfiles`, tracks the selected home. To root config or
cache under an alternate install home, set `--config-dir`/`--cache-dir` or the
corresponding `XDG_*`/`BATFILES_*` variable explicitly.

An absent or empty location variable is treated as unset. Location values are
not trimmed; whitespace is part of the path value.

The OS home is consulted only when a root still needs it. Selecting every root a
command resolves — including by way of `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME`
— therefore works even where no home directory can be determined at all. For a
command that resolves the config and cache directories alone, those two are
every root it resolves: `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME` alone are
enough to run it on a machine with no determinable home.

`batfiles <command> -v` prints the roots that command resolved, which is the way
to check what a given combination of options and variables selected for it. A
command that reads the leaf repository prints `repository:` and `home:` ahead of
`config:` and `cache:`. A command that resolves the config and cache directories
alone prints those two and nothing else: it has no home and no repository to
report, rather than a resolved value withheld from the report.

## Color

Color selection follows this precedence:

```text
--color > BATFILES_COLOR > non-empty NO_COLOR > auto
```

`BATFILES_COLOR` accepts `auto`, `always`, or `never`. An absent or empty
`BATFILES_COLOR` is treated as unset, and the next input in precedence decides.
Any other unrecognized value produces a diagnostic and falls back instead of
silently selecting a different color mode.

Precedence stops at the first input that answers, and an input that is never
consulted is never validated: an invalid `BATFILES_COLOR` is reported when it is
reached, and passed over in silence when `--color` already settled the question.

`NO_COLOR` follows the cross-tool convention: presence alone is insufficient;
its value must be non-empty. It acts as `never` only when neither `--color` nor
`BATFILES_COLOR` supplies a higher-precedence choice. Color inputs affect only
presentation.

`auto` is left unresolved by this precedence and answered by whatever is about
to print. Batfiles' own warnings and errors follow **standard error**, because
that is the only stream it ever colors — requested data goes to standard output
unlabeled and uncolored, so there is no second stream whose state could
disagree. The help, `--version`, and usage-error output that clap renders is
handed the mode untouched and applies clap's own terminal detection.

The selected color mode also applies to help, version output, and argument
parsing errors.

## Release base

| Variable        | Effect                                                                   |
|-----------------|--------------------------------------------------------------------------|
| `BATFILES_BASE` | The [release base](distribution.md#the-release-base) `init` writes into the stub, and [`update`](cmdline.md#update) installs from. |

An absent or empty `BATFILES_BASE` is treated as unset, and the base this build
was released from applies, which a build compiles in from
`BATFILES_DEFAULT_BASE` and otherwise takes to be the official one. A value that
is not a URL made of the characters the installers quote is an error for the
command that reads it. The hosted installer and the stub read the same variable
on their own; see [distribution](distribution.md).

## Variables passed on to `git`

`git-clone` runs the `git` on your `PATH`, which inherits batfiles' own
environment, so the things that make Git work as you have set it up keep
working: `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM`, `GIT_SSH_COMMAND`,
`GIT_ASKPASS`, `SSH_AUTH_SOCK`, and the proxy variables are all passed through
untouched.

One family is removed first:

```text
GIT_DIR  GIT_WORK_TREE  GIT_COMMON_DIR
GIT_CEILING_DIRECTORIES  GIT_DISCOVERY_ACROSS_FILESYSTEM  GIT_PREFIX
GIT_INDEX_FILE  GIT_OBJECT_DIRECTORY  GIT_ALTERNATE_OBJECT_DIRECTORIES
GIT_NAMESPACE  GIT_CONFIG  GIT_CONFIG_COUNT
```

Repository discovery and command-local configuration overrides are cleared along
with repository, index, and object-store redirects. The one command that checks
whether an existing `.git` is a repository directory at all, before any other
runs there, also sets `GIT_CONFIG_NOSYSTEM=1`, points `GIT_CONFIG_GLOBAL` and
`GIT_CONFIG_SYSTEM` at `/dev/null`, sets `LC_ALL=C`, and removes `LANGUAGE`, so
its answer is about the directory rather than your configuration, and its one
meaningful failure can be recognized. Every later command sees your
configuration as usual. `GIT_CONFIG_COUNT` overrides
are not supported; use your Git configuration files for proxy or header settings.
This is protection against accidental inherited state, not a security boundary.
