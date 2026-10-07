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
| [`init`](commands/init.md#init) | Create a repository skeleton or add missing installer stubs |
| [`version`](commands/version.md#version) | Print the installed version |
| [`update`](commands/update.md#update) | Check for or install a binary release |
| [`clone`](commands/clone.md#clone) | Clone a leaf repository, adopt bootstrap choices, and sync |
| [`sync`](commands/sync.md#sync) | Materialize remotes and execute selected actions |
| [`apply-action`](commands/apply-action.md#apply-action), [`apply-group`](commands/apply-group.md#apply-group) | Execute a named part of the manifest |
| [`enable-action`, `disable-action`, `enable-group`, `disable-group`](commands/enable-disable.md#enable-and-disable-actions-or-groups) | Persist machine-local selections |
| [`vars set`](commands/vars.md#vars-set), [`get`](commands/vars.md#vars-get), [`unset`](commands/vars.md#vars-unset) | Manage machine-local variables |
| [`vars list`](commands/vars.md#vars-list) | Inspect effective values and origins |
| [`vars refresh`](commands/vars.md#vars-refresh) | Refresh cached dynamic variables |

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
headings and exclusions. `-vv` also prints the [effective variables](commands/vars.md#vars-list).
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

## Selecting what a run does

Selection preserves declaration order. `sync` considers all actions; apply
commands consider the named action or group. Exclusions come from:

- [Persistent disables](state.md#disabledtoml-disabled-actions-and-groups).
- Run-only skips: CLI options and [environment lists](environment.md#run-only-skips).
- The record's [condition](repoformat.md#conditions).
- For included records, the inclusion's [filters](actions/include-remote.md#selecting-part-of-a-remote),
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
[applies nothing](commands/apply-action.md#apply-action).

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
entries, and the lists an [inclusion](actions/include-remote.md#include-remote) contributed,
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

**Unreleased:** addresses selecting individual clone-list entries require
a build newer than 0.1.0.

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
or group that cannot be resolved still [fails](commands/apply-action.md#apply-action).
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
