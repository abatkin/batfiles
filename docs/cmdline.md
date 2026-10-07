# Batfiles command-line surface

Commands, accepted options, output, and exit statuses. Every command and option
below is implemented; unrecognized commands and options are usage errors, as
[exit statuses](#exit-statuses) describes.

## Command overview

```text
batfiles [global-options] <command> [command-options]
```

Global options may appear before or after the command name.

| Command | Purpose |
| --- | --- |
| [`init`](#init) | Create a repository skeleton or add missing installer stubs |
| [`version`](#version) | Print the installed version |
| [`update`](#update) | Check for or install a binary release |
| [`clone`](#clone) | Clone a leaf repository, adopt bootstrap choices, and sync |
| [`sync`](#sync) | Materialize remotes and execute selected actions |
| [`apply-action`](#apply-action), [`apply-group`](#apply-group) | Execute a named part of the manifest |
| [`enable-action`, `disable-action`, `enable-group`, `disable-group`](#enable-and-disable-actions-or-groups) | Persist machine-local selections |
| [`vars set`](#vars-set), [`get`](#vars-get), [`unset`](#vars-unset) | Manage machine-local variables |
| [`vars list`](#vars-list) | Inspect effective values and origins |
| [`vars refresh`](#vars-refresh) | Refresh cached dynamic variables |

Shared reference: [execution options](#shared-action-execution-options),
[output](#output-streams), [selection](#selecting-what-a-run-does),
[dry-run](#dry-run-behavior), [partial plans](#plan-completeness),
[failures](#execution-failures).

## Global options

These options are accepted by every command, although a command only uses the
locations relevant to its work. `init`, for example, operates on the current
directory and does not use any selected roots.

| Option                          | Purpose                                                                                                           |
|---------------------------------|-------------------------------------------------------------------------------------------------------------------|
| `-v`, `--verbose`               | Increase diagnostic detail. May be repeated, such as `-vv`.                                                       |
| `-q`, `--quiet`                 | Suppress informational output and leave errors or explicitly requested data. Mutually exclusive with `--verbose`. |
| `--color <auto\|always\|never>` | Control colored output. Defaults to `auto`; see [color selection](environment.md#color).                          |
| `--batfiles-dir <path>`         | Select the leaf repository. See [location selection](environment.md#location-selection) for its fallbacks.                         |
| `--home-dir <path>`             | Select the destination home directory. Defaults to the current user's home directory.                             |
| `--config-dir <path>`           | Select the directory containing `vars.toml` and `disabled.toml`. Defaults to the XDG config location.             |
| `--cache-dir <path>`            | Select the directory containing `dynamic-vars.toml` and the [run lock](state.md#run-lock). Defaults to the XDG cache location. |

`-v` prints the roots the command resolves, unchanged destinations, and action
headings and exclusions. `-vv` also prints the [effective variables](#vars-list).
See [location selection](environment.md#location-selection) for which roots
each command uses and their precedence, and [output streams](#output-streams)
for quiet mode.

## Shared action execution options

These options are accepted by every command that executes actions: `sync`,
`apply-action`, `apply-group`, and `clone`, which forwards them to its follow-up
synchronization.

| Option              | Purpose                                                                         |
|---------------------|---------------------------------------------------------------------------------|
| `--var <key=value>` | Set a one-shot variable. Repeatable; the last value for a key wins.             |
| `--refresh-vars`    | Run every dynamic variable's command, even where its cached value is fresh.     |
| `--refresh-content` | Install copies and fetched seeds again over what is at their destinations.     |
| `--no-overwrite`    | Skip anything in a destination's way instead of backing it up and replacing it. |
| `--interactive`     | Ask at each destination in the way: back up and replace, overwrite, or skip.    |

A `--var` key must be a [variable name](repoformat.md#names-and-ids); an invalid
key is a usage error before roots are resolved or files read. An empty value,
such as `--var profile=`, overrides lower layers. See
[variable precedence](environment.md#variable-precedence).

`--refresh-vars` forces commands for the declarations this run evaluates; see
[cache evaluation](state.md#when-declarations-are-evaluated).

[Conflicts and backups](safety.md#conflicts-and-backups) defines destination
handling and its output. `--no-overwrite` and `--interactive` are mutually
exclusive, and `--interactive` cannot accompany `--dry-run`.
`--refresh-content` affects only `copy`, `copy-dir`, `fetch-file`, and
`fetch-archive`; see [refreshing seeds](safety.md#refreshing-seeds).

## Output streams

| Stream | Content | Effect of `--quiet` |
| --- | --- | --- |
| Standard output | Requested data: `version`, `vars get`, `vars list`, `update --check` | None |
| Standard error | Changes, progress, prompts, warnings, errors | Suppresses informational lines; keeps warnings and errors |

Requested data has no labels or color. Errors and warnings raised by batfiles
use `error:` and `warning:` labels, colored bold red and bold yellow when
[color](environment.md#color) is enabled. Verbose detail is unlabeled.

Dynamic commands inherit standard error unless `--quiet` disconnects it; their
output is otherwise captured or discarded. See
[dynamic-command execution](environment.md#how-dynamic-commands-are-run).

## Commands

### `init`

```text
batfiles init [--no-git-init | --stubs]
```

Create a leaf-repository skeleton in the current directory. Location options
such as `--batfiles-dir` have no effect.

| Option | Purpose |
| --- | --- |
| `--no-git-init` | Skip `git init`; it is also skipped automatically inside a Git repository |
| `--stubs` | Add only missing installer stubs to an existing repository |

The layout is created in this order: `batfiles.toml`, `.gitignore`, `bin/`,
`files/`, executable [`install.sh`](distribution.md#leaf-stub), and
[`install.ps1`](distribution.md#the-windows-stub), on every platform. The
starter manifest has only commented samples and installs nothing. The
`.gitignore` excludes generated `remotes/`; that tree is not created by `init`.
Stubs use the selected [release base](environment.md#release-base), validated
before anything is created.

Before writing, `init` refuses:

- Anything already named `batfiles.toml`.
- The invoking user's OS home directory, regardless of `--home-dir`. An
  undeterminable home does not fail this check.
- A layout path of the wrong filesystem kind. Symlinks are classified by their
  targets; broken ones are the wrong kind.

Existing paths of the expected kind retain contents and permissions and are
omitted from the creation report. An existing `.gitignore` that appears not to
cover `remotes/`, or an existing installer that is not a batfiles stub, warns
and is left alone. A failure to launch or complete `git init` fails the command;
any layout already created remains.

#### Adding the stubs to a repository

`init --stubs` requires a `batfiles.toml` file in the current directory. It
writes only missing stubs, creates no other layout, and runs no Git. It cannot
be combined with `--no-git-init`. Existing stubs follow the rules above; a wrong
node kind refuses the command before writing. Output names the stubs created,
or reports that all were already present.

### `version`

```text
batfiles version
```

Print `batfiles <version>` to standard output, like `batfiles --version`.
[Release builds](distribution.md#versions) report the release version including
its pre-release suffix; other builds report the `Cargo.toml` version.
No location roots are resolved.

### `update`

```text
batfiles update [<version>] [--check]
```

Replace the running binary with the latest [release](distribution.md#release-tree),
or with `<version>`. Only this command installs or checks for updates, and only
when run. No location roots are resolved.

| Argument or option | Purpose |
|--------------------|---------|
| `<version>`        | The release to install: a [version](distribution.md#versions), with or without a leading `v`, or `latest`, the default. One outside the grammar is a usage error. |
| `--check`          | Print what is running and what is available, and install nothing. |

Releases come from the [release base](environment.md#release-base).

Without `<version>`, `update` reads `<base>/latest/download/VERSION` once and
downloads nothing unless that release is newer, by [version
order](distribution.md#versions), than the running one; a pre-release therefore
stays until a stable release passes it. If `VERSION` cannot be read, the error
suggests naming a release, since the base may have published only pre-releases.
A named `<version>` is installed even when it is older or the same, which also
repairs a binary.

Installation uses `<base>/download/v<version>/`:

1. Create a private file beside the running executable, after resolving
   symlinks, so a link to batfiles stays a link. An unwritable directory fails
   before downloading. An existing file at that path, such as one an interrupted
   update left, is never replaced; remove it and retry.
2. Download `SHA256SUMS` and this build's [asset](distribution.md#targets) into
   that file, verifying its digest.
3. Make it executable and run its `version`, which must report the release.
4. Rename it over the running executable, and report both versions and the path.

Any failure removes the file and leaves the running binary unchanged.

On Windows, the staged file ends in `.exe`, and the running `batfiles.exe` is
first renamed aside to `batfiles.exe.batfiles-old` (and restored if the second
rename fails). A hidden `%SystemRoot%` `powershell.exe` removes that file after
`update` exits. If another running batfiles still holds it, the next `update`
removes it first, reporting at `-v` and warning on failure.

`--check` reads only the needed `VERSION`: `latest/download/` without
`<version>`, otherwise that release's own, which must hold the named version.
It prints two lines of requested data to standard output:

```text
running 1.2.0
available 1.3.0
```

It succeeds whether or not the available release is newer; an unreadable or
invalid `VERSION` fails. Without `--check`, all output is diagnostic.

### `clone`

```text
batfiles clone <url> [--ref <ref>] [--skip-action <id>]... [--skip-group <group>]...
    [--enable-action <id>]... [--disable-action <id>]...
    [--enable-group <group>]... [--disable-group <group>]...
```

Clone a leaf repository into the selected batfiles directory, adopt its
bootstrap policy, and [sync](#sync). [Location selection](environment.md#location-selection)
excludes working-directory discovery for this command.

The destination must not exist, even as an empty directory or broken symlink.
Batfiles rejects it before launching Git; missing parents are created.

| Option | Purpose |
| --- | --- |
| `--ref <ref>` | Check out a branch, tag, or commit before reading the manifest; never empty |
| `--skip-action <id>` | Skip an action during this synchronization; repeatable |
| `--skip-group <group>` | Skip a group during this synchronization; repeatable |
| `--disable-action <id>` / `--enable-action <id>` | Persist a bootstrap action choice; repeatable |
| `--disable-group <group>` / `--enable-group <group>` | Persist a bootstrap group choice; repeatable |

The [shared execution options](#shared-action-execution-options) also apply.
`clone` accepts neither `--dry-run` nor `--refresh-remotes`.

`--ref` follows the [manifest ref rules](repoformat.md#ref-following-one-branch-tag-or-commit):
a published branch becomes a local tracking branch; other resolved refs are
detached. Without it, the origin's `HEAD` selects the branch. Batfiles records
no ref and never updates the leaf repository itself.

#### What the bootstrap decides

Bootstrap applies the leaf's [default-disabled candidates](repoformat.md#default-disabled-bootstrap-entries)
and explicit enable/disable inputs using [adoption precedence](environment.md#bootstrap-adoption-precedence),
then persists the result under the [state lifecycle rules](state.md#bootstrap-adoption).
Included manifests' bootstrap sections are ignored. Candidate conditions use
the leaf's effective variables; a condition failure warns and leaves the
candidate unapplied. Ordinary condition exclusions are reported at `-v`.

Each decision is reported with its source and the same wording as an
[enable/disable command](#enable-and-disable-actions-or-groups):

```text
default-disabled: disabled action `p10k`
BATFILES_DISABLE_GROUPS: disabled group `gui`
--enable-group: enabled group `gui` (was disabled)
```

Malformed option addresses fail before cloning; malformed environment addresses
warn and are dropped. Once cloned, the repository is kept on any later failure:

- An unresolved `--ref` fails before manifest loading and leaves the default
  branch checked out. Correct it with Git, then run `sync --bootstrap`.
- A missing `batfiles.toml` fails before bootstrap, identifying the clone as a
  Git repository that is not a batfiles repository.
- A synchronization failure leaves both the clone and written bootstrap state
  in place. Fix the cause and retry with `sync`.

### `sync`

```text
batfiles sync [--dry-run | --refresh-remotes] [--skip-action <id>]... [--skip-group <group>]...
    [--bootstrap [--enable-action <id>]... [--disable-action <id>]...
                 [--enable-group <group>]... [--disable-group <group>]...]
```

[Materialize remotes](repoformat.md#materialization), then execute selected
actions in declaration order. Each action sees the filesystem left by its
predecessors. [Selection](#selecting-what-a-run-does) defines exclusions and
loading; [execution failures](#execution-failures) defines stopping behavior.

| Option | Purpose |
| --- | --- |
| `--dry-run` | [Report intended work](#dry-run-behavior) |
| `--refresh-remotes` | Fetch every admitted file and archive remote again |
| `--skip-action <id>` | Skip one action; repeatable |
| `--skip-group <group>` | Skip one group; repeatable |
| `--bootstrap` | Adopt bootstrap choices before synchronization |

The [shared execution options](#shared-action-execution-options) also apply.

Normal output names each change, one line per link or installed child. Correct
destinations are silent. `-v` adds unchanged destinations and a heading per
action; unnamed actions use their one-based manifest position, and ungrouped
actions omit the group:

```text
symlink zshrc (group shell)
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
symlink-dir action 7 (group shell)
linked /home/you/.ackrc -> /home/you/dotfiles/files/ackrc
```

Remote work is reported first under `remote <id>` headings, using the Git or
fetching action's wording. Replaced file/archive materializations say
`refetched`; unchanged ones appear only at `-v`. Remote exclusions use
[exclusion reporting](#exclusion-reporting).

`--bootstrap` performs [clone's bootstrap](#what-the-bootstrap-decides).
The four enable/disable options in the syntax are accepted only with it;
otherwise they are usage errors. In a dry run, bootstrap reports its decisions
and uses them for selection without writing `disabled.toml`.
The [leaf stub](distribution.md#leaf-stub) runs `sync --bootstrap`.

`--refresh-remotes` rebuilds even materializations whose declarations still
match their stamps. Remote conditions and ownership checks still apply; Git
remotes are unaffected because normal synchronization already updates them.
It cannot accompany `--dry-run`.

### `apply-action`

```text
batfiles apply-action --id <id> [--dry-run]
```

Execute one action or clone-list entry by [address](#addresses), with `sync`'s
action behavior and the [shared execution options](#shared-action-execution-options).
`--id` is required. An action without an ID cannot be named directly.

The [selection table](#selection-by-command) defines which exclusions naming
an action bypasses. Apply commands use existing remote materializations.

These failures exit 1:

- An invalid address, checked before the repository is opened.
- An address matching nothing, with the address and manifest in the error.
- An unread enclosing inclusion or clone list, with its name and reason.
- An address naming an `include-remote` itself: it contributes actions but is
  not an executable action.

A contributed action excluded by inclusion filters still resolves, so naming it
succeeds without installing anything. `-v` identifies the record and reason:

```text
nothing to apply: the inclusion that contributed the action did not select it
```

### `apply-group`

```text
batfiles apply-group --group <group> [--skip-action <id>]... [--dry-run]
```

Execute a group's actions in declaration order, with `sync`'s action behavior
and the [shared execution options](#shared-action-execution-options).
`--group` is required; `--skip-action` is repeatable. See
[selection by command](#selection-by-command) for member exclusions.

An invalid or unknown group exits 1; a group exists only when an action names
it. A qualified group in an unread inclusion fails naming that inclusion.
An existing group with no eligible actions exits 0 and reports:

```text
nothing to apply: every action in the group is disabled, skipped, excluded by its own condition, or was not contributed
```

`-v` gives individual reasons. Inclusions count only through the actions they
contribute, so a group containing an inclusion that contributes nothing also
has nothing to apply.

### `vars set`

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

### `vars get`

```text
batfiles vars get <key>
```

Print the persisted machine-local string on standard output. No other variable
layers or host facts are resolved. An absent key exits 1, names the key on
standard error, and prints nothing on standard output.

### `vars unset`

```text
batfiles vars unset <key>
```

Remove a machine-local value. An absent key succeeds and reports
``` `editor` was not set ```.

### What the three of them share

These commands operate on [`vars.toml`](state.md#varstoml-machine-local-variables)
without loading a repository. Invalid [variable names](repoformat.md#names-and-ids)
exit 1 before the document is opened. See the state reference for idempotence,
empty documents, and writes, and [output streams](#output-streams) for quiet mode.

### `vars list`

```text
batfiles vars list [--machine-only] [--no-refresh]
```

Print effective leaf variables, their origins, and shadowed origins to standard
output. Resolve `[vars]`, `vars.toml`, and `BATFILES_VAR_*` using
[variable precedence](environment.md#variable-precedence). `--var` is not
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

Dynamic-variable evaluation follows the [cache rules](state.md#when-declarations-are-evaluated).
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
[reporting name](repoformat.md#include-remote). Values overridden by a higher
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

### `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Run dynamic commands regardless of cache freshness and save their captures.
With no keys, refresh every declaration [in play](state.md#when-declarations-are-evaluated),
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

## Selecting what a run does

Selection preserves declaration order. `sync` considers all actions; apply
commands consider the named action or group. Exclusions come from:

- [Persistent disables](state.md#disabledtoml-disabled-actions-and-groups).
- Run-only skips: CLI options and [environment lists](environment.md#run-only-skips).
- The record's [condition](repoformat.md#conditions).
- For included records, the inclusion's [filters](repoformat.md#selecting-part-of-a-remote),
  checked before all other exclusions.

CLI and environment skips form a union, as do skips and persistent disables.
Action and group namespaces are separate. An unnamed action can be selected by
its group; an ungrouped action by its ID. Identified clone-list entries belong
to the action namespace; groups reach entries only through the parent list.
Excluding an inclusion leaves its whole manifest unread.

Malformed run-only addresses warn when inputs are read. Well-formed skips
matching nothing warn after expansion and [clone-list preparation](#clone-list-preparation),
before any action executes. Both permit continuation. A name within an unread
inclusion or list is neither matched nor unmatched, and does not warn.
Persistent names matching nothing are always silent.

### Selection by command

| Exclusion source | `sync` | `apply-action` | `apply-group` |
| --- | --- | --- | --- |
| An inclusion's selection filters | Honor | Honor | Honor |
| Disabled action IDs | Honor | Waive | Honor |
| Disabled groups | Honor | Waive | Waive the named group; honor others |
| `--skip-action` / `BATFILES_SKIP_ACTIONS` | Honor | Option rejected; environment ignored | Honor |
| `--skip-group` / `BATFILES_SKIP_GROUPS` | Honor | Option rejected; environment ignored | Option rejected; environment ignored |
| The action's `when` / `unless` | Evaluate if otherwise selected | Do not evaluate | Evaluate if otherwise selected |

`clone` follows the `sync` column. Waivers affect only this invocation and never
edit persistent state. `apply-action` does not evaluate the named record's
condition even if it would fail.

**Naming a child does not waive its container's exclusions.** A qualified target
opens its enclosing inclusion or clone list only if that container passes
`sync`'s rules, including disables, accepted run-only skips, and its condition.
An excluded container remains unread and fails the target naming the reason:

```text
error: action `corp.zshrc` would come from include-remote `corp`, which is excluded: group `work` is disabled
error: entry `zsh-plugins.p10k` would come from git-clone-list `zsh-plugins`, which is excluded: action `zsh-plugins` is disabled
```

This includes a list excluded by inclusion filters: its entries cannot resolve.
A contributed action whose own record is filtered out still resolves but
[applies nothing](#apply-action).

Conversely, naming a list waives none of its entries' exclusions; each entry
uses the table under its own address. Naming one entry with `apply-action`
waives only that entry's exclusions. An `apply-group` target reaching an
inclusion reaches its contributed actions, but waives only the requested group:
a contributed action whose own group is disabled is still left out, so
`apply-group --group work` passes over the actions in a disabled `corp.shell`.

All three commands read the leaf manifest and `disabled.toml`, even when they
waive lists. Missing state is empty; missing manifests and unreadable or
malformed documents fail before action execution. Variable loading follows
[precedence](environment.md#variable-precedence).

Only `sync` [materializes remotes](repoformat.md#materialization); apply commands
use existing trees. Every command evaluates remote conditions, and none waives
them: sourcing an excluded remote fails. Only `sync` reports the remote's
exclusion as skipped materialization work.

### Clone-list preparation

After the run's list is expanded and selection and action conditions are settled,
all selected, unskipped clone lists are read and validated before any action
writes. This includes a list read only because the command names one of its
entries, and the lists an [inclusion](repoformat.md#include-remote) contributed,
each read from the materialization its record came from. Skipped lists are not
opened. A missing or malformed executable list fails the run before installation
begins, even if its action appears later in the list. A list produced by an
earlier action in the same run is therefore unavailable for preparation.

Preparation decides each entry as selection decides a record — its disable, then
the run-only skips, then its condition, evaluated only for an entry nothing else
excludes — but the exclusions are reported when the parent action runs, in list
order. It comes before run-only skips are checked for a match and before an
apply command's target is resolved, since an entry's address is known only once
its list has been read.

### Addresses

An address is a nonempty sequence of [IDs](repoformat.md#names-and-ids) joined
by `.`, with no segment-count limit. It is used wherever actions or groups are
named, including skips, apply targets, persistent choices, and bootstrap defaults.

| Form | Meaning |
| --- | --- |
| `<action-id>` | Leaf action |
| `<group>` | Leaf group |
| `<inclusion>.<action-id>` | Contributed action |
| `<inclusion>.<group>` | Contributed group |
| `<list>.<entry-id>` | Entry in a leaf clone list |
| `<inclusion>.<list>.<entry-id>` | Entry in a contributed clone list |

Unqualified names search only the leaf. `<inclusion>` is the inclusion record's
ID, which can differ from its remote's ID. Without an inclusion ID, contributed
content runs but cannot be addressed. An entry likewise needs both its own ID
and an addressable list. Entries have no group addresses.

Unique action IDs disambiguate two-segment forms: `zsh-plugins.p10k` is an
entry if `zsh-plugins` is a list, a contributed action if it is an inclusion.
Reaching into either follows [container selection](#selection-by-command).

Syntax and resolution are separate. Empty or invalid segments are malformed;
a valid address matching no supported form, such as `a.b.c.d.e`, is simply
not found. Commands storing addresses accept such names without resolving them.

### Exclusion reporting

Expected exclusions appear only at `-v`, in the action heading:

```text
create-dir zsh-cache (group shell) - skipped: group `shell` is disabled
symlink zshrc (group shell) - skipped: `zshrc` from --skip-action
symlink gitconfig-work (group git) - skipped: when "work" is false
symlink corp.p10k (group corp.prompt) - skipped: not selected by include-remote `corp`
```

Only the first applicable reason is reported, in this order: inclusion
filters, persistent disables, run-only skips, then the condition. Among the
disables, and among the skips, the action's own address comes before its group.
A skip names the option or environment variable responsible, and a condition is
quoted as written. A record excluded for another reason never has its condition
evaluated.

A condition that [cannot be evaluated](repoformat.md#when-a-condition-cannot-be-evaluated)
warns at every verbosity, without the `- skipped:` frame. The warning identifies
the record, condition, failure, and remedy, without revealing the offending value.

Clone-list exclusions appear under their parent in entry order. Unrequested
entries are silent; expected exclusions are verbose detail:

```text
not cloning https://github.com/company/internal-zsh-tools.git (plugins.txt line 2): when "work" is false
not cloning https://github.com/romkatv/powerlevel10k.git (id=p10k, plugins.txt line 3): action `zsh-plugins.p10k` is disabled
```

Failed entry conditions warn in the same `not cloning` frame and leave other
entries eligible. Remote exclusions appear before actions under `remote <id>`:
ordinary condition exclusions at `-v`, evaluation failures as warnings. Neither
stops the run, but an action sourcing that excluded remote fails naming it and
repeating the reason.

### Enable and disable actions or groups

```text
batfiles disable-action <id>...
batfiles enable-action <id>...
batfiles disable-group <group>...
batfiles enable-group <group>...
```

Add or remove persistent [addresses](#addresses) in `disabled.toml`. These
commands load no repository, run no synchronization, and remove no installed
content. Unknown names are accepted; malformed addresses exit 1 before opening
the document or applying any supplied name. Repeated names warn and apply once.

One diagnostic per name reports whether state changed:

```text
disabled action `p10k`
action `zshrc` was already disabled
```

`--quiet` suppresses reports, not edits. See [state lifecycle](state.md#semantics-and-lifecycle)
for idempotence and persistence.

## Dry-run behavior

A dry run inspects the real filesystem and reports intended work without
creating, replacing, or removing installation content. Each action sees the
original filesystem; earlier reported writes are not simulated.

Bookkeeping still runs, including the [run lock](state.md#run-lock) and
[dynamic-variable evaluation](state.md#when-declarations-are-evaluated).
Dynamic commands are arbitrary programs and can themselves change files or
contact the network. Their captures are saved in the cache.

Reports use `would link`, `would copy`, and similar wording, in the same order
and granularity as a real run: one line per child for directory-wide actions,
one for a whole installed tree. Exclusion reports are identical in both modes.

| Operation | Dry-run inspection and limits |
| --- | --- |
| Local source | Validate presence/readability before considering the destination, even if it would be kept |
| Occupied destination | Apply the same inspection policy and report each proposed backup name; never prompt |
| Seed | Report keeping an occupied destination, or the intended copy/download |
| `--refresh-content` | Build and fetch nothing; report the refresh that differing content would require, without testing equality |
| Git clone/update | Run no Git, including read-only commands; report cloning at a vacant destination or updating a directory |
| File/archive remote | Inspect node and stamp to decide whether to fetch, refetch, keep, or refuse |
| Source inside a remote | Read the existing materialization, however stale; fail if missing or excluded |
| Included manifest | Read the existing materialization; if absent, warn and mark the [plan partial](#plan-completeness) |

Clone-list entries are read during [preparation](#clone-list-preparation) and
reported individually; refs are printed as declared without resolving them.
No installation downloads or Git network requests occur. A directory cannot
be classified as a usable checkout without Git, so a dry run may report an
update where a real run would find a conflict. Other destination kinds are
classified normally.

A dry run cannot predict write permissions, concurrent changes, download
digests, or archive entries. Archives are reported as whole trees. Declared
remotes are described but never materialized, and excluded remotes are not read.

Overlapping actions can produce different reports in a real run. For example,
`create-dir` followed by `copy` at the same destination reports `would create`
and `would copy`, but a real run reports `created` and `kept`. A broken symlink
that a real run clears once can be reported repeatedly in dry-run mode,
including once per child when it occupies a directory action's container.

## Plan completeness

An `include-remote` whose missing materialization prevents listing its actions
warns with the remote, reason, and remedy:

```text
warning: remote `corporate` is not materialized at /home/me/dotfiles/remotes/corporate,
         so what it includes cannot be listed; run `batfiles sync` to bring it down
```

That warning marks a **partial** plan. Otherwise the plan is **complete**, with
no additional completion message. These do not make a plan partial:

- An unreached inclusion, or one excluded by a condition, disable, or skip.
- A remote whose condition closes.
- Warnings from a manifest that was read: unmatched inclusion filters, ignored
  nested inclusions, or ignored remote declarations.

Partiality alone does not fail a command: available actions still run or are
reported, and status is 0 absent another failure. A specifically targeted action
or group that cannot be resolved still [fails](#apply-action).
`sync` materializes remotes before action execution; dry runs and apply
commands use existing trees, where missing materializations can leave a partial
plan. Materialize the remote and repeat the command to see the remaining work.

## Execution failures

An action failure stops `sync` or an apply command with status 1. Earlier
changes remain; nothing is rolled back. Directory-wide actions likewise stop
at the first failed child, preserving work already completed for earlier children.

### Clone-list entry failures

`git-clone-list` processes entries independently. Something in the way of an
entry's destination, including a directory that is not a clone, an indirect
checkout, or an incomplete clone, is a [conflict](safety.md#conflicts-and-backups)
settled for that entry alone. The list warns and continues after:

- a Git subprocess that ran but failed, including clone, fetch, or update;
- a declared ref that cannot be resolved.

The warning names the repository source, list file, line number, and entry ID
when present. If the entry's destination was backed up and could not be put
back, it also names the backup. Warnings remain visible under `--quiet`.

These stop the run instead: failure to launch Git, filesystem read or write
failures, failure to create the destination container, and an unanswered
`--interactive` question. The categories are conservative; they do not claim
that every remaining entry would fail the same way.

Recoverable entry failures do not fail the command, so status 0 can include
entries that did not clone or update, even when every entry failed. Consult the
warnings and the [Git recovery policy](safety.md#git-updates) before retrying.
Standalone `git-clone` failures propagate normally.

Condition evaluation failures also warn and skip the affected action or entry
without making the command fail. The [condition rules](repoformat.md#when-a-condition-cannot-be-evaluated)
define this fail-closed behavior; [exclusion reporting](#exclusion-reporting)
defines the action warning format.

## Exit statuses

| Status | Meaning |
| --- | --- |
| `0` | Successful command, `--help`, or `--version`; may include warnings |
| `1` | Runtime or semantic failure; earlier work may remain |
| `2` | Usage error before execution; no files read or written |

Status 1 includes invalid addresses or machine-variable keys, missing or invalid
documents, unresolved apply targets, missing `vars get` values, refresh failures,
lock refusal, and installation/update failures. Explicit usage errors, such as
invalid `--var` keys or `vars refresh` key syntax, are called out above.

Failures before action execution leave installation destinations untouched,
although cloning, remote materialization, bootstrap state, and dynamic captures
may already have occurred. See [execution failures](#execution-failures) for
stopping behavior and warnings that permit status 0.
