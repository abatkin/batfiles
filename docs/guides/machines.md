# Configure a machine

Keep one dotfiles repository and select different content for each machine.
Use groups for manual choices and variables with conditions for reusable rules.

## Declare a default and a condition

Add this `[vars]` table at the top of `batfiles.toml`, before any `[[actions]]`:

```toml
[vars]
work = "false"
```

Add an action that should run only on a work machine:

```toml
[[actions]]
type = "create-dir"
id = "work-notes"
group = "work"
dest = "~/work-notes"
when = "work"
```

Variables are strings, including `"true"` and `"false"`. They control
conditions; they do not interpolate into paths or other strings.

## Choose locally or for one run

```sh
batfiles vars set work true
batfiles sync --dry-run
batfiles sync
```

The value is saved on this machine, without editing the repository. To override
it for one run:

```sh
batfiles sync --var work=false
```

To remove the machine override and return to the repository's default:

```sh
batfiles vars unset work
```

The condition prevents future execution; it does not remove `~/work-notes`
after that directory has been created. Use `sync --dry-run` to check conditions;
explicitly naming an action with `apply-action` bypasses its own condition.

## Inspect values

```sh
batfiles vars list --no-refresh
batfiles vars list --machine-only
batfiles vars get work
```

The first lists effective leaf variables and their origins without running
dynamic commands. The second lists only stored machine values. `vars get` reads
a stored machine value, not the effective value, and fails if none is stored.

For ordinary leaf variables, command-line overrides win over environment
values, which win over machine values, which win over manifest defaults.
See [the complete precedence](../environment.md#variable-precedence), including
inclusion-specific values.

## Select by operating system

Conditions can inspect built-in facts:

```toml
[[actions]]
type = "create-dir"
id = "linux-cache"
dest = "~/.cache/my-tools"
when = "facts.os == 'linux'"
```

Other OS values include `macos` and `windows`. Use `unless` for the inverse
condition; an action cannot declare both. For named profiles, compare strings:
`when = "profile == 'work'"`, with a declared `profile` variable.
See [conditions](../repoformat.md#conditions) and [host facts](../environment.md#host-facts-in-conditions).

## Set bootstrap defaults

A repository can initially disable a group until a user opts into it:

```toml
[[default-disabled.groups]]
group = "work"
```

`clone` and `sync --bootstrap` offer these defaults when no `disabled.toml`
exists. To bootstrap while opting into the work group:

```sh
batfiles sync --bootstrap --enable-group work
```

Enabling a group does not make a false action condition true; set `work` as well
if using the earlier example. Defaults do not reset existing machine choices.
See [bootstrap defaults](../repoformat.md#default-disabled-bootstrap-entries).

## Discover values with commands

[Dynamic variables](../repoformat.md#dynamic-variables) capture a command's
output or success and cache it. Use them when a static machine choice is not
enough. Their commands run with your permissions, including during dry-run.
Included repositories need explicit permission to run them.

Use `batfiles vars refresh` to refresh the declarations in play, or
`batfiles sync --refresh-vars` to refresh those evaluated by that run.
