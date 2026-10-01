# Batfiles Command-Line Surface

The parts of the command-line interface that run today: the set of commands, the
options every command accepts, where output goes, and what the exit status
means.

Additional address forms are described in
[`future/cmdline.md`](future/cmdline.md).

## What runs today

The whole surface parses. Every command and option listed below is accepted, and
an invalid invocation is rejected as a usage error before anything else happens.

**Every command does its work, and every option it accepts is honored.** What
is not built yet is the behavior [`future/cmdline.md`](future/cmdline.md)
describes.

[`init`](#init) lays the conventional layout into the current directory and puts
a Git repository around it. It resolves no roots at all, and what it creates is
a repository `sync` can read: a freshly initialized one installs nothing, because
every sample in the starter manifest is commented out.

[`clone`](#clone) is the other end of that: it brings an existing repository
down onto a machine that has none, adopts the [bootstrap
policy](repoformat.md#default-disabled-bootstrap-entries) that repository
declares, and synchronizes it — all in one command. What the machine starts with
switched off is settled from the leaf's candidates, the four `BATFILES_*`
bootstrap lists, and `clone`'s own enable and disable options, and is written to
[`disabled.toml`](state.md) before the first action.

`sync` materializes the [remotes](repoformat.md#materialization) the manifest
declares and executes every action type, `--dry-run` reports what it would do
without doing any of it, and `--skip-action`/`--skip-group` leave part of it out
for one run. [`include-remote`](repoformat.md#include-remote) contributes the
actions of the remote it names, at its own position in the list, and they run
like any other. An inclusion it could not read at all warns, which is what leaves
a [plan partial](#plan-completeness).

`apply-action` and `apply-group` carry out part of the same manifest, named
rather than filtered — the same actions, in the same order, with the same
semantics and the same `--dry-run`.

The enable and disable commands write machine-local state rather than anything in
the home directory, and `sync` acts on it: an action or group recorded in
[`disabled.toml`](state.md) is passed over. They are one of the document's two
writers; `clone`'s bootstrap, above, is the other.

`vars set`, `vars get`, and `vars unset` maintain the other machine-local
document, [`vars.toml`](state.md#varstoml-machine-local-variables). Every
command that executes actions merges it with the repository's `[vars]` — running
the [dynamic variables](repoformat.md#dynamic-variables) there, or reusing what
[`dynamic-vars.toml`](state.md#dynamic-varstoml-dynamic-variable-cache) cached —
`BATFILES_VAR_*`, and `--var` into one [effective
set](environment.md#variable-precedence), and `-vv` prints what that came to.
`vars list` asks for that set on its own. It is what a record's [`when` or
`unless`](repoformat.md#conditions) is decided against, which is the only thing
that reads a variable. [`vars refresh`](#vars-refresh) runs the dynamic
variables' commands ahead of a run, whatever the cache holds.

[`update`](#update) replaces the running binary with another release, when the
user asks and only then; like `init` and `version`, it resolves no roots.

An option a command accepted but did not honor yet would fail rather than be
ignored, ahead of everything else the command would do — see
[unimplemented options](#unimplemented-options).

## Command Overview

```text
batfiles [global-options] <command> [command-options]

Commands:
  init
  version
  update

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
one level prints the roots that command resolved, which is not always all four:
the `repository:` and `home:` lines appear only for a command that reads the
leaf repository, as [location selection](environment.md#location-selection)
specifies. It also prints the destinations `sync` left alone because they were
already correct. A second level, `-vv`, adds the [effective
variable listing](#vars-list) a run resolved. `--quiet`
suppresses the lines saying what `sync` did, and nothing else.

All four resolved roots are live. `sync` and the two apply commands read the
leaf repository, [`disabled.toml`](state.md), and
[`vars.toml`](state.md#varstoml-machine-local-variables), and write into the
selected home; `clone` writes the leaf repository and `disabled.toml` before
doing all of that; `vars list` reads the first and the last of those, and
`vars refresh` all three. Each of them also reads and writes
[`dynamic-vars.toml`](state.md#dynamic-varstoml-dynamic-variable-cache) under
the cache directory, when the manifest declares a dynamic variable. The enable
and disable commands read and rewrite `disabled.toml` under the config
directory, and the machine-local variable commands do the same for `vars.toml`
beside it. Neither kind resolves the repository or the home at all. See
[location selection](environment.md#location-selection) for the precedence, and
run a command with `-v` to see what it selected.

## Shared Action Execution Options

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

Variables exist only to feed `when` and `unless` conditions, and `-vv` prints
the set a run produced.

`--refresh-vars` changes which [dynamic
variables](repoformat.md#dynamic-variables) run, not which are evaluated: the
leaf's, and each opened inclusion's whose remote [allows](repoformat.md#git)
them, run whatever the [cache](state.md#freshness-and-refresh-behavior) holds,
and what they capture is written back.

**Something already in a destination's way is backed up and replaced**, unless
`--no-overwrite` or `--interactive` says otherwise. [Conflicts and
backups](safety.md#conflicts-and-backups) specifies what counts as in the way,
where a backup goes, and how each choice is reported; `--no-overwrite` and
`--interactive` cannot be combined, and `--interactive` cannot be combined with
`--dry-run`. A backup is reported before the line for what replaced it:

```text
backed up /home/you/.gitconfig to /home/you/.gitconfig.batfiles-backup-20260929T142233Z
linked /home/you/.gitconfig -> /home/you/dotfiles/git/gitconfig
```

`--refresh-content` [refreshes seeds](safety.md#refreshing-seeds): `copy`,
`copy-dir`, `fetch-file`, and `fetch-archive` build their content again, leave
it alone where it already matches, and otherwise settle what is there as a
conflict and report `refreshed <dest> from <origin>`. It leaves every other
action as it is.

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
standard output. `version`, `vars get`, and `vars list` are the three that do
today.

Errors and warnings batfiles raises itself are labeled `error:` and `warning:`,
and the label alone is colored when color is enabled — bold red and bold yellow
respectively. Detail added by `-v` is unlabeled: it elaborates on what a command
is doing rather than reporting a problem. Nothing on standard output is ever
labeled or colored.

A [dynamic variable](repoformat.md#dynamic-variables)'s command writes its own
standard error straight to batfiles', unlabeled, rather than through batfiles;
`--quiet` disconnects it instead of leaving the noisiest output on an otherwise
quiet channel, and batfiles' own warning about a failed capture remains. Its
standard output never reaches batfiles' standard output. See [how dynamic
commands are run](environment.md#how-dynamic-commands-are-run).

## Commands

### `init`

```text
batfiles init [--no-git-init]
```

Lay the conventional leaf-repository layout into the current directory, without
overwriting anything already there. `init` works on that directory alone and
resolves none of the four roots, so `--batfiles-dir` and the rest have no effect
on where the skeleton lands.

| Option          | Purpose                                                                                                   |
|-----------------|-----------------------------------------------------------------------------------------------------------|
| `--no-git-init` | Do not run `git init`. Batfiles also skips `git init` automatically when already inside a Git repository. |

The layout is `batfiles.toml`, `.gitignore`, `bin/`, `files/`, and the
executable [leaf stub](distribution.md#leaf-stub) `install.sh`, created in that
order. The stub carries the [release base](distribution.md#the-release-base):
`BATFILES_BASE` when it is set and not empty, otherwise the base this build was
released from. A base that is not a URL made of the characters the installers
quote fails `init` before it creates anything. The generated [`remotes/`](repoformat.md#materialization) tree is
not created; the written `.gitignore` excludes it instead. An existing path of
the expected kind is left exactly as it is, including its contents and
permissions, and is not named in the line reporting what was created.

The starter `batfiles.toml` is valid and installs nothing: every sample in it is
commented out, so a `sync` in a freshly initialized repository does no work
until its owner edits the file.

`init` refuses to run, before creating anything, when:

- The current directory already contains anything named `batfiles.toml`,
  whatever kind of filesystem node it is. The directory is already a batfiles
  repository, and `init` is not a repair path for one.
- The current directory is the invoking user's OS home directory — the home the
  operating system reports, never one selected with `--home-dir`. Only that
  directory is refused; a directory below it, such as the default `~/dotfiles`,
  is the normal case. A home that cannot be determined is not fatal here,
  because `init` needs one only for this check.
- A path of the conventional layout exists as the wrong kind of filesystem
  node, such as a regular file named `files`. Symlinks are judged by what they
  point at, and a symlink pointing at nothing is a wrong kind too.

`init` also fails when Git initialization was requested and `git` could not be
run or `git init` failed. Whatever was already created stays; `init` does not
unwind a partial layout. Use `--no-git-init` to initialize without Git.

A `.gitignore` that was already there and does not appear to cover the
`remotes/` tree is reported as a warning. The file belongs to the repository's
owner, so `init` does not edit one it did not write. An `install.sh` that was
already there and is not a batfiles stub is reported the same way, and left
alone.

Everything `init` prints is a diagnostic on standard error. It produces no
requested data, so `--quiet` leaves only warnings and errors.

### `version`

```text
batfiles version
```

Print `batfiles <version>` to standard output and exit successfully. A
[release](distribution.md#versions) build reports its release version, including
any pre-release suffix; any other build reports the `Cargo.toml` version. This
command does not resolve the selected repository, home, config, or cache
directories.

`batfiles version` and `batfiles --version` print the same line.

### `update`

```text
batfiles update [<version>] [--check]
```

Replace the running binary with the latest [release](distribution.md#release-tree),
or with `<version>`. The user runs it; batfiles never runs it, and no other
command checks for updates. Like `version`, it resolves none of the four roots.

| Argument or option | Purpose |
|--------------------|---------|
| `<version>`        | The release to install: a [version](distribution.md#versions), with or without a leading `v`, or `latest`, the default. One outside the grammar is a usage error. |
| `--check`          | Print what is running and what is available, and install nothing. |

Releases come from the [release base](distribution.md#the-release-base):
`BATFILES_BASE` when it is set and not empty, otherwise the base this build was
released from. A self-hoster sets `BATFILES_BASE` in the shell environment their
own dotfiles install.

With no `<version>`, `update` reads `<base>/latest/download/VERSION` once. A
latest release that is not newer than the running one, by [version
order](distribution.md#versions), is reported and nothing is downloaded, so a
pre-release stays until a stable release passes it. When `VERSION` cannot be
read, the error suggests naming a release, since the base may have published
only pre-releases. A named `<version>` is installed whatever the running
version, older or the same, which also repairs a binary.

Everything else comes from `<base>/download/v<version>/`:

1. Create a private file beside the running executable, with symlinks resolved,
   so a link to batfiles stays a link and what it points at is replaced. A
   directory this user cannot write fails here, before anything is downloaded,
   naming it. Something already at that path, such as the file an interrupted
   update left, is never replaced; remove it and run `update` again.
2. Download `SHA256SUMS`, and the binary it lists for this build's
   [asset](distribution.md#targets) into that file, verifying its digest.
3. Make it executable and run its `version`, which must report the release.
4. Rename it over the running executable, and report both versions and the
   path.

A failure at any step removes the file and leaves the running binary as it
was. Replacing a running executable on Windows is not built yet: `update` fails
there before downloading anything, and `--check` works.

`--check` downloads only the one `VERSION` it needs — `latest/download/` with
no `<version>`, that release's own otherwise, which must hold the version
named — and prints two lines of requested data to standard output:

```text
running 1.2.0
available 1.3.0
```

It succeeds whether or not the available release is newer; a `VERSION` that
cannot be read or holds something other than a version is a failure. Without
`--check`, everything `update` prints is a diagnostic on standard error.

### `clone`

```text
batfiles clone <url> [--skip-action <id>]... [--skip-group <group>]...
    [--enable-action <id>]... [--disable-action <id>]...
    [--enable-group <group>]... [--disable-group <group>]...
```

Clone a leaf repository into the selected batfiles directory, settle what this
machine starts with switched off, and [synchronize](#sync) it: how a machine
with no repository gets one. The three are one command and one failure — nothing
is installed unless the clone arrived, and what happens to what arrived is
exactly what `sync` would do with it.

Where the clone lands is the one thing `clone` decides differently from every
other command that has a leaf repository at all: working-directory discovery
does not apply. A `batfiles.toml` in the current directory is what selects a
repository to *read*, and this command is creating one, so only `--batfiles-dir`,
`BATFILES_DIR`, and the `<selected-home>/dotfiles` default have any bearing on
it. See [location selection](environment.md#location-selection).

**The destination must not exist at all.** Anything there — a file, a link to
nothing, even the empty directory `git clone` itself would accept — refuses the
command before Git is launched, with nothing downloaded and nothing installed.
Batfiles creates the repository directory, and a directory it did not create is
not one it writes a repository into. Missing directories above the destination
are created with it.

| Option                    | Purpose                                                        |
|---------------------------|----------------------------------------------------------------|
| `--skip-action <id>`      | Leave one action out of the synchronization. Repeatable.       |
| `--skip-group <group>`    | Leave one group out of the synchronization. Repeatable.        |
| `--disable-action <id>`   | Start this machine with one action switched off. Repeatable.   |
| `--enable-action <id>`    | Start it with one action switched on. Repeatable.              |
| `--disable-group <group>` | Start this machine with one group switched off. Repeatable.    |
| `--enable-group <group>`  | Start it with one group switched on. Repeatable.               |

`--var` reaches the synchronization the same way, as do the rest of the [shared
action-execution options](#shared-action-execution-options). `clone` accepts neither `--dry-run` nor `--refresh-remotes` at all,
rather than refusing them for now: a machine with no repository has no plan to
describe, and a fresh clone materializes its remotes during the synchronization
that follows. Use `sync --dry-run` afterwards to inspect later plans.

The two halves of the table are different in kind. A skip leaves something out
of *this run*; an enable or disable decides what this *machine* starts with and
is written to [`disabled.toml`](state.md), where it stands until an enable or
disable command changes it.

#### What the bootstrap decides

Before the first action, `clone` settles the machine-local lists from the leaf's
[default-disabled candidates](repoformat.md#default-disabled-bootstrap-entries),
the four `BATFILES_*` bootstrap variables, and the four options above, in the
[adoption precedence](environment.md#bootstrap-adoption-precedence) the
environment reference owns. Only the leaf's candidates are read: bootstrap
policy belongs to the repository this machine was pointed at, so an included
remote's own section is passed over like its `[remotes]`.

A candidate's `when` or `unless` is decided here, against the same [effective
variable set](environment.md#variable-precedence) the run uses. A condition this
machine cannot decide closes the gate, as everywhere else, so the candidate is
not offered and what it names is left enabled — with a warning saying so.

Each decision says what it did and what asked for it, and the synchronization
that follows passes over what was just switched off:

```text
cloned /home/you/dotfiles from git@github.com:me/dotfiles.git
default-disabled: disabled action `p10k`
BATFILES_DISABLE_GROUPS: disabled group `gui`
--enable-group: enabled group `gui` (was disabled)
symlink zshrc (group shell)
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
```

Each line leads with what asked for the decision, and says what the decision did
in the words the [enable and disable
commands](#enable-and-disable-actions-or-groups) use for the same document.

A candidate a condition simply closed is reported at `-v`, like every other
expected exclusion.

Once a clone succeeds nothing is unwound. The repository is kept whether or not
the rest of the command gets anywhere with it:

- A repository holding no `batfiles.toml` fails before the bootstrap starts,
  saying that what was cloned is a Git repository but not a batfiles one. The
  mistake is the URL rather than a missing file.
- A synchronization that fails, fails the way `sync` fails — see [execution
  failures](#execution-failures). The clone stays where it landed and so does
  the state the bootstrap wrote, so the fix is an edit and a `sync` rather than
  a second download.

A malformed address given to one of the four options fails the command before
anything is cloned. A malformed one in a `BATFILES_*` list warns and is dropped,
and the rest of that list still applies.

### `sync`

```text
batfiles sync [--dry-run | --refresh-remotes] [--skip-action <id>]... [--skip-group <group>]...
    [--bootstrap [--enable-action <id>]... [--disable-action <id>]...
                 [--enable-group <group>]... [--disable-group <group>]...]
```

Read the leaf repository's [manifest](repoformat.md#reading-the-manifest) and
execute the actions it selects, in declaration order, each one inspecting the
filesystem as the previous one left it. See [execution failures](#execution-failures)
for stopping behavior, clone-list exceptions, and exit status.

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

Before any of them, `sync` [materializes](repoformat.md#materialization) every
[remote](repoformat.md#remotes) the manifest declares: it clones a Git remote
that is missing and updates one that is there, and fetches a file or archive
remote that is missing or whose declaration has changed. Those lines come first,
under a heading naming the record they belong to. A Git remote's are the lines a
`git-clone` action prints, and a file or archive remote's the lines a fetching
action does, with `refetched` where one replaces an earlier fetch:

```text
remote core
cloned /home/you/dotfiles/remotes/core from git@github.com:me/dotfiles-core.git
remote fzf
extracted /home/you/dotfiles/remotes/fzf from https://example.com/fzf-0.65.2.tar.gz
remote pathogen
refetched /home/you/dotfiles/remotes/pathogen from https://example.com/pathogen.vim
symlink zshrc (group shell)
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
```

A materialization that was already up to date says nothing, as an unchanged
destination does. A remote that cannot be cloned, updated, or fetched fails the
run before any action, so a `sync` that reaches its first action has every
declared remote it takes in place. Under `--dry-run` batfiles says what it would
clone, update, or fetch, and runs no git and fetches nothing at all, which is the
[dry-run rule](#dry-run-behavior) for every caller.

A remote whose own [condition](repoformat.md#a-remotes-condition) closes on this
machine is passed over instead, under the same heading and by the rules in
[exclusion reporting](#exclusion-reporting):

```text
remote core - skipped: when "work" is false
symlink zshrc (group shell)
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
```

Every command decides those conditions, though only `sync` acts on them: an
action installing from an excluded remote is refused by name wherever it runs.

`sync` is the only command that does this; see [what all three
load](#selection-by-command).

| Option                 | Purpose                                                                |
|------------------------|--------------------------------------------------------------------------|
| `--dry-run`            | Report the action plan without executing it — see [dry-run behavior](#dry-run-behavior). |
| `--refresh-remotes`    | Fetch every file and archive remote again, current or not.               |
| `--skip-action <id>`   | Leave one action out of this run. Repeatable.                            |
| `--skip-group <group>` | Leave every action in one group out of this run. Repeatable.             |
| `--bootstrap`          | First settle this machine's starting point, as `clone` does.             |

**`--bootstrap` makes this run the bootstrap of a checkout that arrived without
`clone`**, such as by `git clone`: before the first action, it decides exactly
what [`clone`'s bootstrap](#what-the-bootstrap-decides) decides, from the same
inputs, and writes the outcome to [`disabled.toml`](state.md) the same way. The
four bootstrap options — `--enable-action`, `--disable-action`,
`--enable-group`, and `--disable-group` — mean what they mean to `clone`, and
`sync` accepts them only with `--bootstrap`; without it, giving one is a usage
error. The repository's candidates are offered only to a machine with no
`disabled.toml`, so running `sync --bootstrap` again adds nothing they propose.
With `--dry-run` the bootstrap decides the same things, reports what it would
enable and disable, and the plan is made with that outcome, but nothing is
written. The [leaf stub](distribution.md#leaf-stub) runs `sync --bootstrap`.

**`--refresh-remotes` fetches every file and archive remote again**, including
one whose [stamp](repoformat.md#materialization) says it is current, and
replaces it as a changed declaration would be; it is how a remote whose URL
publishes new content under the same name is brought up to date. It excludes
nothing it would otherwise include: a remote its condition closes is still passed
over, and a `remotes/<id>` no stamp claims is still refused. Git remotes are
unaffected, since every `sync` updates them already. It cannot be combined with
`--dry-run`, which materializes nothing and so has nothing to refresh.

`sync` also takes the [shared action execution
options](#shared-action-execution-options).

### `apply-action`

```text
batfiles apply-action --id <id> [--dry-run]
```

Carry out the one action carrying `--id`, with the same semantics `sync` gives
it: the same inspection, the same decision at the destination, the same reported
lines. An action written with no `id` cannot be named, and is reachable only
through its [group](repoformat.md#groups).

Naming one action bypasses its exclusions for this invocation, without editing
what the next `sync` does. The [selection table](#selection-by-command) specifies
which lists and conditions each command consults.

An `--id` that no action answers to is a failure: the command resolved nothing,
so it exits 1 naming the address and the manifest, and writes nothing. That
covers a well-formed [address](#addresses) no action carries, qualified or not.
A value that is not a valid address fails the same way, before the repository is
opened. An `--id` inside an inclusion the run did not read — one this machine
[excludes](#selection-by-command), or one whose remote has not been
fetched — fails as well, naming the inclusion and why rather than an address
nothing carries.

**An `--id` naming an [`include-remote`](repoformat.md#include-remote) is
refused**, and refused as that rather than as an unknown action. The inclusion's
`id` is the prefix its included actions are addressed under; the record itself is
a position in the list, not something to carry out, so the address named the
wrong thing rather than nothing.

One thing leaves a named action uncarried out rather than failing: an inclusion's
[selection filters](repoformat.md#selecting-part-of-a-remote) left the record out,
which naming it does not waive. The record exists and answers to the address, so
the run says so in one line and exits 0, as `apply-group` does where its group
applies nothing. Which record it was, and why, is `-v` detail.

```console
$ batfiles apply-action --id corp.p10k
nothing to apply: the inclusion that contributed the action did not select it
```

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
separate empty-group case to succeed quietly over. A qualified group inside an
inclusion the run did not read fails the same way, naming the inclusion instead.

Naming a group bypasses group exclusions; each member's own exclusions still
apply. See the [selection table](#selection-by-command) for the complete rules.

Where the group applies no action, the run says so in one line and exits 0 — the
group exists and the command did what was asked. Which record was passed over,
and why, is `-v` detail as it is in a `sync`.

```console
$ batfiles apply-group --group shell --skip-action aliases
nothing to apply: every action in the group is disabled, skipped, excluded by its own condition, or was not contributed
```

The last of those is what an [inclusion](repoformat.md#include-remote) adds: a
group may hold one, and one that brought nothing in leaves the group with no
action to apply and none that was passed over. An inclusion is never itself an
applied action — it installs nothing, and what it contributed is counted as
itself.

| Option                 | Purpose                                                             |
|------------------------|-----------------------------------------------------------------------|
| `--group <group>`      | Required. The group whose actions are carried out.                    |
| `--skip-action <id>`   | Leave one action of that group out of this run. Repeatable.           |
| `--dry-run`            | Report what it would do — see [dry-run behavior](#dry-run-behavior).  |

Both also take the [shared action execution
options](#shared-action-execution-options).

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

### `vars list`

```text
batfiles vars list [--machine-only] [--no-refresh]
```

List the effective variables on standard output, one per line: the value in
force, the layer that decided it, and the layers it overrode.

```console
$ batfiles vars list
editor  = "nvim" (vars.toml; over batfiles.toml)
profile = "work" (vars.toml; over batfiles.toml)
rank    = "9" (BATFILES_VAR_*; over batfiles.toml)
```

Action commands at `-vv` use the same listing format, indented under a
`variables:` heading, and include any `--var` overrides supplied to that run:

```console
$ BATFILES_VAR_editor=code batfiles sync -vv --var editor=emacs
variables:
  editor  = "emacs" (--var; over BATFILES_VAR_*, vars.toml, batfiles.toml)
  profile = "work" (vars.toml; over batfiles.toml)
```

An [inclusion](repoformat.md#include-remote) reports a block of its own, named
for the inclusion and listing only the names its two layers declared — the
[`vars`](repoformat.md#variables-for-one-inclusion) written on the record, and
the [`[vars]`](repoformat.md#variables-an-included-remote-declares) the remote
declared for itself. The block appears where the inclusion was opened, as the
run's list is assembled, so it comes before the action lines rather than beside
the one that contributed a record:

```console
$ batfiles sync -vv
variables:
  profile = "personal" (batfiles.toml)
include-remote `corp` variables:
  editor  = "vim" (batfiles.toml of include-remote `corp`)
  profile = "work" (include-remote `corp`; over batfiles.toml)
include-remote corp
create-dir corp.work-tools
```

An origin reading `batfiles.toml of <inclusion>` is the included manifest's own
`[vars]`, which the bare document name would not tell from the leaf's or from a
second remote's.

A name a higher layer also declares is listed with the layer in force, so a
declaration that lost reads as having lost:

```text
profile = "lab" (vars.toml; over include-remote `corp`, batfiles.toml)
profile = "personal" (batfiles.toml; over batfiles.toml of include-remote `corp`)
```

An inclusion this run did not open reports no block, and neither does one whose
record overrode nothing and whose remote declared nothing. An inclusion written
without an `id` heads its block with [the name it does
have](repoformat.md#include-remote), so two inclusions of one remote report two
blocks rather than one ambiguous pair.

A [dynamic variable](repoformat.md#dynamic-variables) in force says how it
arrived, after its origin, and one with no value says so in place of a value:

```console
$ batfiles vars list
email  = "me@corp.example" (batfiles.toml, command)
has_op = "false" (batfiles.toml, command could not start)
shell  = "zsh" (batfiles.toml, cached 3h ago)
team   = "platform" (batfiles.toml, command failed, cached 2d ago)
token  = no value (batfiles.toml, command failed)
```

`command` means it ran now; `cached` gives the age of the cache entry used, in
its largest whole unit; `command failed, cached` is a failed command falling
back on that entry; `command could not start` is a status capture read as
`"false"` for this run. A declaration a higher layer overrides is listed as that
layer's line says, with no state of its own.

Both listings explicitly print values; mutation reports name only the key to
avoid disclosing values incidentally. `vars get` supplies a bare persisted value
when that is what a script needs.

The listing resolves the leaf repository's `[vars]`, `vars.toml`, and
`BATFILES_VAR_*` using the [variable precedence](environment.md#variable-precedence).
`vars list` does not accept `--var`. It does not list the `facts.*` or `env.*`
namespaces; those are accessed through [conditions](repoformat.md#conditions).
It also does not list what an
[inclusion](repoformat.md#variables-for-one-inclusion) overrides or what an
[included remote declares](repoformat.md#variables-an-included-remote-declares):
those values hold inside one inclusion's records rather than in the set this
command lists, and an action command's `-vv` output is where they are reported.

The leaf's dynamic variables are resolved as a run resolves them — a fresh cache
entry is used, and a stale or missing one runs its command and is written back —
with one exception: a declaration a `vars.toml` value overrides is not run,
since the listing would not show what it produced.

| Option           | Purpose                                                               |
|------------------|-----------------------------------------------------------------------|
| `--machine-only` | List `vars.toml` alone, reading no repository and no environment.     |
| `--no-refresh`   | Run no dynamic variable's command and write no cache; list what it holds. |

`--no-refresh` reports each dynamic variable from [the
cache](state.md#freshness-and-refresh-behavior) alone, so a fresh entry reads as
it would anyway, a stale one reads `stale, cached 2d ago`, and one with no entry
reads `no value (batfiles.toml, not cached)`. It adds nothing to
`--machine-only`, which reads neither the manifest nor the cache.

`--machine-only` answers with what this machine has persisted, so every line it
prints names a variable `vars unset` would remove and nothing can be shown as
overriding anything. It is also how to list variables where no repository is
selected: a normal listing reads the leaf `batfiles.toml` and fails when there
is none, as every command that reads the manifest does.

A value is quoted, so an empty value is visible as `""` rather than reading as a
variable with no value. Control characters are escaped to keep each entry on one
line. Names are sorted and aligned; shadowed origins are listed from highest to
lowest precedence. An empty set writes nothing to standard output — an
empty set is not data — and reports that there is nothing to list on standard
error, where `--quiet` suppresses it.

### `vars refresh`

```text
batfiles vars refresh [<key>...]
```

Run [dynamic variables](repoformat.md#dynamic-variables)' commands whatever the
[cache](state.md#freshness-and-refresh-behavior) holds, and write what they
capture back to it, so that a later run finds fresh values. With no key, every
declaration [in play](state.md#when-declarations-are-evaluated) runs: the leaf
repository's, and those of each remote an inclusion this machine would open
names. A declaration a `vars.toml` value overrides runs too, as it does in a
run.

```console
$ batfiles vars refresh
refreshed `email`
refreshed `corporate.has_op`
```

A key names one declaration: a leaf variable by its name, or a remote's as
`<remote-id>.<name>`, where `<remote-id>` is the remote's key in the leaf's
`[remotes]` rather than an inclusion's `id`. Only the declarations named run. A
key that is neither a [variable name](repoformat.md#names-and-ids) nor a remote
ID and a variable name joined by `.` is a usage error, before anything is read;
the cache's own `remote:corporate.has_op` spelling is one.

```console
$ batfiles vars refresh corporate.has_op
refreshed `corporate.has_op`
```

Which remotes are in play is decided in the leaf scope: the leaf's `[vars]`,
`vars.toml`, and `BATFILES_VAR_*`, as `vars list` shows them. The command
accepts no `--var`. Deciding needs the leaf's declarations, so a command that
refreshes a remote resolves the leaf's first, as a run would: a fresh value is
used, a stale one runs, and a leaf key named in the same command is refreshed
before the decision reads it. Naming only leaf keys reads no remote at all.

Every key is checked before any command it names runs. Each one that cannot be
refreshed is listed with its reason in one error, and the command exits 1:

```console
$ batfiles vars refresh editor corporate.team
error: cannot refresh 2 dynamic variables:
  `editor`: it is a static variable, with no command to run
  `corporate.team`: remote `corporate` is not allowed to run dynamic variables; set `allow-dynamic-vars = true` on it to run them
```

A key is refused when the manifest that would declare it does not, or declares
it as a plain string, and when it names a remote the leaf does not declare, does
not [allow](repoformat.md#git) to run commands, has out of play, or has not
materialized. A leaf key refreshed to decide a remote key's reachability stays
refreshed when that key is then refused. Without keys, a remote in play that is
not materialized warns and is passed over, as it is in a run. `vars refresh`
fetches nothing; [`sync`](#sync) brings a remote down.

Each declaration refreshed is named on standard error, without its value;
`--quiet` suppresses these lines, and a command with nothing to refresh says so.
Nothing is written to standard output. A command that fails warns as it does in
a run and keeps its cached value; once every other capture is saved, the command
exits 1 saying how many could not be refreshed. A status capture that could not
be started counts: the `"false"` a run assumes is not a captured value.

## Selecting What a Run Does

Selection preserves manifest declaration order. `sync` considers all actions;
the apply commands consider only the named action or group.

Three things say an action should be passed over, and a run honors all three:

- the machine-local [`disabled.toml`](state.md#disabledtoml-disabled-actions-and-groups)
  lists, which persist until an enable command or a hand edit removes the name;
- the run-only skips: `--skip-action` and `--skip-group`, together with
  [`BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS`](environment.md#run-only-skips),
  which apply to one invocation and are never written down; and
- the record's own [`when` or `unless`](repoformat.md#conditions), which is the
  repository saying the action does not belong on this machine.

A record an [inclusion](repoformat.md#include-remote) contributed answers to a
fourth, which comes before all of these: the [selection
filters](repoformat.md#selecting-part-of-a-remote) on the record that brought it
in. Those say what the leaf repository took from the remote rather than what this
machine leaves out of a run, so a record they left out is reported as not
selected without its condition being evaluated or its lists consulted.

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
invocation and a run that fails partway should not swallow them. A malformed
address is reported as the options are read; whether a well-formed one matched
anything waits until every [inclusion](repoformat.md#include-remote) the run
reaches has contributed what it contributes, since until then there is nothing to
match it against.

**An [inclusion](repoformat.md#include-remote) is excluded as a unit.** It is a
record in the list like any other, so every source above can name it — and what
it names is the inclusion together with everything it contributed, which is left
out unread. The qualified address of one contributed record is the finer way to
leave out part of it.

**A `disabled.toml` entry that matches nothing is silent.** Pre-registering a
name that a later branch or Git update introduces is the point of that document,
so the same non-match that warns above is expected there.

### Selection by command

| Exclusion source | `sync` | `apply-action` | `apply-group` |
| --- | --- | --- | --- |
| An inclusion's selection filters | Honor | Honor | Honor |
| Disabled action IDs | Honor | Waive | Honor |
| Disabled groups | Honor | Waive | Waive |
| `--skip-action` / `BATFILES_SKIP_ACTIONS` | Honor | Option rejected; environment ignored | Honor |
| `--skip-group` / `BATFILES_SKIP_GROUPS` | Honor | Option rejected; environment ignored | Option rejected; environment ignored |
| The action's `when` / `unless` | Evaluate if otherwise selected | Do not evaluate | Evaluate if otherwise selected |

`apply-action` bypasses its record's condition even if evaluation would fail.
Conditions on entries inside a selected clone list still apply. These overrides
affect only the invocation; they do not edit persistent disabled state.

[`clone`](#clone) is not a fourth column: the run it performs is a `sync`, so it
reads the first one.

A record an [inclusion](repoformat.md#include-remote) contributed answers to
every row above under its qualified address, and the inclusion that contributed
it answers under its own. Asking for the inclusion — `apply-group` naming the
group it is in — reaches everything it brought in.

**Naming what an inclusion contributed does not name the inclusion.** A
qualified target makes the command read the inclusion's manifest, but the
inclusion itself is decided as `sync` decides it: its disables, the run-only
skips the command reads, and its own `when`/`unless` all apply, and nothing the
table waives for a named record is waived for it. So an inclusion this machine
keeps out keeps what it contributes out under every command. Its manifest goes
unread, so the target cannot be resolved, and the command fails naming the
inclusion and the reason:

```console
$ batfiles apply-action --id corp.zshrc
error: action `corp.zshrc` would come from include-remote `corp`, which is excluded: group `work` is disabled
```

**An inclusion's [selection filters](repoformat.md#selecting-part-of-a-remote)
are honored everywhere**, which is the one row no command waives. They are the
leaf repository describing what it composed rather than a list this machine
keeps, for the same reason a remote's own condition below is not waived either.
So [`apply-action`](#apply-action) naming a record the filters left out installs
nothing and says so, rather than reaching past the description to the remote's
manifest.

All three commands load the leaf manifest and `disabled.toml`, even when lists
are waived. A missing manifest fails; a missing state document is empty under
the [state-file rules](state.md). Malformed or unreadable documents fail before
action execution. Variable loading follows [variable precedence](environment.md#variable-precedence).

Only `sync` [materializes](repoformat.md#materialization) the declared remotes.
An apply command is aimed at one record, and bringing the whole declared set up
to date is the whole-repository job `sync` is for, so it uses whatever is
already in `remotes/` and fetches nothing.

**A remote's own condition is different**, and every command evaluates it. It
decides whether this machine takes the remote at all rather than what one run
does, so an action installing from an [excluded
remote](repoformat.md#a-remotes-condition) is refused under all three commands —
including `apply-action`, whose waiver covers the record it names and not the
repositories that record reaches into. Only `sync` reports the exclusion, since
only `sync` had work to pass over.

### Clone-list preparation

After the run's list is expanded and selection and action conditions are settled,
all selected, unskipped clone lists are read and validated before any action
writes. This includes the lists an [inclusion](repoformat.md#include-remote)
contributed, each read from the materialization its record came from. Skipped
lists are not opened. A missing or malformed executable list fails the run before
installation begins, even if its action appears later in the list. A list
produced by an earlier action in the same run is therefore unavailable for
preparation. Entry conditions are evaluated during preparation, but their
exclusions are reported when the parent action runs, in list order.

### Addresses

Everywhere an action or a group is named — `--skip-action`, `--skip-group`,
`apply-action --id`, `apply-group --group`, the four enable and disable
commands, `disabled.toml`, and `[default-disabled]` — the value is an *address*:
a nonempty list of [IDs](repoformat.md#names-and-ids) joined by `.`, with no
upper bound on the number of segments. Dots are the separators and are not part
of an individual ID.

Four forms resolve today:

| Form                       | Meaning                                                 |
|----------------------------|---------------------------------------------------------|
| `<action-id>`              | Top-level action in the leaf repository.                |
| `<group>`                  | Group in the leaf repository.                           |
| `<inclusion>.<action-id>`  | Action an [`include-remote`](repoformat.md#include-remote) contributed. |
| `<inclusion>.<group>`      | Group a contributed action names.                       |

**An unqualified address names the leaf repository and nothing else.** Batfiles
does not search what an inclusion contributed for a matching unqualified name, so
a leaf `zshrc` and a contributed `corp.zshrc` are two records with one ID and two
addresses. In a qualified address, `<inclusion>` is the `id` of the leaf's
`include-remote` record, which need not match the remote it names; content from
an inclusion written without an `id` runs and answers to no address at all.
Reports still [name that inclusion](repoformat.md#include-remote), by where it
was written rather than by an address.

An address qualified by an inclusion is also what makes a command read that
inclusion's manifest, which is how `apply-action --id corp.zshrc` reaches past a
record it does not name. Reaching past the inclusion waives none of its
exclusions — see [selection by command](#selection-by-command).

The remaining form — an addressable entry inside a `git-clone-list` — is in
[`future/cmdline.md`](future/cmdline.md#address-forms) with the slice that gives
it something to refer to.

**Syntax and resolution are separate questions.** A well-formed address that no
form above can resolve — one naming an inclusion that declares nothing by that
name, or `a.b.c.d.e` — simply names nothing, so a command reports that it was
**not found** rather than that it was malformed. Only a malformed address, one
with an empty or non-ID segment, is refused as a name. Commands that record an
address without resolving it accept any well-formed one, which is what lets a
`disabled.toml` written today name what an inclusion added since.

```console
$ batfiles apply-action --id corp.nowhere
error: no action in /home/you/dotfiles/batfiles.toml has the id `corp.nowhere`
```

A run-only skip is warned about when nothing answers it, with one exception: a
name qualified by an inclusion this run did not read is neither matched nor
unmatched, since what would have answered it was never read.

### Exclusion reporting

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
warning: symlink gitconfig-work (group git): when "work" cannot be evaluated, so it is not installed: `work` is not declared. Add `work = "false"` to [vars] in batfiles.toml, run `batfiles vars set work <value>`, or write `vars.work` if the variable is meant to be optional
```

**A record an inclusion's [selection
filters](repoformat.md#selecting-part-of-a-remote) left out says so**, in the
same `-v` heading and ahead of every reason below, the leaf repository never
having taken it in:

```text
symlink corp.p10k (group corp.prompt) - skipped: not selected by include-remote `corp`
```

The reason names the inclusion the way [every report
does](repoformat.md#include-remote), so one written without an `id` is named by
where it was written rather than going unattributed.

When more than one reason applies, one is reported, in this order: a disable
ahead of a run-only skip, because it is the one still in force tomorrow when the
skip is gone; the action's own name ahead of its group's, because it is the more
specific of the two; and the condition last of all. Last means the condition is
not evaluated at all for a record something else already excludes, which is why a
condition that [cannot be
evaluated](repoformat.md#when-a-condition-cannot-be-evaluated) costs only the
runs that would otherwise have carried the record out.

Clone-list exclusions are reported under the parent action, in entry order.
An ordinary condition exclusion is `-v` detail:

```text
not cloning https://github.com/company/internal-zsh-tools.git (plugins.txt line 2): when "work" is false
```

An entry condition that cannot be evaluated instead emits a warning using the
same `not cloning` frame, naming the condition and failure. Other entries remain
eligible to run.

A [remote](repoformat.md#a-remotes-condition) its condition closes is reported
the same two ways, under the heading its materialization would have printed
under, before the first action:

```text
remote core - skipped: unless "personal" is true
warning: remote core: when "work" cannot be evaluated, so it is not materialized: `work` is not declared. Add `work = "false"` to [vars] in batfiles.toml, run `batfiles vars set work <value>`, or write `vars.work` if the variable is meant to be optional
```

Neither stops the run. An action that then installs from the excluded remote
fails as any action reaching content it may not read does, naming the remote and
repeating the reason.

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

**A dry run runs dynamic variables' commands.** The plan is decided by
conditions, and a condition may read a [dynamic
variable](repoformat.md#dynamic-variables), so a dry run resolves them exactly as
a real run does: a stale or missing value's command runs, `--refresh-vars` runs
every one, and what they capture is written to
[`dynamic-vars.toml`](state.md#dynamic-varstoml-dynamic-variable-cache). Those
commands are arbitrary programs, and whatever else they do — to the filesystem,
the network, or anything — is the one part of a dry run batfiles does not
control.

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
in both modes: a dry run settles a `dest` holding a regular file or a symlink
out of the repository exactly as a real run does, and says it would back it up.
What it gives up is one distinction — telling a clone from a plain directory
takes git, so a dry run says it would update either, and the real run is where
the second is found to be a conflict.
That is the same "intent, not success" boundary the fetching actions sit on.

The alternative — a dry run that fetched, so its report could say what an update
would actually bring — would make `--dry-run` a command that reaches the network
and modifies a checkout it was asked only to describe, and doing neither of those
is the whole value of the flag. To see the current picture, update the clone and
run it again.

**A declared remote is described, not materialized.** `sync`
[brings every declared remote down](repoformat.md#materialization) before its
first action; under `--dry-run` it says what it would clone or update for each
Git remote on exactly the terms above, runs no git, and creates no `remotes/`
tree to say it in. A file or archive remote is described the way a fetching
action is: whether it would be fetched, fetched again, or left alone is decided
from what is at `remotes/<id>` and the stamp beside it, which are inspection, and
nothing is requested. A `remotes/<id>` batfiles would refuse to replace is
refused in a dry run too. The one command that fetches a remote fetches none here, so a dry
run leaves the repository as untouched as it leaves the home. A remote whose
[condition](repoformat.md#a-remotes-condition) closes is passed over instead,
and reports the line it reports in a real run — a skip is the line that takes no
tense, and neither mode reads the tree.

What the manifest names is still read. A `source` that is missing or unreadable
fails in either mode, before the destination is considered — including where the
destination is occupied and a real run would have kept it. A manifest naming a
source that is not there is a repository error, and reporting it only on the day
the destination happens to be empty would be the less useful behavior. A
destination in the way is settled in either mode too, for the same reason: the
decision is inspection's, so a dry run names each backup it would make, under
the name this run would give it. It asks nothing, which is why `--interactive`
is refused beside it. Under `--refresh-content` it builds and fetches nothing,
so it cannot tell a seed that already matches from one that differs, and
reports the refresh a difference would make.

A source inside a remote is that rule applied to a whole tree. What a dry run
reads is the materialization already on this machine, which is as current as the
last `sync` left it and no more — the report says what that tree holds, and
knowingly so, rather than what the remote has published since. Where there is no
materialization at all the action is refused by name, as it is under an
[apply command](#apply-action): a dry run materializes nothing, so it is in the
same position, and a plan drawn from a tree that is not there would be an
invention.

**An inclusion is described from the same tree, and costs more when it is not
there.** What comes out of a materialization an
[`include-remote`](repoformat.md#include-remote) reads is not one action's
content but the actions themselves, so an inclusion is described exactly as the
tree on this machine declares it, which may be out of date and knowingly so. An
inclusion with no materialization at all contributes actions that cannot be
listed, and that is the one case where an absent tree is *not* a refusal: one
action's source is something the rest of the plan can do without, and the list
itself is not. It is reported with a reason, and the plan is marked partial
rather than pretending to be whole. To see the rest, refresh the remote — by
running `sync`, or by updating that checkout by hand — and repeat the dry run.

## Plan Completeness

An [`include-remote`](repoformat.md#include-remote) that cannot be listed at all
**warns, naming the remote, the reason, and the remedy**:

```text
warning: remote `corporate` is not materialized at /home/me/dotfiles/remotes/corporate,
         so what it includes cannot be listed; run `batfiles sync` to bring it down
```

That warning is what marks a plan **partial**. A plan in which nothing was
reported that way is **complete**, and batfiles says nothing further about it:
completeness is the absence of that warning rather than a verdict printed at the
end, so a run with no inclusions in it and a run whose every inclusion was read
look alike, as they should.

A remote [whose own condition closed](repoformat.md#a-remotes-condition) is not
one of these. The inclusion reports that it brought nothing in and the plan stays
whole — a manifest leaving something out as written has left nothing unanswered.
Neither is an inclusion the run never reached, one a condition, a disable, or a
skip excluded.

Nor are the warnings an inclusion draws from a manifest it did read: a
[filter that matched nothing](repoformat.md#selecting-part-of-a-remote), a
[nested inclusion left out](repoformat.md#what-an-included-action-may-not-write),
and the [`[remotes]` that manifest declares](repoformat.md#an-included-manifests-own-remotes)
all describe a list that was read in full. Nothing about the plan is missing, so
none of them makes it partial.

Completeness is about what the run could describe, not about whether it
succeeded: a partial plan exits 0, and everything else the run was able to list
runs or is reported exactly as it would be otherwise. Only a run that
materializes can turn a partial plan complete. `sync` does that before its first
action, so its plans are complete unless a remote's condition closed;
`sync --dry-run` and the apply commands read what is already on the machine and
fetch nothing, which is where a missing materialization shows up.

## Unimplemented Options

An option that parses but is not honored yet is **refused, never ignored**: the
run exits 2 naming the option and the step that makes it live, before any root
is resolved or any file is opened. Every option any command accepts is honored
today, so nothing is refused this way.

## Execution failures

An action failure stops `sync` or an apply command with status 1. Earlier
changes remain; nothing is rolled back. Directory-wide actions likewise stop
at the first failed child, preserving work already completed for earlier children.

### Clone-list entry failures

`git-clone-list` processes entries independently. An entry's destination that
something is in the way of, including a directory that is not a clone, an
indirect checkout, or an incomplete clone, is a
[conflict](safety.md#conflicts-and-backups) settled for that entry alone. It
warns and continues after:

- a Git subprocess that ran but failed, including clone, fetch, or update;
- a declared ref that cannot be resolved.

The warning names the repository source, list file, line number, and entry ID
when present, and, where the entry's failure came after its destination was
backed up and the backup could not be put back, where the backup is. Warnings remain visible under `--quiet`. Failure to launch Git,
filesystem read/write failures, failure to create the destination container,
and an `--interactive` question left unanswered stop the run. These are conservative error categories, not a claim that every
remaining entry would fail in the same way.

Recoverable entry failures do not make the command fail: status 0 can therefore
include entries that did not clone or update, even when every entry failed.
Consult the warnings and the [Git recovery policy](safety.md#git-updates)
before retrying. Standalone `git-clone` action failures propagate normally.

Condition evaluation failures also warn and skip the affected action or entry
without making the command fail. The [condition rules](repoformat.md#when-a-condition-cannot-be-evaluated)
define this fail-closed behavior; [exclusion reporting](#exclusion-reporting)
defines the action warning format.

## Exit Statuses

| Status | Meaning                                                                 |
|--------|---------------------------------------------------------------------------|
| `0`    | The command did what was asked. `--help` and `--version` exit here too.   |
| `1`    | The command ran and failed.                                               |
| `2`    | The command did not run: the invocation was wrong.                        |

The distinction that matters is between 1 and 2. A status of 1 means batfiles
started doing the work and something went wrong partway, so the filesystem may
have been touched. A status of 2 means nothing was attempted — a usage error —
so nothing was read or written and the invocation can be corrected and retried
freely.

Status 2 is what clap already uses for the usage errors it renders, and an
[unimplemented option](#unimplemented-options) would join it rather than
report a failure it never had.

Status 1 covers a command that needs a home directory and cannot determine one,
a `sync` whose leaf `batfiles.toml` is missing, malformed, or invalid, or whose
`disabled.toml` is malformed, a `vars get` naming a variable this machine has no
value for, a [`vars refresh`](#vars-refresh) naming a key it cannot refresh or
whose command failed, an [`update`](#update) that could not read, verify, or
install the release it chose, an argument that is not a well-formed address or variable name, an
[`init`](#init) that refused the directory it was run in or could not put a Git
repository around it, and an
action that could not be carried out — a source the repository does not contain,
a remote's materialization holding something batfiles will not replace, an
`--interactive` question standard input closed on, or a write the operating
system refused. In each case the invocation was well-formed and
something outside it did not hold up.

Failures before action execution leave installation destinations untouched;
action failures may leave earlier work completed. See [execution failures](#execution-failures)
for stopping behavior and warnings that permit status 0.
