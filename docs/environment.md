# Environment variables

Environment parsing, precedence, host inputs, subprocess execution, and release lookups.

[Locations](#location-selection) · [Skips](#run-only-skips) ·
[Bootstrap](#bootstrap-enable-and-disable-lists) · [Variables](#variable-precedence) ·
[Dynamic commands](#how-dynamic-commands-are-run) ·
[Release lookups](#how-github-release-lookups-are-made) · [Host facts](#host-facts) ·
[Host environment](#host-environment) · [Color](#color) ·
[Releases](#release-base) · [Git](#variables-passed-on-to-git)

The environment is captured once at startup. Names are case-sensitive on Unix;
Windows names are uppercased at capture. Values are never case-folded.

## Location variables

| Variable              | Equivalent option | Effect                                                                            |
|-----------------------|-------------------|-----------------------------------------------------------------------------------|
| `BATFILES_DIR`        | `--batfiles-dir`  | Selects the leaf repository.                                                      |
| `BATFILES_HOME`       | `--home-dir`      | Selects the destination home directory.                                           |
| `BATFILES_CONFIG_DIR` | `--config-dir`    | Selects the directory containing `vars.toml` and `disabled.toml`.                 |
| `BATFILES_CACHE_DIR`  | `--cache-dir`     | Selects the directory containing the dynamic-variable cache, `dynamic-vars.toml`. |
| `BATFILES_STATE_DIR`  | `--state-dir`     | Selects the directory containing the `run`-action record, `runs.toml`.            |

See [location selection](#location-selection) for precedence and which roots
each command resolves.

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

Like run-only skips, these are comma-separated, trimmed lists with empty items
discarded. They apply only to `clone` and `sync --bootstrap`. See
[bootstrap adoption](state.md#bootstrap-adoption) for persistence.

### Bootstrap adoption precedence

Inputs apply in order, each overriding previous decisions:

```text
leaf default-disabled candidates
< environment disables
< environment enables
< command-line disables
< command-line enables
```

Thus CLI wins over environment, and enables win over disables within either.
Only candidates eligible under [bootstrap adoption](state.md#bootstrap-adoption)
participate; explicit inputs apply even when candidates are not offered.

Addresses are recorded without resolving them. Malformed option addresses fail
the command; malformed environment addresses warn and are dropped.

## One-shot variables: `BATFILES_VAR_<NAME>`

`BATFILES_VAR_<NAME>` defines a user variable for this invocation, equivalent to
`--var <NAME>=<value>`. It is never persisted to `vars.toml`.

- The suffix is the exact variable name. On Windows capture uppercases it, so
  `BATFILES_VAR_editor` defines `EDITOR`; use uppercase manifest and machine keys.
- A bare `BATFILES_VAR_` is ignored; an empty value is significant.
- Invalid [variable names](repoformat.md#names-and-ids), including reserved
  identifiers, warn and are dropped. Warnings name the environment key but never
  its value. Invalid CLI `--var` names instead [fail](cmdline.md#shared-action-execution-options).

### Two distinct destinations

`BATFILES_VAR_FOO` creates the user variable `FOO`, subject to precedence below.
It is also visible unchanged as `env["BATFILES_VAR_FOO"]` in the raw environment.
An ordinary environment key `FOO` is only `env.FOO`; it creates no user variable,
so a bare `FOO` in a condition is an undeclared name.

## Variable precedence

Each layer overrides those above it in this list:

```text
included remote [vars]
< leaf [vars]
< include-remote vars overrides
< persisted machine-local vars.toml
< BATFILES_VAR_*
< one-shot --var
```

An empty string overrides a lower value. Repeated `--var` assignments use the
last value. Execution commands merge these layers in real and dry runs.

| Scope | Decides | Layers used |
| --- | --- | --- |
| **Leaf scope** | Leaf actions and entries, inclusion conditions, remote conditions | Leaf `[vars]`, machine, environment, CLI |
| Inclusion scope | Records an inclusion contributed, including clone-list entries | All six, with that inclusion's remote defaults and overrides |

Each opened inclusion has a separate scope, even when two include the same
remote. Its values never affect the leaf, another inclusion, or its own gate.

A dynamic declaration, a command or a `github-release` lookup, belongs to its
declaring manifest's layer. A remote command that is not
[allowed](repoformat.md#git) declares nothing; a remote's lookups need no
permission. A declaration producing no value still overrides lower layers and
reads as the empty string. A higher layer is how one machine pins a value, such
as `--var fzf_tag=v0.60.0`.

Malformed or unreadable `vars.toml` fails merging; so does a malformed dynamic
cache when a declaration needs it. Inspect values and origins with
[`vars list` or `-vv`](commands/vars.md#vars-list).

## How dynamic commands are run

These rules also govern a [`run` action's](actions/run.md#how-the-command-runs)
command, except where its page says otherwise.

Dynamic commands run as the invoking user without a sandbox, in real and dry
runs. They inherit batfiles' environment; resolved variables are not exported.
Their working directory is the declaring repository's root: the leaf or remote
materialization.

| Declaration | Launch |
| --- | --- |
| String | `sh -c` on Unix; on Windows, `cmd /S /C` with the string quoted, so `cmd` reads it as written |
| List of strings | Direct program and arguments, without a shell |

A relative program with a directory part, such as `./bin/setup`, is found from
the working directory. A bare name, such as `git`, is looked up on `PATH` as
usual.

The login shell and its startup files are not used. Standard input is closed.
Standard error is inherited verbatim, or disconnected under `--quiet`;
batfiles' own failure warnings remain visible.

| Capture | Result |
| --- | --- |
| `stdout` | Surrounding whitespace trimmed; requires exit 0 and valid UTF-8, with at most 1 MiB output |
| `status` | `"true"` for exit 0, `"false"` for other exit statuses; output discarded |

Output never reaches batfiles' standard output. Stdout capture waits for the
stream to close, including when a background child holds it open; redirect
background output, for example `daemon >/dev/null &`.

`command-timeout` bounds the entire capture. Expiry kills only the directly
started process, not background children, and fails either capture mode; it
does not produce a status value of `"false"`. Stdout exceeding 1 MiB also stops
the command and closes the pipe, refusing further writes rather than storing
them. [Cache rules](state.md#freshness-and-refresh-behavior) define fallback and
how failure to start differs from a captured nonzero status.

## How GitHub release lookups are made

A [`github-release`](repoformat.md#github-release-declarations) declaration's
value is the tag of a repository's latest release: GitHub's own notion of
latest, the most recent release that is not a draft or a prerelease. Batfiles
asks for the repository's release page and reads the tag from where it is
redirected, without following the redirect:

```text
GET <github-url>/OWNER/REPO/releases/latest
```

`<github-url>` is the declaration's resolved
[`github-url`](repoformat.md#the-github-url-field), `https://github.com` by
default. The request follows the shared
[transfer's](repoformat.md#the-transfer-both-fetching-actions-share) proxy and
TLS trust rules, and sends no credentials, so it sees only public repositories. It runs no code, and needs no permission in an included manifest.
Lookups run one at a time, in real and dry runs, like commands.

The lookup succeeds when the answer is a redirect (301, 302, 303, 307, or 308)
whose `Location`, absolute or relative and without any query or fragment,
contains `/releases/tag/`. The value is everything after the last
`/releases/tag/`, percent-decoded, exactly as GitHub names the tag, so a tag
holding a `/` stays whole and a `v` prefix is kept. The response is trusted:
the `Location` is not checked against the host or repository asked.

| Answer | Failure reason |
| --- | --- |
| No connection, a DNS or TLS failure, or no answer within `timeout` | The request failed, or timed out |
| `404` | GitHub has no public repository `OWNER/REPO` |
| A redirect to `.../releases` | `OWNER/REPO` has no published release |
| A redirect to `.../releases/latest` | `OWNER/REPO` has moved to the new location; update `github-release` |
| Any other status, or a redirect elsewhere | Not a redirect to a release |
| An empty tag, or one that is not UTF-8 | The tag is unusable |

A renamed repository's redirect is not followed: the manifest should name its
current home. A `github-url` that fails to evaluate or resolves to an invalid
URL also fails the lookup. Reasons are ASCII, and quote server-supplied text
only when it is printable ASCII. [Cache rules](state.md#freshness-and-refresh-behavior)
define what a failed lookup falls back on.

`timeout`, ten seconds by default, bounds the whole lookup rather than each
phase of it. A refused connection or a failed DNS lookup fails at once; only a
network that drops packets costs up to `timeout` for each declaration.

## Host facts

`facts` is a read-only string namespace for [conditions](repoformat.md#conditions)
and [templates](repoformat.md#templates), separate from user-variable precedence.
Member and index access both work: `facts.os` or `facts["os"]`.

| Key | Value |
| --- | --- |
| `facts.os` | `linux`, `macos`, `windows`, and other platform names |
| `facts.arch` | Target architecture, such as `x86_64` or `aarch64` |
| `facts.family` | `unix` or `windows` |
| `facts.hostname` | Platform-reported host name |

Unknown keys return the empty string; later versions may add keys. macOS uses
`macos`, not `darwin`.

On Unix, the hostname retains any configured domain: `silver.example.net` is
not truncated to `silver`. On Windows it is the short physical DNS hostname,
without the suffix, even on domain-joined machines. A condition shared by Unix
and Windows machines should compare the short name, or test the domain
separately. A qualified Windows name is tracked in
[enhancements](https://github.com/abatkin/batfiles/blob/main/docs/contributing/enhancements.md#fully-qualified-windows-host-name).

## Host environment

`env` exposes the captured process environment as read-only strings, separate
from user variables, to [conditions](repoformat.md#conditions),
[templates](repoformat.md#templates), and a `github-release` declaration's
[`github-url`](repoformat.md#the-github-url-field) (in an included manifest,
only with
[permission](actions/include-remote.md#templates-and-the-host-environment)).
An unset key is the empty string. Use member syntax for
identifier-compatible names or index syntax for any name:

```toml
when = "env.HOME != ''"
when = 'env["XDG_CONFIG_HOME"] != ""'
```

Windows references must use uppercase keys (`env.PATH`); a lowercase lookup
returns the empty string. Values are not converted to numbers or booleans.

## Location selection

Commands resolve only the roots they need:

| Commands | Roots |
| --- | --- |
| Enable/disable commands; `vars set`, `get`, `unset`, `list --machine-only` | Config and cache |
| `sync`, apply commands, `clone` | Repository, home, config, cache, state |
| Normal `vars list`, `vars refresh` | Repository, home, config, cache |
| `init`, `version`, `update` | None |

`init` separately checks the OS home to refuse initialization directly in it;
see [`init`](commands/init.md#init).

Choose the first available value in each row:

| Root | Precedence, highest first |
| --- | --- |
| Destination home | `--home-dir` → `BATFILES_HOME` → OS home |
| Leaf repository | `--batfiles-dir` → `BATFILES_DIR` → current directory containing `batfiles.toml` → `<selected-home>/dotfiles` |
| Config | `--config-dir` → `BATFILES_CONFIG_DIR` → `$XDG_CONFIG_HOME/batfiles` → `<os-home>/.config/batfiles` |
| Cache | `--cache-dir` → `BATFILES_CACHE_DIR` → `$XDG_CACHE_HOME/batfiles` → `<os-home>/.cache/batfiles` |
| State | `--state-dir` → `BATFILES_STATE_DIR` → `$XDG_STATE_HOME/batfiles` → `<os-home>/.local/state/batfiles` |

An absent or empty location variable is unset. Values are not trimmed.
OS home uses nonempty `$HOME` on Unix, otherwise the passwd entry; Windows uses
nonempty `%USERPROFILE%`, otherwise the OS profile directory.

Repository discovery checks only `./batfiles.toml`, never parents. It does not
validate the manifest before selecting the directory; an unreadable or invalid
manifest then fails instead of falling through. Failure to determine the current
directory or inspect that file is fatal when discovery is needed. Explicit
repository selection avoids discovery. `clone` also skips discovery and uses
the other three repository choices.

Config, cache, and state fallbacks always use **OS home**, independently of
`--home-dir`/`BATFILES_HOME`. Only the repository fallback follows the selected
destination home. Set config, cache, or state options or environment variables
explicitly to relocate machine state.

OS home is consulted only when a needed root lacks an explicit value. A command
that needs it and cannot determine it fails; it never falls back to the current
directory. Selecting all required roots works without a determinable home.
Config/cache-only commands do not use the destination-home options.

`-v` prints only resolved roots: `repository:`, `home:`, `config:`, `cache:`,
`state:` in that order, leaving out those the command does not resolve. A
command that does not resolve the state root never fails for want of one. Cache
selection also determines which runs share the [run lock](state.md#run-lock),
which stays in the cache directory.

## Color

```text
--color > BATFILES_COLOR > non-empty NO_COLOR > auto
```

`BATFILES_COLOR` accepts `auto`, `always`, and `never`; absent or empty means
unset. An invalid value warns and falls back. Inputs below the first valid
choice are not consulted or validated. Nonempty `NO_COLOR` selects `never`
only when neither higher-priority input decides.

`auto` follows standard error's terminal status for batfiles diagnostics;
requested data on standard output stays uncolored. Help, `--version`, and
usage errors receive the selected mode and use clap's terminal detection.

## Release base

| Variable        | Effect                                                                   |
|-----------------|--------------------------------------------------------------------------|
| `BATFILES_BASE` | The [release base](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#the-release-base) `init` writes into the stub, and [`update`](commands/update.md#update) installs from. |

An absent or empty `BATFILES_BASE` is unset, and the base compiled into the
build applies. A value outside the [base syntax](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md#the-release-base)
fails the command that reads it. The hosted installer and the stub read the same
variable themselves. A self-hoster sets it in the environment their dotfiles
install.

## Variables passed on to `git`

Git actions, clone lists, and Git remotes run the `git` on your `PATH` with
batfiles' environment, so your Git setup keeps working: `GIT_CONFIG_GLOBAL`,
`GIT_CONFIG_SYSTEM`, `GIT_SSH_COMMAND`, `GIT_ASKPASS`, `SSH_AUTH_SOCK`, and the
proxy variables pass through untouched. These are removed first:

```text
GIT_DIR  GIT_WORK_TREE  GIT_COMMON_DIR
GIT_CEILING_DIRECTORIES  GIT_DISCOVERY_ACROSS_FILESYSTEM  GIT_PREFIX
GIT_INDEX_FILE  GIT_OBJECT_DIRECTORY  GIT_ALTERNATE_OBJECT_DIRECTORIES
GIT_NAMESPACE  GIT_CONFIG  GIT_CONFIG_COUNT
```

That clears repository discovery, command-local configuration overrides, and
repository, index, and object-store redirects. `GIT_CONFIG_COUNT` overrides are
therefore unsupported; put proxy or header settings in your Git configuration.
This protects against accidental inherited state; it is not a security boundary.

The first command run against an existing `.git`, which checks that it is a
repository directory, also sets `GIT_CONFIG_NOSYSTEM=1`, points
`GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` at `/dev/null`, sets `LC_ALL=C`, and
removes `LANGUAGE`, so its answer depends only on the directory. Later commands
see your configuration as usual.

## Options without environment equivalents

There are no environment-variable equivalents for `--dry-run`, `--refresh-vars`,
`--refresh-remotes`, `--refresh-content`, `--no-overwrite`, `--interactive`,
`--bootstrap`, explicit apply commands, or verbosity (`--verbose` and `--quiet`). Persisted enable/disable commands also have no
ambient environment equivalent: the [`BATFILES_ENABLE_*` and
`BATFILES_DISABLE_*` lists](#bootstrap-enable-and-disable-lists) apply only to
bootstrap commands.
