# Share configuration between repositories

Keep reusable configuration in one repository and choose how each machine uses
it from your own dotfiles repository. Your own checkout is called the *leaf*;
a declared *remote* supplies additional content.

## Use a remote's files

Add a Git remote near the top of `batfiles.toml`, before `[[actions]]`:

```toml
[remotes.shared]
type = "git"
url = "https://git.example.com/team/dotfiles.git"
ref = "main"
```

Replace the URL with your shared repository. Its ordinary files can be used
without a manifest:

```toml
[[actions]]
type = "copy"
id = "shared-editor"
source = "@shared/files/editor.toml"
dest = "~/.config/my-editor/settings.toml"
```

`sync` fetches the remote into `remotes/shared/` before applying actions.
That generated directory belongs in `.gitignore`. Your leaf still chooses what
to install and where. As a copy, this example keeps an existing destination;
refresh it deliberately when you want updated starting content.

## Include the remote's actions

If the shared repository has its own `batfiles.toml`, you can include its actions
instead of declaring copies yourself:

```toml
[[actions]]
type = "include-remote"
id = "team"
remote = "shared"
install-groups = ["editor"]
```

This includes actions in the shared manifest's `editor` group, at this position
in your manifest. Omit `install-groups` to include all its actions. Review the
included repository: it can choose destinations and affect your files.

Use `sync` to fetch the remote and run the selected actions. Until `sync` has
fetched it, a dry run cannot list the included actions and reports a partial
plan, and apply commands cannot run them.

Inclusion is one level deep. Nested inclusions are warned about and dropped;
the included manifest's own remotes are ignored. See
[the inclusion reference](../actions/include-remote.md).

## Address included actions

If the shared manifest has an action with `id = "settings"`, the inclusion
above exposes it as `team.settings`:

```sh
batfiles apply-action --id team.settings
batfiles disable-action team.settings
batfiles apply-group --group team.editor
```

The prefix is the inclusion's `id`, not the remote's name. Naming an included
action does not bypass the inclusion: if `team` is disabled or its condition is
false the command fails, and an action outside `install-groups` applies nothing.
See [addresses](../cmdline.md#addresses).

## Keep a work remote off personal machines

Gate both the remote and its inclusion:

```toml
[vars]
work = "false"

[remotes.shared]
type = "git"
url = "https://git.example.com/team/dotfiles.git"
when = "work"

[[actions]]
type = "include-remote"
id = "team"
remote = "shared"
when = "work"
```

Set `batfiles vars set work true` on a work machine. Disabling only the inclusion
does not prevent `sync` from contacting an otherwise eligible remote; the
remote's own condition controls that.

An inclusion can pass static values through `vars = { profile = "work" }`.
Included dynamic commands are disabled unless the leaf explicitly sets
`allow-dynamic-vars = true` on the Git remote. See
[inclusion variables](../actions/include-remote.md#variables-for-one-inclusion).
