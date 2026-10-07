# `sync`

```text
batfiles sync [--dry-run | --refresh-remotes] [--skip-action <id>]... [--skip-group <group>]...
    [--bootstrap [--enable-action <id>]... [--disable-action <id>]...
                 [--enable-group <group>]... [--disable-group <group>]...]
```

[Materialize remotes](../repoformat.md#materialization), then execute selected
actions in declaration order. Each action sees the filesystem left by its
predecessors. [Selection](../cmdline.md#selecting-what-a-run-does) defines exclusions and
loading; [execution failures](../cmdline.md#execution-failures) defines stopping behavior.

| Option | Purpose |
| --- | --- |
| `--dry-run` | [Report intended work](../cmdline.md#dry-run-behavior) |
| `--refresh-remotes` | Fetch every file and archive remote whose condition holds again |
| `--skip-action <id>` | Skip one action; repeatable |
| `--skip-group <group>` | Skip one group; repeatable |
| `--bootstrap` | Adopt bootstrap choices before synchronization |

The [shared execution options](../cmdline.md#shared-action-execution-options) also apply.

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
[exclusion reporting](../cmdline.md#exclusion-reporting).

`--bootstrap` performs [clone's bootstrap](clone.md#what-the-bootstrap-decides).
The four enable/disable options in the syntax are accepted only with it;
otherwise they are usage errors. In a dry run, bootstrap reports its decisions
and uses them for selection without writing `disabled.toml`.
The [leaf stub](../installer.md#leaf-stub) runs `sync --bootstrap`.

`--refresh-remotes` rebuilds even materializations whose declarations still
match their stamps. Remote conditions and ownership checks still apply; Git
remotes are unaffected because normal synchronization already updates them.
It cannot accompany `--dry-run`.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
