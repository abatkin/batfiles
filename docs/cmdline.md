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

**Only `version` does any work.** Every other command resolves the location
roots it needs and then reports that it is not implemented yet, exiting 2 having
written nothing. That message is the answer to "what can batfiles do", and it
disappears one command at a time.

`sync` goes one step further before it gets there: it reads and parses the leaf
repository's `batfiles.toml`, so a repository that has none, or whose manifest
is malformed, fails on the manifest instead. See
[reading the manifest](repoformat.md#reading-the-manifest).

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
one level prints the resolved roots, which is the only detail there is to add so
far. `--quiet` does what it promises and currently suppresses nothing, because
no command prints an informational line yet.

Of the resolved roots, only the leaf repository is read, and only by `sync`,
which opens the `batfiles.toml` it finds there. Nothing reads or writes anything
under the other three, and nothing writes anywhere at all. See
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
unimplemented command joins it rather than reporting a failure it never had.

Two failures reach status 1 so far: a command that needs a home directory and
cannot determine one, and a `sync` whose leaf `batfiles.toml` is missing or
malformed. Nothing was written in either case — there is nothing yet that
writes — but both are failures to do the work rather than refusals to start it.
The invocation was well-formed, and something outside it did not hold up: no
home the machine could name, or no manifest where the repository should have
one.
