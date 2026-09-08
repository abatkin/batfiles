# Environment variables

The environment inputs batfiles reads today: the four location variables that
select where it works, the two run-only skip lists, the one-shot user variables,
and the color selection. There is also one family it deliberately does *not*
pass on, covered at the end. The rest — bootstrap adoption and the host facts
conditions use — are in [`future/environment.md`](future/environment.md), along
with the table naming every variable in the intended set.

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

Three of the four are live so far. `sync` and the two apply commands open the
`batfiles.toml` in the leaf repository and both documents under the config
directory — [`disabled.toml`](state.md) and
[`vars.toml`](state.md#varstoml-machine-local-variables) — and install into the
selected home; the enable and disable commands rewrite `disabled.toml` and open
nothing else, as the three machine-local variable commands do for `vars.toml`.
Only the cache directory is still an answer to where a command *would* work. All
four roots are resolved together.

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

Conditions will read these variables under a second, unrelated name as well:
`BATFILES_VAR_FOO` will be readable as the raw environment entry
`env.BATFILES_VAR_FOO`, which is a separate channel that does not participate in
the precedence below. That namespace is not built; it is specified in
[`future/environment.md`](future/environment.md#two-distinct-destinations).

## Variable precedence

Four layers can declare a user variable. Each overrides the ones before it:

```text
leaf [vars]
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

Because the merge reads [`vars.toml`](state.md#varstoml-machine-local-variables),
a malformed or unreadable one now fails these commands as a malformed manifest
does.

**Nothing consults a merged value yet.** Variables exist to feed `when` and
`unless` conditions, and no command evaluates one, so the set a run resolves
changes nothing about what it installs. What it does do is answer questions
about itself: `batfiles <command> -vv` prints the effective set, each variable
with the value in force, the layer that supplied it, and the layers that value
overrode.

```console
$ BATFILES_VAR_editor=code batfiles sync -vv --var editor=emacs
variables:
  editor  = "emacs" (--var; over BATFILES_VAR_*, vars.toml, batfiles.toml)
  profile = "work" (vars.toml; over batfiles.toml)
```

Unlike the machine-local variable commands, whose outcome lines deliberately
name a key and never its value, this listing prints values: it exists to show
which layer won, and `-vv` is a request for exactly that.

Actions spliced from an included remote will add layers of their own, specified
in [`future/environment.md`](future/environment.md#runtime-variable-precedence).

## Location selection

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
or disable command, for example, does not inspect the working directory. When a
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

The OS home is consulted only when a root still needs it. Selecting every root
explicitly — including by way of `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME` —
therefore works even where no home directory can be determined at all.

`batfiles <command> -v` prints the resolved roots. Commands that read a leaf
repository include a `repository:` line; commands that do not read one omit the
line because working-directory discovery did not select a repository. This is
the way to check what a given combination of options and variables selected for
that command.

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

Color is resolved before the arguments are parsed, because clap may need to
render a usage error for arguments it could not parse, and that output should
honor the requested color too. This is why `--color` is recovered from the raw
arguments rather than read off the parsed command.

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
with repository, index, and object-store redirects. `GIT_CONFIG_COUNT` overrides
are not supported; use your Git configuration files for proxy or header settings.
This is protection against accidental inherited state, not a security boundary.
