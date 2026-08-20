# Batfiles Command-Line Surface

The parts of the command-line interface that run today: the set of commands, the
options every command accepts, where output goes, and what the exit status
means.

The per-command specifications, the shared action-execution and selection
options, dry-run behavior, and address forms are in
[`future/cmdline.md`](future/cmdline.md) until the commands that use them are
built.

## What runs today

The whole surface parses. Every command and option listed below is accepted, and
an invalid invocation is rejected as a usage error before anything else happens.

**Only `version` and `sync` do any work.** Every other command resolves the
location roots it needs and then reports that it is not implemented yet, exiting
2 having written nothing. That message is the answer to "what can batfiles do",
and it disappears one command at a time.

`sync` is the first command that writes. It executes the one action type that
exists, which is enough to install a repository made of symlinks and nothing
else. Options it accepts but does not honor yet fail rather than being ignored,
so nothing appears to have happened that did not.

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
| `--batfiles-dir <path>`         | Select the leaf repository. Defaults to `<selected-home>/dotfiles`.                                               |
| `--home-dir <path>`             | Select the destination home directory. Defaults to the current user's home directory.                             |
| `--config-dir <path>`           | Select the directory containing `vars.toml` and `disabled.toml`. Defaults to the XDG config location.             |
| `--cache-dir <path>`            | Select the directory containing `dynamic-vars.toml`. Defaults to the XDG cache location.                          |

`--color` and the four location options have their full effect. `--verbose` at
one level prints the resolved roots and the actions `sync` found nothing to do
about. `--quiet` suppresses the lines saying what `sync` did, and nothing else.

Two of the four resolved roots are live, and only for `sync`: it reads the leaf
repository and writes into the selected home. Nothing reads or writes anything
under the config and cache directories yet. See
[location selection](environment.md#location-selection) for the precedence, and
run a command with `-v` to see what it selected.

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
standard output. `version` is the only one that does today; `vars get` and
`vars list` join it when they are built.

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
batfiles sync
```

Read the leaf repository's [manifest](repoformat.md#reading-the-manifest) and
execute its actions in declaration order, each one inspecting the filesystem as
the previous one left it. The first failure stops the run; what earlier actions
did stays done, and nothing is rolled back.

One line on standard error names each action that changed something. An action
that found its destination already correct says nothing, because a repository
that is already installed is the ordinary case and forty lines of "unchanged"
is how output stops being read; `-v` reports those too. `--quiet` suppresses
both.

These options parse and are refused rather than ignored, each naming the step
that makes it live: `--dry-run`, `--skip-action`, `--skip-group`, `--var`,
`--refresh-remotes`, `--refresh-vars`, `--refresh-content`, `--no-overwrite`,
and `--interactive`. The refusal is a status-2 "nothing was attempted", raised
before any root is resolved or any file is opened. The list shrinking to empty
is how you know `sync` is finished.

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
a `sync` whose leaf `batfiles.toml` is missing, malformed, or invalid, and an
action that could not be carried out — a source the repository does not contain,
a destination holding something batfiles will not replace, or a write the
operating system refused. In each case the invocation was well-formed and
something outside it did not hold up.

The first two of those happen before anything is written; the third may not.
`sync` stops at the first action that fails, so an earlier action's symlink is
still there. That is what status 1 means and status 2 does not: the filesystem
may have been touched, and the fix is to look rather than to retype the command.
