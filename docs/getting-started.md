# Your first repository

Create a repository, declare one file to install, and preview the result. This
example works on Linux, macOS, and Windows. It installs a demonstration file
under your home directory without changing any application's configuration.

You need [batfiles](guides/install.md) on your `PATH` and Git installed.

## Create the repository

In a directory where `dotfiles-demo` does not already exist:

```sh
mkdir dotfiles-demo
cd dotfiles-demo
batfiles init
```

These commands also work in PowerShell 7. Stay in this directory for the rest
of the tutorial. `init` creates a starter manifest, `files/`, `bin/`, an
appropriate `.gitignore`, and checkout installers, and initializes Git if needed.
The starter manifest installs nothing.

## Declare a file

Create `files/editor.toml` in your editor with this content:

```toml
theme = "dark"
```

Replace the commented starter `batfiles.toml` with:

```toml
[[actions]]
type = "copy"
id = "editor"
group = "editor"
source = "files/editor.toml"
dest = "~/.config/batfiles-demo/editor.toml"
```

`source` is relative to this repository. `~` in `dest` names your destination
home. An action's `id` lets you run it individually; `group` lets you select
several related actions together.

`copy` installs a starting file, called a *seed*. Later runs keep any existing
destination, so you can edit it locally. To keep a live connection to the file
in Git instead, Unix users can choose a [symlink](#use-a-live-link-on-unix).

## Preview and apply

```sh
batfiles sync --dry-run
batfiles sync
batfiles sync -v
```

If the destination is vacant, the preview reports `would copy`; the real run
reports `copied` and creates any missing parent directories. Open
`~/.config/batfiles-demo/editor.toml` to see the installed content. On Windows,
that path is inside your user profile directory.

The verbose run reports that the destination is kept. An ordinary repeat of
`batfiles sync` is silent when nothing changes. If the file was already there
before the tutorial, this copy action keeps it too.

Dry-run leaves installed content alone; see [what it can still do](cmdline.md#dry-run-behavior).

## Change the starting content

Change `files/editor.toml` to `theme = "light"`. Running `sync` again keeps the
installed file. To deliberately replace it with the changed starting content:

```sh
batfiles apply-action --id editor --refresh-content
```

When content differs, the default policy backs up the existing destination
beside it before installing the replacement. `--interactive` asks how to handle
that conflict; `--no-overwrite` keeps it. See [refreshing seeds](safety.md#refreshing-seeds).

## Use a live link on Unix

For real configuration that should always use the repository's copy, use
`symlink` instead of `copy`. For example, after adding your shell configuration
at `files/zshrc`:

```toml
[[actions]]
type = "symlink"
id = "zshrc"
group = "shell"
source = "files/zshrc"
dest = "~/.zshrc"
```

Preview before applying. An unmanaged file in the way is backed up by default;
use `--no-overwrite` to skip conflicts. Once linked, editing `~/.zshrc`
edits `files/zshrc` in the repository. **Symlink actions fail on Windows.** See
[replacement rules](safety.md#replacing-what-is-already-there) for other node types.

## Keep the repository in Git

```sh
git add batfiles.toml .gitignore files bin install.sh install.ps1
git commit -m "Add my dotfiles"
```

Push it to your preferred Git host when ready. The checkout installers let you
[bootstrap another machine](guides/install.md#use-an-existing-dotfiles-repository).
Keep credentials and other secrets out of the repository.

Next: [Everyday use](guides/everyday.md) explains selective runs, and
[Configure a machine](guides/machines.md) introduces machine-specific choices.
