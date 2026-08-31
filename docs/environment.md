# Environment variables

The environment inputs batfiles reads today: the four location variables that
select where it works, the two run-only skip lists, and the color selection. The
rest — one-shot variable overrides, bootstrap adoption, and the host facts
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

Three of the four are live so far. `sync` opens the `batfiles.toml` in the leaf
repository and the [`disabled.toml`](state.md) under the config directory, and
installs into the selected home; the enable and disable commands rewrite
`disabled.toml` and open nothing else. Only the cache directory is still an
answer to where a command *would* work. All four are resolved together anyway,
because one set of rules covers all four roots and splitting it would mean
writing those rules twice.

## Run-only skips

| Variable                | Equivalent option | Effect                                                   |
|-------------------------|-------------------|----------------------------------------------------------|
| `BATFILES_SKIP_ACTIONS` | `--skip-action`   | Names actions to leave out of the current run.           |
| `BATFILES_SKIP_GROUPS`  | `--skip-group`    | Names groups to leave out of the current run.            |

Both are comma-separated lists. Each item is trimmed, empty items are discarded,
and the remaining items are **unioned** with the values of the matching option
rather than replacing or being replaced by them — these say what to leave out,
so anything either source names is left out. This is why the general
`option > variable` precedence does not apply to them.

```console
$ BATFILES_SKIP_GROUPS=" gui , fonts" batfiles sync --skip-action p10k
```

`sync` honors both, and `clone` will when it is built.
[`apply-group`](cmdline.md#apply-group) honors `BATFILES_SKIP_ACTIONS` and
ignores `BATFILES_SKIP_GROUPS`, matching the options it accepts: it has already
named the group it is applying, so a group skip could only contradict that.
[`apply-action`](cmdline.md#apply-action) ignores both, naming one action being
what waives every reason to pass it over.

Skips apply only to the current run and are never
persisted to [`disabled.toml`](state.md); what a run does with the two together,
and what it says about a name that matches nothing, is specified in
[selecting what a run does](cmdline.md#selecting-what-a-run-does).

Each item is an action or group [address](cmdline.md#addresses). Since items are
trimmed and split on commas, an address may contain neither — which is why the
ID rule excludes both characters.

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
Batfiles does not silently use the current directory.

The leaf repository is selected in this order:

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
default, `<selected-home>/dotfiles`, tracks the selected home. To root config or
cache under an alternate install home, set `--config-dir`/`--cache-dir` or the
corresponding `XDG_*`/`BATFILES_*` variable explicitly.

An absent or empty location variable is treated as unset. Location values are
not trimmed; whitespace is part of the path value.

The OS home is consulted only when a root still needs it. Selecting every root
explicitly — including by way of `$XDG_CONFIG_HOME` and `$XDG_CACHE_HOME` —
therefore works even where no home directory can be determined at all.

`batfiles <command> -v` prints the four resolved roots, which is the way to
check what a given combination of options and variables selected.

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
