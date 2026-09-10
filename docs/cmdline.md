# Batfiles Command-Line Surface

The parts of the command-line interface that run today: the set of commands, the
options every command accepts, where output goes, and what the exit status
means.

Unimplemented commands, options, and additional address forms are described in
[`future/cmdline.md`](future/cmdline.md).

## What runs today

The whole surface parses. Every command and option listed below is accepted, and
an invalid invocation is rejected as a usage error before anything else happens.

**Only `version`, `sync`, `apply-action`, `apply-group`, the four enable/disable
commands, and `vars set`, `vars get`, and `vars unset` do any work.** Every
other command resolves the location roots it needs and then reports that it is
not implemented yet, exiting 2 having written nothing.

`sync` executes the action types that exist, `--dry-run` reports what it
would execute without doing any of it, and `--skip-action`/`--skip-group` leave
part of it out for one run.

`apply-action` and `apply-group` carry out part of the same manifest, named
rather than filtered — the same actions, in the same order, with the same
semantics and the same `--dry-run`.

The enable and disable commands write machine-local state rather than anything in
the home directory, and `sync` acts on it: an action or group recorded in
[`disabled.toml`](state.md) is passed over.

`vars set`, `vars get`, and `vars unset` maintain the other machine-local
document, [`vars.toml`](state.md#varstoml-machine-local-variables). Every
command that executes actions merges it with the repository's `[vars]`,
`BATFILES_VAR_*`, and `--var` into one [effective
set](environment.md#variable-precedence), and `-vv` prints what that came to.
That set is what a record's [`when` or
`unless`](repoformat.md#conditions) is decided against, which is the only thing
that reads a variable.

An option any command accepts but does not honor yet fails rather than being
ignored, ahead of everything else the command would do — see
[unimplemented options](#unimplemented-options).

## Command Overview

```text
batfiles [global-options] <command> [command-options]

Commands:
  init
  version

  clone
  sync

  disable-action
  enable-action
  disable-group
  enable-group

  apply-action
  apply-group

  vars set
  vars get
  vars list
  vars unset
  vars refresh
```

Global options may appear before or after the command name.

## Global Options

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
| `--cache-dir <path>`            | Select the directory containing `dynamic-vars.toml`. Defaults to the XDG cache location.                          |

`--color` and the four location options have their full effect. `--verbose` at
one level prints the resolved roots; the `repository:` line appears only for a
command that reads one. It also prints the destinations `sync` left alone
because they were already correct. A second level, `-vv`, adds the [effective
variables](environment.md#variable-precedence) a run resolved. `--quiet`
suppresses the lines saying what `sync` did, and nothing else.

Three of the four resolved roots are live. `sync` and the two apply commands
read the leaf repository, [`disabled.toml`](state.md), and
[`vars.toml`](state.md#varstoml-machine-local-variables), and write into the
selected home; the enable and disable commands read and rewrite `disabled.toml`
under the config directory, and the three machine-local variable commands do the
same for `vars.toml` beside it. Neither kind touches the repository or the home.
Nothing reads or writes anything under the cache directory yet. See
[location selection](environment.md#location-selection) for the precedence, and
run a command with `-v` to see what it selected.

## Shared Action Execution Options

These options are accepted by every command that executes actions: `sync`,
`apply-action`, `apply-group`, and `clone`, which forwards them to its follow-up
synchronization. One of them is honored so far; the rest are
[refused for now](#unimplemented-options).

| Option              | Purpose                                                                    |
|---------------------|-----------------------------------------------------------------------------|
| `--var <key=value>` | Set a one-shot variable. Repeatable; the last value for a key wins.        |

A `--var` key must be a valid [user-variable name](repoformat.md#names-and-ids).
An invalid one fails the command as a usage error, before the location roots are
resolved and before any file is read. An invalid `BATFILES_VAR_*` name is
deliberately treated differently — it warns and is dropped — for the reason
given with [one-shot
variables](environment.md#one-shot-variables-batfiles_var_name).

An empty value is significant: `--var profile=` sets `profile` to the empty
string, which is a value like any other and overrides the layers below it. Where
a `--var` sits relative to the other three layers is
[variable precedence](environment.md#variable-precedence).

Variables exist only to feed `when` and `unless` conditions, and no command
evaluates one yet, so a `--var` changes nothing about what a run installs.
`-vv` prints the set it produced.

## Output Streams

Every command follows one rule for where its output goes:

- **Standard output** carries requested data: the value a command was asked
  for, and nothing else. It is never suppressed by `--quiet`, because `--quiet`
  suppresses what a command *did*, not what it was *asked for*. Data lines
  carry no label and no color, so `$(batfiles vars get editor)` yields the
  value alone.
- **Standard error** carries everything else: errors, warnings, progress, the
  lines describing what a command did, and any interactive prompt. `--quiet`
  suppresses the informational lines while leaving warnings and errors; `-v`
  adds detail.

Most commands produce no requested data at all and therefore write nothing to
standard output. `version` and `vars get` are the two that do today; `vars list`
joins them when it is built.

Errors and warnings batfiles raises itself are labeled `error:` and `warning:`,
and the label alone is colored when color is enabled — bold red and bold yellow
respectively. Detail added by `-v` is unlabeled: it elaborates on what a command
is doing rather than reporting a problem. Nothing on standard output is ever
labeled or colored.

## Commands

### `version`

```text
batfiles version
```

Print the batfiles version to standard output and exit successfully. This
command does not resolve the selected repository, home, config, or cache
directories.

`batfiles version` and `batfiles --version` print the same line, because the
command renders the same string clap renders for the flag.

### `sync`

```text
batfiles sync [--dry-run] [--skip-action <id>]... [--skip-group <group>]...
```

Read the leaf repository's [manifest](repoformat.md#reading-the-manifest) and
execute the actions it selects, in declaration order, each one inspecting the
filesystem as the previous one left it. The first failure stops the run; what
earlier actions did stays done, and nothing is rolled back.

Not every action in the manifest is one of them: a run passes over what is
disabled or skipped — see
[selecting what a run does](#selecting-what-a-run-does). The
[`disabled.toml`](state.md) lists are read along with the manifest, before the
first action, so a malformed one fails the run rather than being passed over.

One line on standard error names each change made — one per link, so an action
that installs several names each of them. A destination that was already
correct says nothing, because a repository that is already installed is the
ordinary case and forty lines of "unchanged" is how output stops being read;
`-v` reports those too. `--quiet` suppresses both.

Those lines name paths rather than records, so `-v` also prints a heading before
each action, saying what kind it is, what it is called, and which
[group](repoformat.md#groups) it is in:

```text
symlink zshrc (group shell)
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
symlink-dir action 7 (group shell)
linked /home/you/.ackrc -> /home/you/dotfiles/files/ackrc
```

An action with no `id` is named by its one-based position in the manifest, which
is how a load error names one too, so a heading and a diagnostic point at the
same record by the same words. An action with no group ends after its name.

| Option                 | Purpose                                                                |
|------------------------|--------------------------------------------------------------------------|
| `--dry-run`            | Report the action plan without executing it — see [dry-run behavior](#dry-run-behavior). |
| `--skip-action <id>`   | Leave one action out of this run. Repeatable.                            |
| `--skip-group <group>` | Leave every action in one group out of this run. Repeatable.             |

Every other option `sync` accepts is [refused for now](#unimplemented-options);
that list shrinking to empty is how you know `sync` is finished.

### `apply-action`

```text
batfiles apply-action --id <id> [--dry-run]
```

Carry out the one action carrying `--id`, with the same semantics `sync` gives
it: the same inspection, the same decision at the destination, the same reported
lines. An action written with no `id` cannot be named, and is reachable only
through its [group](repoformat.md#groups).

**Naming one action waives every reason it would otherwise be passed over** —
both [`disabled.toml`](state.md) lists, the run-only skips, which is why the
command accepts neither `--skip-action` nor `--skip-group` and ignores
`BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS`, and the record's own
[condition](repoformat.md#conditions). `disable-action` records that an action is
not part of an ordinary `sync`; asking for it by name is the way to say otherwise
for one invocation, without editing what the next `sync` does.

The condition is waived rather than merely satisfied, so it is not evaluated at
all: `apply-action` reaches its record even where the same condition [cannot be
evaluated](repoformat.md#when-a-condition-cannot-be-evaluated) and a `sync` over
the same manifest passes the record over. Nothing is finer-grained than the one
record the command was given, which is the same rule the two lists are waived
under.

An `--id` that no action answers to is a failure: the command resolved nothing,
so it exits 1 naming the address and the manifest, and writes nothing. That
covers a well-formed [address](#addresses) no action carries, qualified or not.
A value that is not a valid address fails the same way, before the repository is
opened.

| Option      | Purpose                                                                    |
|-------------|------------------------------------------------------------------------------|
| `--id <id>` | Required. The [address](#addresses) of the action to carry out.              |
| `--dry-run` | Report what it would do — see [dry-run behavior](#dry-run-behavior).         |

### `apply-group`

```text
batfiles apply-group --group <group> [--skip-action <id>]... [--dry-run]
```

Carry out the actions naming `--group`, in declaration order, with the same
semantics `sync` gives them. A [group](repoformat.md#groups) is nothing but the
actions naming it, so a group no action names does not exist: it exits 1 naming
the group, exactly as `apply-action` does for an unknown ID, and there is no
separate empty-group case to succeed quietly over.

**Naming the group waives the group's own disable and nothing else.** A
`disable-group` that would have kept these actions out of a `sync` does not keep
them out here, while an action disabled by its own `id` is still passed over, and
so is one named by `--skip-action` or `BATFILES_SKIP_ACTIONS`, and so is one its
own [condition](repoformat.md#conditions) closes. The rule is that an explicit
request waives the exclusions naming *what was asked for*; an exclusion naming
something more specific still applies, and a condition is written on one record.

`--skip-group` is not accepted and `BATFILES_SKIP_GROUPS` is ignored: the command
has already named the group it is applying, and a group skip could only
contradict that.

Where every action in the group is passed over, the run says so in one line and
exits 0 — the group exists and the command did what was asked. Which record was
passed over, and why, is `-v` detail as it is in a `sync`.

```console
$ batfiles apply-group --group shell --skip-action aliases
nothing to apply: every action in the group is disabled, skipped, or excluded by its own condition
```

| Option                 | Purpose                                                             |
|------------------------|-----------------------------------------------------------------------|
| `--group <group>`      | Required. The group whose actions are carried out.                    |
| `--skip-action <id>`   | Leave one action of that group out of this run. Repeatable.           |
| `--dry-run`            | Report what it would do — see [dry-run behavior](#dry-run-behavior).  |

Both commands read the leaf manifest and `disabled.toml` before carrying
anything out, the way `sync` does, so a document that is missing, malformed, or
invalid fails the command with the file named rather than partway through. That
holds for `apply-action` too, which waives both lists and reads them anyway: a
state file that cannot be read fails any command that executes actions.

Every other option they accept is [refused for now](#unimplemented-options).

### `vars set`

```text
batfiles vars set <key> <value>
```

Set one machine-local variable. The value is stored verbatim as a string, the
empty string included; `vars get`'s absent-key failure is what keeps an empty
value distinguishable from no value.

One line on standard error reports what the edit did:

```console
$ batfiles vars set editor nvim
set `editor`
```

The line names the key and never the value. A value may be a token or a path
that identifies a machine, and an informational line would put it in terminal
scrollback and in a calling script's logs. `vars get` is the way to read a value
back. The other two lines are ``changed `editor` (it had a different value)``
and ``` `editor` was already set to that value ```; the second changes nothing
and does not rewrite the document.

### `vars get`

```text
batfiles vars get <key>
```

Print the stored machine-local string for one variable on standard output. It
resolves nothing else: not the repository's `[vars]`, not the environment, not
host facts.

```console
$ batfiles vars get editor
nvim
```

A key with no machine-local value is a failure: nothing is written to standard
output, and the diagnostic naming the key goes to standard error. Printing an
empty line and exiting successfully would be indistinguishable from a key stored
as the empty string.

### `vars unset`

```text
batfiles vars unset <key>
```

Remove one machine-local variable. An absent key is an idempotent success,
reported as ``` `editor` was not set ```, and writes nothing at all — it creates
neither a `vars.toml` nor its directory. Removing the last key leaves an empty
`vars.toml` rather than deleting it.

### What the three of them share

They read and write [`vars.toml`](state.md#varstoml-machine-local-variables) and
nothing else. They do not load the leaf repository, so a `batfiles.toml` that is
malformed, or missing altogether, cannot fail one.

Each key is validated as a [variable
name](repoformat.md#names-and-ids) — which is not the rule an ID follows — and
an invalid one fails with status 1 before the document is opened. `--quiet`
suppresses the lines describing an edit and not the edit itself; it never
suppresses what `vars get` was asked for, which is the general rule for
[requested data](#output-streams).

What a stored value goes on to decide is a record's [`when` or
`unless`](repoformat.md#conditions), read at its place in the [variable
precedence](environment.md#variable-precedence). A manifest whose records carry
no condition is unaffected by anything these three commands do.

## Selecting What a Run Does

This is what a `sync` selects over, and it is the whole of it: a `sync` asks for
the manifest, so it honors everything either source names. The apply commands
run the same filter over the same list, minus the exclusions naming what they
were asked for — see [`apply-action`](#apply-action) and
[`apply-group`](#apply-group).

Three things say an action should be passed over, and a run honors all three:

- the machine-local [`disabled.toml`](state.md#disabledtoml-disabled-actions-and-groups)
  lists, which persist until an enable command or a hand edit removes the name;
- the run-only skips: `--skip-action` and `--skip-group`, together with
  [`BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS`](environment.md#run-only-skips),
  which apply to one invocation and are never written down; and
- the record's own [`when` or `unless`](repoformat.md#conditions), which is the
  repository saying the action does not belong on this machine.

The first two select over the same two namespaces. An action is named by its
`id`, and a [group](repoformat.md#groups) is named by the `group` field its
members carry — so an action written with no `id` can be left out only through
its group, and an action in no group only by its own name. The two namespaces are
separate, so `--skip-group zshrc` does not reach the action `zshrc`.

A condition names nothing and is not a list: it is a property of the record, read
against [the variables this run resolved](environment.md#variable-precedence).

The option and the variable **union** rather than one overriding the other, and
so do the run-only skips and the persistent lists. These are lists of what to
leave out, so anything any of them names is left out.

**A run-only name that matches nothing warns, and the run continues.** `sync`
has the manifest loaded, so unlike the [enable and disable
commands](#enable-and-disable-actions-or-groups) it can tell — and a name that
catches nothing is a typo in something typed for one run, which is worth saying
and not worth failing over. A name that is not a valid [address](#addresses)
warns the same way and for the same reason: it could never have matched.

```console
$ batfiles sync --skip-group editor --skip-action core..zshrc
warning: --skip-action: `core..zshrc` is not a valid address: every dot-separated segment must be an ID starting with a letter or digit, followed by letters, digits, hyphens, or underscores
warning: --skip-group `editor` matched no group
```

Both come before the first action, because they are complaints about the
invocation and a run that fails partway should not swallow them.

**A `disabled.toml` entry that matches nothing is silent.** Pre-registering a
name that a later branch or Git update introduces is the point of that document,
so the same non-match that warns above is expected there.

### Addresses

Everywhere an action or a group is named — `--skip-action`, `--skip-group`,
`apply-action --id`, `apply-group --group`, the four enable and disable
commands, `disabled.toml`, and `[default-disabled]` — the value is an *address*:
a nonempty list of [IDs](repoformat.md#names-and-ids) joined by `.`, with no
upper bound on the number of segments. Dots are the separators and are not part
of an individual ID.

Two forms resolve today:

| Form            | Meaning                                  |
|-----------------|------------------------------------------|
| `<action-id>`   | Top-level action in the leaf repository. |
| `<group>`       | Group in the leaf repository.            |

The remaining forms — an addressable entry inside a `git-clone-list`, and
anything qualified by the remote that contributed it — are in
[`future/cmdline.md`](future/cmdline.md#address-forms) with the slices that give
a dotted name something to refer to.

**Syntax and resolution are separate questions**, which is why a qualified
address is accepted now rather than waiting for remotes. A well-formed address
that no form above can resolve — `core.zshrc`, `a.b.c.d.e` — simply names
nothing, so a command reports that it was **not found** rather than that it was
malformed. Only a malformed address, one with an empty or non-ID segment, is
refused as a name. Commands that record an address without resolving it accept
any well-formed one.

```console
$ batfiles apply-action --id core.zshrc
error: no action in /home/you/dotfiles/batfiles.toml has the id `core.zshrc`
```

A skipped action is reported at `-v` **only**, as part of the heading naming the
record. Asking for a skip and then being told about it at normal verbosity is
noise; `-v` is where the whole account of a run lives.

```text
create-dir zsh-cache (group shell) - skipped: group `shell` is disabled
symlink zshrc (group shell) - skipped: `zshrc` from --skip-action
symlink gitconfig-work (group git) - skipped: when "work" is false
copy profile
copied /home/you/.profile
```

The reason names the source the reader can go and change, which is why it spells
out the option or the variable rather than saying only that a skip applied. A
condition names itself instead, in the spelling the record used — `unless "gui"
is true` rather than the verdict alone, because `unless` is the one a reader gets
backwards.

**A condition that [cannot be
evaluated](repoformat.md#when-a-condition-cannot-be-evaluated) is the one
exception**, and it is a warning rather than detail: it closes the gate like any
other reason, but nobody asked for it, so it is printed at every verbosity and
without the `- skipped:` frame, the reason having already said what is not
happening.

```text
warning: symlink gitconfig-work (group git): when "work" cannot be evaluated, so it is not installed: `work` is not declared. Add ...
```

When more than one reason applies, one is reported, in this order: a disable
ahead of a run-only skip, because it is the one still in force tomorrow when the
skip is gone; the action's own name ahead of its group's, because it is the more
specific of the two; and the condition last of all. Last means the condition is
not evaluated at all for a record something else already excludes, which is why a
condition that [cannot be
evaluated](repoformat.md#when-a-condition-cannot-be-evaluated) costs only the
runs that would otherwise have carried the record out.

### Enable and disable actions or groups

```text
batfiles disable-action <id>...
batfiles enable-action <id>...
batfiles disable-group <group>...
batfiles enable-group <group>...
```

Persistently add one or more action IDs or group names to, or remove them from,
the machine-local disabled lists. These commands run no synchronization and
remove no installed content: what they change is what the *next* `sync` does,
which is to pass the recorded names over — see
[selecting what a run does](#selecting-what-a-run-does).

They read and write `disabled.toml` and nothing else. They do not load
the leaf repository, so a `batfiles.toml` that is malformed, or missing
altogether, cannot fail one — and each supplied name is validated for
[syntax](repoformat.md#names-and-ids) alone. A name matching nothing in the
manifest is recorded without complaint: these commands resolve nothing, and
pre-registering a name a later branch introduces is supported.

One line on standard error reports each name, saying whether the state actually
moved:

```console
$ batfiles disable-action p10k zshrc
disabled action `p10k`
action `zshrc` was already disabled
```

`--quiet` suppresses those lines and not the edit. A repeated name warns and is
applied once. A name that is not a valid ID fails the whole invocation with
status 1, before the document is opened, so the other names on the command line
are not applied either. The
[lifecycle rules](state.md#semantics-and-lifecycle) are specified with the
document.

## Dry-Run Behavior

Every action inspects the real filesystem as the previous action left it,
decides what to do, and then either does it or, under `--dry-run`, says what it
would have done. That is the whole mechanism: one pass per action, with no
separate planning phase and nothing forecast. A dry run reports the first
action's decision exactly, and each later action's as though its predecessors
had not run — which is accurate for the overwhelmingly common case of actions
with distinct destinations, and wrong only where one action's output is
another's input.

**Batfiles does not simulate a filesystem to close that gap.** A shadow
filesystem layered over the real starting state would have to model creation,
replacement, permissions, and symlink traversal, and every divergence between
the model and the real implementation is a dry run that lies.

**A dry run does not do the work.** Nothing is created, replaced, or removed.
That is a promise about the plan and not about a directory: batfiles' own
bookkeeping runs in both modes, so a dry run is not a promise that the process
writes nothing anywhere — it is a promise that none of the plan it prints is
carried out. "Nothing under the home changes" would be both weaker and false,
since the repository can default to `<selected-home>/dotfiles`.

A dry run's lines are the real run's lines in a different tense: `would link`
and `would copy` where a real run reports `linked` and `copied`. Order and
granularity match too — one line per child for the directory-wide actions, and
one line for a whole tree where a real run installs one. A skipped action is the
one line that takes no tense at all: it reports a decision rather than an act, so
it reads the same in both modes, and the two runs pass over exactly the same
actions.

**Tense is the only difference where actions have distinct destinations.** Where
one action's output is another's input the runs differ in substance, for the
reason above: a `create-dir` followed by a `copy` at the same path reports
`would create` and then `would copy`, where a real run reports `created` and
then `kept`, the second action having seen the first one's work.

One exception has the same cause: a dry run removes nothing, so a broken symlink
that a real run clears once is rediscovered by everything that looks at that
path afterwards and reported each time. A `symlink-dir` or `copy-dir` whose
destination directory is itself a broken symlink therefore says so once per
child, where a real run says it once.

**A dry run describes intent, not success.** It stops before the write, so a
permission failure or a destination another process takes first appears only in
the real run. A mismatched digest belongs with them: a fetching action says what
it would fetch and where it would land without contacting the network, so
whether the bytes are the ones the manifest names is not something it can know.
A `fetch-archive` reports at the same granularity a whole-directory `copy` does
— what it would install and where it came from, not a list of entries, which it
could not know without unpacking the archive it did not fetch. Every decision
resting on inspection is still exact: a `copy` whose destination is already
occupied reports that it would keep what is there, and copies nothing, and so
does either fetching action, which asks nothing of the server either.

**No `git` runs under `--dry-run`, at all.** `git-clone` reports that it would
clone the repository into a destination nothing is at, or that it would update
the clone at a destination holding a directory, and it runs no git to decide
which. So nothing reaches the network and no checkout is touched, not even by
the read-only commands that would tell batfiles whether an update is possible.

`git-clone-list` says one line per entry on the same terms. Its list is a file
in the repository, read as the repository is loaded and therefore in hand in
both modes, so a dry run describes every entry from a document it already
holds rather than from anything it goes and asks for: it would clone the ones
whose directories are empty and update the ones already holding something,
whichever mixture a list happens to be in. A declared `ref` is reported as the
list writes it, since nothing resolved it.

What is *at* a destination is a filesystem question, so it is answered the same
in both modes: a dry run refuses a `dest` holding a regular file or a symlink
out of the repository exactly as a real run does. What it gives up is one
distinction — telling a clone from a plain directory takes git, so a dry run
says it would update either, and the real run is where the second is refused.
That is the same "intent, not success" boundary the fetching actions sit on.

The alternative — a dry run that fetched, so its report could say what an update
would actually bring — would make `--dry-run` a command that reaches the network
and modifies a checkout it was asked only to describe, and doing neither of those
is the whole value of the flag. To see the current picture, update the clone and
run it again.

What the manifest names is still read. A `source` that is missing or unreadable
fails in either mode, before the destination is considered — including where the
destination is occupied and a real run would have kept it. A manifest naming a
source that is not there is a repository error, and reporting it only on the day
the destination happens to be empty would be the less useful behavior. A
destination batfiles will not install over is refused in either mode too, for
the same reason: the refusal is a decision, and inspection is what decides it.

## Unimplemented Options

The whole option set parses from the first release, so the shape of each command
is visible before the command works. An option that parses but is not honored
yet is **refused, never ignored**: the run exits 2 naming the option and the
step that makes it live, before any root is resolved or any file is opened.
Silently accepting it would be worse than not accepting it at all, because
nothing would have happened and it would look as though something had.

The refusal comes ahead of a command's own not-implemented message, so
`batfiles clone <url> --interactive` reports `--interactive` rather than
`clone`. The option is the part of the invocation that would still be wrong once
the command exists.

| Command                       | Options refused for now                                                                                                                    |
|-------------------------------|--------------------------------------------------------------------------------------------------------------------------------------------|
| `sync`                        | `--refresh-remotes`, `--refresh-vars`, `--refresh-content`, `--no-overwrite`, `--interactive` |
| `clone`                       | `sync`'s list plus `--enable-action`, `--disable-action`, `--enable-group`, `--disable-group`. `clone` accepts neither `--dry-run` nor `--refresh-remotes` at all |
| `apply-action`, `apply-group` | `--refresh-vars`, `--refresh-content`, `--no-overwrite`, `--interactive`                                              |
| `vars list`                   | `--no-refresh`                                                                                                                             |
| everything else               | none                                                                                                                                       |

An option that arrives together with the command that takes it is not listed —
`init`'s `--no-git-init` and `vars list`'s `--machine-only` — because the
command's own not-implemented message already covers it. Neither is an option
that is live elsewhere and is waiting only on the command: `clone
--skip-group gui` reports `clone`, because `--skip-group` is not the part of
that invocation batfiles cannot do yet.

## Exit Statuses

| Status | Meaning                                                                 |
|--------|---------------------------------------------------------------------------|
| `0`    | The command did what was asked. `--help` and `--version` exit here too.   |
| `1`    | The command ran and failed.                                               |
| `2`    | The command did not run: the invocation was wrong or is not supported yet.|

The distinction that matters is between 1 and 2. A status of 1 means batfiles
started doing the work and something went wrong partway, so the filesystem may
have been touched. A status of 2 means nothing was attempted — a usage error, or
a command or option batfiles does not implement yet — so nothing was read or
written and the invocation can be corrected and retried freely.

Status 2 is what clap already uses for the usage errors it renders, and an
unimplemented command or option joins it rather than reporting a failure it
never had.

Status 1 covers a command that needs a home directory and cannot determine one,
a `sync` whose leaf `batfiles.toml` is missing, malformed, or invalid, or whose
`disabled.toml` is malformed, a `vars get` naming a variable this machine has no
value for, an argument that is not a well-formed address or variable name, and an
action that could not be carried out — a source the repository does not contain,
a destination holding something batfiles will not replace, or a write the
operating system refused. In each case the invocation was well-formed and
something outside it did not hold up.

All but the last of those happen before anything is written; the last may not.
`sync` stops at the first action that fails, so an earlier action's symlink is
still there. That is what status 1 means and status 2 does not: the filesystem
may have been touched, and the fix is to look rather than to retype the command.
