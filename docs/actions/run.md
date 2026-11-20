# `run`

Runs a command, as often as its trigger says.

```toml
[[actions]]
type = "run"
id = "mise-install"
command = ["~/.local/bin/mise", "install"]
trigger = { after = "mise" }
```

| Field | Type | Required | Description |
| --- | --- | :---: | --- |
| `command` | [template string](../repoformat.md#templates), or non-empty list of template strings | yes | A shell command line, or a program and its arguments run without a shell. |
| `trigger` | [trigger](#triggers) | no | When the command runs. Defaults to `"always"`. |
| `cwd` | [template string](../repoformat.md#templates) | no | Where the command runs. Never empty; `~` is the home, and a relative path starts from the declaring repository's root, which is the default. |
| `env` | map of name to template string | no | Variables set for the command, on top of the environment it inherits. |
| `command-timeout` | [duration](../repoformat.md#duration-values) | no | How long the command may run before it is killed. Greater than zero. Defaults to `"1m"`. |
| `allow-failure` | boolean | no | Whether a failed command warns instead of failing the run. Defaults to `false`. |

A `run` action installs nothing. It is [selected](../cmdline.md#selecting-what-a-run-does)
like any other action, and then its trigger decides whether the command runs.

## Triggers

| `trigger` | The command runs when | Needs `id` |
| --- | --- | :---: |
| `"always"` | Every run that admits the action | no |
| `"bootstrap"` | The run is [`clone`](../commands/clone.md#clone) or [`sync --bootstrap`](../commands/sync.md#sync) | no |
| `"once"` | It has never succeeded | yes |
| `{ every = "24h" }` | It has never succeeded, or its last success is at least that old | yes |
| `{ after = "mise" }`, `{ after = ["mise", "@tools"] }` | It has never succeeded, or a target it names [changed](#what-changed-means) since its last success | yes |

`once`, `every`, and `after` record each success in
[`runs.toml`](../state.md#runstoml-run-action-history), keyed by the action's
[address](../cmdline.md#addresses). They need an `id`, and a contributed one
also needs its inclusion to have one, as in `corp.rebuild-cache`. A missing
action `id` is a load error; a missing inclusion `id` fails the run that opens
the inclusion, naming both.

Any other trigger string, an empty `after` list, a table writing both or neither
of `every` and `after`, or an invalid duration is a load error. A zero `every`
behaves like `always` with a record kept.

A trigger that does not fire is not a failure. `-v` gives its reason:

```text
run mise-install
not run: nothing it follows changed
```

The reasons are `not a bootstrap run`, `already ran`, `ran 3h ago`, and
`nothing it follows changed`.

[`apply-action`](../commands/apply-action.md#apply-action) runs the command
whatever its trigger says, and records a success as usual.
[`apply-group`](../commands/apply-group.md#apply-group), `sync`, and `clone`
honor the trigger. Changing the command resets nothing: to run it again, use
`apply-action` or rename the `id`.

### `after` targets

- A bare ID names an action declared earlier in the same manifest: any type,
  including another `run` action or an `include-remote`.
- `@id` names a [remote](../repoformat.md#remotes) the manifest declares, as a
  repository path's `@remote` does. An included manifest cannot name one.

An unknown target, a target declared later, or the action itself is a load
error.

### What "changed" means

A target changed when, in this run, it created, replaced, refreshed, or updated
something:

| Target | Changed when |
| --- | --- |
| Seed and link actions | It installed or replaced at least one node, or created the directory a directory-wide action installs into |
| `create-dir` | It created the directory. An existing usable directory is unchanged. |
| `git-clone`, `git-clone-list` | It cloned, or an update moved a checkout to a different commit |
| `run` | Its command ran and succeeded |
| `include-remote` | Any record it contributed changed, by this table |
| File or archive remote | It was fetched or refetched |
| Git remote | It was cloned, or an update moved it to a different commit |

Unchanged, excluded, disabled, skipped, and failed targets are not changes.
Remotes change only in commands that [materialize](../repoformat.md#materialization)
them: `sync` and `clone`. A target containing an `always` command changes on
every run, so an `after` naming it runs on every run.

Every change by an action or remote with an ID is recorded in `runs.toml`,
whether or not anything follows it, as is every success, each under the
[generation](../state.md#generations) of the run that made it. An `after`
command is due when a target changed in a later generation than its last
success. A change earlier in the same run, before the success, is one it has
handled. So:

- A command that is disabled, skipped, excluded, or fails, or that a run stops
  before reaching, stays due, and runs on the next run that admits it.
- A command that several changes passed by runs once to catch up.
- A failure during materialization, assembly, or preparation still records the
  remotes and actions that changed before it, so their followers run once the
  failure is fixed, even though nothing changes on that run.

`core` and `@core` ask different questions when both exist:

```toml
[remotes.core]
type = "git"
url = "https://git.example.com/dotfiles-core.git"

[[actions]]
type = "include-remote"
id = "core"
remote = "core"

[[actions]]
type = "run"
id = "reload"
command = "tmux source-file ~/.tmux.conf"
trigger = { after = "core" }

[[actions]]
type = "run"
id = "rebuild"
command = ["make", "-C", "remotes/core"]
trigger = { after = "@core" }
```

`reload` runs when something the inclusion installed changed; `rebuild` when
the remote's checkout moved, whether or not anything it installs changed.

Following a remote is what makes an upgrade run: a file remote fetched again
for a new release changes the remote, while the symlink to it reports
unchanged.

```toml
[vars]
mise_version = "2026.6.0"

[remotes.mise]
type = "file"
url = "https://github.com/jdx/mise/releases/download/v${vars.mise_version}/mise-v${vars.mise_version}-${facts.os}-${facts.arch}"
executable = true

[[actions]]
type = "symlink"
id = "mise"
source = "@mise"
dest = "~/.local/bin/mise"

[[actions]]
type = "run"
id = "mise-install"
command = ["~/.local/bin/mise", "install"]
trigger = { after = "@mise" }
```

## How the command runs

A command runs as the invoking user without a sandbox, like a
[dynamic variable's](../environment.md#how-dynamic-commands-are-run), with these
differences:

| Aspect | Rule |
| --- | --- |
| Launch | String: `sh -c` on Unix, `cmd /S /C` on Windows. List: program and arguments, no shell, with a relative program such as `./bin/setup` found from the working directory. |
| Working directory | `cwd`, or the declaring repository's root: the leaf, or the remote's materialization. A `cwd` that does not reach a directory, including a dangling symlink, is a failure to start. |
| Environment | Inherited, with `env` added over it. Variables batfiles resolves are not exported otherwise. |
| Standard input | Closed. |
| Standard output and error | Inherited, so the command sees the terminal and keeps its own colors and progress output. Under `--quiet`, standard output is disconnected and standard error kept. |
| Timeout | `command-timeout` bounds the whole run. Expiry kills only the directly started process, and is a failure. |
| Success | Exit status 0. |

Before launching, batfiles names the action and the command as written, with
its templates unresolved:

```text
run mise-install: ~/.local/bin/mise install
```

The command and each list element, `cwd`, and each `env` value are
[templates](../repoformat.md#templates), evaluated only when the command is about
to run; a trigger that does not fire evaluates nothing. The string form hands resolved text to a shell, so prefer the
list form for interpolated values.

In the string form, template syntax meets shell syntax. `$HOME` passes through
untouched, but `${HOME}` and `${VAR:-default}` are templates. Write
`${env.HOME}` to have batfiles resolve it, or `$${VAR:-default}` to hand the
shell a literal `${`.

### Where it runs, and with what

`cwd` resolves like a `dest` when it begins with `~`, under the
[selected home](../environment.md#location-selection); an absolute path is used
as written, and a relative one starts from the declaring repository's root. A
dry run does not check that it exists, since an earlier action may be what
creates it.

`env` names follow the platform's rules, except that a name is never empty and
holds neither `=` nor NUL. A value is a template, so it can extend what the
command inherits:

```toml
[[actions]]
type = "run"
id = "tools"
command = "make install"
cwd = "~/src/tools"
env = { PREFIX = "${env.HOME}/.local", PATH = "${env.HOME}/.local/bin:${env.PATH}" }
```

`~` is not expanded in an `env` value; write `${env.HOME}`. In an included
manifest, reading `env` needs [`allow-env-templates`](include-remote.md#templates-and-the-host-environment)
as in any template.

### Reaching a program installed in the same run

A program installed earlier in the run, say into `~/.local/bin`, may not be on
`PATH`. In the list form, a first element beginning `~/` expands to the
selected home, as a `dest` does; other elements are passed as written. The
string form leaves `~` to the shell, whose home may differ from a selected
`--home-dir`. To find more than one program there, add the directory to `PATH`
with `env`.

### Failure

A nonzero exit, a timeout, or a failure to start fails the action, stopping the
run with status 1, as any [execution failure](../cmdline.md#execution-failures)
does:

```text
error: run mise-install: the command exited with status 1
```

With `allow-failure = true` it is a warning instead, and the run continues with
its exit status unaffected. Either way a failure records nothing: `once` and
`every` try again on the next run, and `after` stays due.

### Dry runs

A [dry run](../cmdline.md#dry-run-behavior) launches nothing and leaves
`runs.toml` untouched. When the command is due, including under `apply-action`,
its templates are checked and it reports `would run`. An `after` command that
nothing has made due since its last success reports what would, without
evaluating the command, because a dry run does not know what its targets would
change:

```text
run mise-install: would run if `@mise` changes
```

Other triggers that do not fire report their usual reason.

## In an included manifest

A `run` action an [inclusion](include-remote.md#include-remote) contributes runs
only if the leaf sets [`allow-commands`](../repoformat.md#git) on the remote:

```toml
[remotes.corporate]
type = "git"
url = "git@git.example.com:it/dotfiles.git"
allow-commands = true
```

Without it, the action is excluded with a warning naming the remote and the
field, and the run continues:

```text
warning: run corp.setup - skipped: remote `corporate` does not set `allow-commands = true`, so the `run` actions it contributes do not run
```

`apply-action` naming it fails with that reason; the permission is not waived.
Leaf `run` actions need no permission. The permission applies only to a checkout
of the repository the remote's `url` names; see
[permissions follow the origin](../safety.md#permissions-follow-the-origin).

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
