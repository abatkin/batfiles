# Everyday use

Run these commands from the root of your dotfiles checkout. From anywhere else,
add `--batfiles-dir /path/to/dotfiles`. Discovery checks the current directory,
not its parents; otherwise it falls back to `~/dotfiles`.
See [location selection](../environment.md#location-selection).

## Apply changes

```sh
batfiles sync --dry-run
batfiles sync
batfiles sync -v
```

`sync` prepares remote content and applies selected actions in manifest order.
It does not pull your own dotfiles checkout; update that with Git separately.
Correct destinations are quiet. `-v` explains unchanged and skipped actions;
`-vv` also shows effective variables.

Symlinked files already reflect repository edits. Copied and downloaded files
are seeds: an ordinary run keeps existing destinations. Use a deliberate
[content refresh](../getting-started.md#change-the-starting-content) when needed.

Changes and diagnostics go to standard error. Data requested by commands such
as `vars get` goes to standard output. See [output](../cmdline.md#output-streams).

## Run one part

Using the `editor` action and group from [the tutorial](../getting-started.md):

```sh
batfiles apply-action --id editor --dry-run
batfiles apply-action --id editor
batfiles apply-group --group editor
```

Apply commands don't fetch remotes; run `sync` first if the action uses one.
Naming an action runs it even when it is disabled or its condition is false, so
use `sync --dry-run` to see what a normal run would do. See
[selection by command](../cmdline.md#selection-by-command).

## Skip something once

```sh
batfiles sync --skip-group editor
batfiles sync --skip-action editor
```

These choices apply only to this run. Repeat the options to skip several names.

## Disable something on this machine

```sh
batfiles disable-group editor
batfiles sync -v
batfiles enable-group editor
```

Use `disable-action` and `enable-action` for a single action. Choices persist
in machine-local configuration, outside the repository.
**Disabling does not remove anything already installed.** It prevents future
normal synchronization of the selected work.

## Remove something

Batfiles has no uninstall command and keeps no record of what it installed.
Removing an action, a child of its `source-dir`, or a clone-list entry leaves
what was already installed for it. To remove an action's content:

1. Delete the action from `batfiles.toml`, or disable it if other machines
   still use it.
2. Delete what it installed yourself. For a link, remove the link, not its
   target: `rm ~/.zshrc`, with no trailing slash for a link to a directory.
   Check a Git checkout for unpushed work before deleting it. A copy or
   download keeps whatever was already at its destination, so make sure the
   file there is one batfiles installed.
   - An action with `dest` installed exactly that path.
   - An action with `dest-dir` (`symlink-dir`, `copy-dir`, or `git-clone-list`)
     installed separate children inside it. **Delete those children and keep
     `dest-dir` itself**: other files share it, and it can be your home
     directory. `symlink-dir` and `copy-dir` install one child per direct child
     of `source-dir`, with a leading `.` under `dot-prefix = true`, limited by
     any `include` and `exclude`. `git-clone-list` installs one checkout per
     list entry, named as [clone names](../repoformat.md#what-a-clone-is-called)
     describe.

   For example, after `symlink-dir` with `source-dir = "files"`,
   `dest-dir = "~"`, and `dot-prefix = true`, remove `~/.zshrc` for
   `files/zshrc` and each other linked child, never `~`.
3. Delete any backups beside each destination or child, named
   `<path>.batfiles-backup-<time>`, once you no longer need them.

To stop using batfiles on a machine, also delete its machine state: by default
`~/.config/batfiles` and `~/.cache/batfiles`. See [state files](../state.md).

## Handle existing content

When a link or clone conflicts with unmanaged content, the default policy backs
it up beside the destination before installing. To leave conflicts untouched:

```sh
batfiles sync --no-overwrite
```

To decide at each conflict:

```sh
batfiles sync --interactive
```

The prompt offers backup and replacement, overwrite, or skip. Backups remain
until you remove them yourself. Seeds normally keep occupied destinations;
refreshing them opts into conflict handling. See [safety](../safety.md).

A failed run may have completed earlier actions. Fix the reported cause and
rerun; see [failure behavior](../cmdline.md#execution-failures) before assuming
nothing changed.
