# `include-remote`

Takes the actions another repository declares into this one, at this position in
the list.

```toml
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
install-groups = ["shell"]
exclude-actions = ["p10k"]
```

| Field             | Type                  | Required | Description                                             |
|-------------------|-----------------------|:--------:|---------------------------------------------------------|
| `remote`          | ID                    |   yes    | A [`git` remote](../repoformat.md#git) this same manifest declares.     |
| `install-actions` | ID or list of IDs     |    no    | Take only the actions named.                            |
| `install-groups`  | ID or list of IDs     |    no    | Take only the actions naming these groups.              |
| `exclude-actions` | ID or list of IDs     |    no    | Leave out the actions named.                            |
| `exclude-groups`  | ID or list of IDs     |    no    | Leave out the actions naming these groups.              |
| `vars`            | map of name to string |    no    | [Values](#variables-for-one-inclusion) for what it takes. |

`remote` must name a Git remote in the same manifest; undeclared, file, or
archive remotes are load errors. Multiple inclusions of one remote share its
materialization.

The inclusion reads `remotes/<remote>/batfiles.toml` and splices its actions
into this position, in their declaration order. Included repository paths, including clone-list
sources, resolve from that materialization. A present materialization with no
manifest fails. Expansion finishes before action execution; a malformed included
manifest therefore prevents installation. An absent materialization follows
[plan completeness](../cmdline.md#plan-completeness).

Inclusion is one level deep, subject to the [restrictions below](#what-an-included-action-may-not-write).
A closed inclusion or remote condition contributes nothing, leaving the plan
complete. [Selection](../cmdline.md#selecting-what-a-run-does) defines when an
inclusion is opened and how its group or exclusion reaches its contents.

An inclusion's `id` qualifies contributed [addresses](../cmdline.md#addresses),
such as `corp.zshrc`; it need not match the remote ID. Without an ID, its
contents still run but cannot be addressed. Reports identify it by position
and remote, and attribute contributed records to it:

```text
include-remote action 2 of remote `corporate`
symlink zshrc (group shell, from include-remote action 2 of remote `corporate`)
```

Named inclusions use ``include-remote `corp` `` in diagnostics. The same name
identifies filter warnings and [variable blocks](../commands/vars.md#vars-list).

## Selecting part of a remote

Filter values are unqualified IDs from the included manifest, as one string
or a list. With no filters, take all actions. An omitted allow-list differs
from an empty one: `install-actions = []` takes nothing.

At most one of `install-actions`, `install-groups`, and `exclude-groups` may be
present. `exclude-actions` may stand alone or accompany either group filter,
but not `install-actions`. Other combinations are load errors.

Allow-lists omit records lacking the relevant ID or group; deny-lists retain
them. Action and group filters use separate namespaces. A name matching nothing
warns, naming the inclusion, filter, and remote, but does not fail the run.

Filtered-out actions retain their addresses for selection diagnostics.
Filters are honored by every command; other exclusions can still apply to
selected records. See [selection](../cmdline.md#selection-by-command).

## Variables for one inclusion

```toml
[[actions]]
type = "include-remote"
id = "corp"
remote = "corporate"
vars = { profile = "work" }
```

`vars` maps [variable names](../repoformat.md#names-and-ids) to strings, validated at load time.
Dynamic declarations are not accepted here. Omitted and empty maps both
provide no overrides. These values apply to contributed records, including
clone-list entries; see [scope and precedence](../environment.md#variable-precedence).

## Variables an included remote declares

An included manifest may declare its own static or dynamic `[vars]`, using the
[same schema](../repoformat.md#variables) as the leaf. They are the lowest layer of
that inclusion's [scope](../environment.md#variable-precedence), so the leaf and
this machine can override them without knowing they exist. They never decide the
inclusion's own condition or the remote's, which use the leaf scope.

Remote dynamic declarations require [`allow-dynamic-vars`](../repoformat.md#git). They share
one capture and cache entry per remote variable across inclusions; see
[cache evaluation](../state.md#when-declarations-are-evaluated).
Use `-vv` to inspect [inclusion variable blocks](../commands/vars.md#vars-list).

## What an included action may not write

Included repository paths must refer to the declaring repository's own tree.
Remote references (`@core/files/zshrc` or the structured equivalent) are
refused even if that manifest declares the remote.

Nested `include-remote` records instead warn and are dropped; other actions
are contributed normally. The nested remote need not exist, since it will
never be resolved:

```text
warning: not included: include-remote corp.shared; an included repository does not reach further repositories
```

## An included manifest's own `[remotes]`

The map is read for schema shape, then ignored. Unknown types and fields still
fail. Value validation is skipped: invalid URLs, digests, empty refs, and
case-colliding remote keys do not fail a run that includes the manifest.
Nothing materializes these remotes or allows an included action to source them.

A nonempty map warns once per inclusion, naming all ignored records; an empty
one is silent.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
