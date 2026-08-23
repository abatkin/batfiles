# Batfiles

A dotfiles manager built around plain files and explicit composition.

Your dotfiles stay ordinary files in an ordinary Git repository. A
`batfiles.toml` at the root of that repository declares where each one belongs
in your home directory, and `batfiles sync` makes the home directory match.
There is no hidden ownership database, no per-repository install hook, and
nothing in the repository you cannot read with `cat`.

> **Status: early, and not yet useful as a dotfiles manager.**
>
> Batfiles is being rebuilt from scratch, and the first of the plan's eleven
> slices is done. On Unix, `sync` installs symlinks — and that is the whole of
> it; every other command parses its arguments and then exits saying it is not
> implemented yet. The plan, and the reason there is a rewrite, are in
> [`rewrite/README.md`](rewrite/README.md).

## What works today

Implemented so far: `symlink`.

- **`sync` reads a repository and installs it.** It executes the manifest's
  actions in declaration order, each one seeing the filesystem the previous
  ones left. The first failure stops the run; what earlier actions did stays
  done.
- **Symlinks are created, repaired, or left alone.** A link batfiles would have
  made is repointed when the manifest changes, because a symlink holds no
  content of its own and what it pointed at is untouched. A link that is
  already right is left alone and says nothing, since an installed repository
  is the ordinary case.
- **Nothing else is replaced.** A destination holding a regular file, a
  directory, or a symlink pointing outside the repository is refused by name.
  Until there is a backup policy to give it back with, batfiles does not
  destroy what it did not create.
- **The manifest is read strictly.** An unknown key, an unknown action type, or
  a section belonging to an unbuilt part of the format is an error — never a
  setting that looks accepted and does nothing.
- **An option that is not live yet is refused rather than ignored.** `sync
  --dry-run` exits 2 naming the option, because silently accepting it would let
  you believe a dry run had happened.
- **`version` prints the version.**
- **Unix only, so far.** Windows compiles and every command runs there, but a
  `symlink` action is refused by name rather than performed — the platform needs
  a file-against-directory distinction and a privilege check that are not built,
  and no step schedules them yet. Since `symlink` is the only action there is,
  Windows can currently install nothing.

Everything else — `init`, `clone`, the four enable/disable commands,
`apply-action`, `apply-group`, and `vars` — parses its arguments and exits 2.

## Example

The repository below is [`tests/fixtures/leaf`](tests/fixtures/leaf), which the
test suite installs on every run, so it cannot drift from what batfiles
actually does.

```text
dotfiles/
├── batfiles.toml
├── README.md
├── bin/batgrep
├── editor/nvim/
│   ├── init.lua
│   └── lua/plugins.lua
├── git/{gitconfig,gitignore}
└── shell/{zshrc,zshenv,aliases.zsh}
```

Only `batfiles.toml` has intrinsic meaning. Every other name in the tree
becomes meaningful when an action references it:

```toml
[[actions]]
type = "symlink"
id = "zshrc"
group = "shell"
source = "shell/zshrc"
dest = "~/.zshrc"

[[actions]]
type = "symlink"
id = "nvim"
group = "editor"
source = "editor/nvim"
dest = "~/.config/nvim"
```

With that repository at `~/dotfiles`, which is where batfiles looks by default:

```console
$ batfiles sync
linked /home/you/.zshrc -> /home/you/dotfiles/shell/zshrc
linked /home/you/.zshenv -> /home/you/dotfiles/shell/zshenv
linked /home/you/.config/zsh/aliases.zsh -> /home/you/dotfiles/shell/aliases.zsh
linked /home/you/.gitconfig -> /home/you/dotfiles/git/gitconfig
linked /home/you/.config/git/ignore -> /home/you/dotfiles/git/gitignore
linked /home/you/.config/nvim -> /home/you/dotfiles/editor/nvim
linked /home/you/.local/bin/batgrep -> /home/you/dotfiles/bin/batgrep
```

Missing parent directories are created. Run it again and it says nothing at
all, because nothing changed; `-v` reports what it looked at. And where
something is already in the way:

```console
$ batfiles sync
error: /home/you/.gitconfig already exists and is a regular file; move it aside and run sync again
```

An exit status of `0` means the command did what was asked, `1` that it ran and
failed partway, and `2` that it did not run at all — a usage error, or a
command or option that is not built yet.

## Building

No binaries are published yet, so build from source. The toolchain is pinned in
`rust-toolchain.toml`.

```sh
cargo build --release   # target/release/batfiles
```

## Not built yet

Roughly in the order it is planned, from
[`rewrite/steps.md`](rewrite/steps.md):

| Slice | What arrives                                                        |
|-------|---------------------------------------------------------------------|
| 1     | The `create-dir` and `copy` actions                                 |
| 2     | `--dry-run`                                                         |
| 3     | Groups, enable/disable, `--skip`, `apply-action`, `apply-group`     |
| 4     | Fetching files and archives, and cloning Git repositories           |
| 5     | Variables, and `when`/`unless` conditions                           |
| 6–7   | Git remotes, and splicing a remote's actions into your own manifest |
| 8     | `init` and `clone` for setting up a new machine                     |
| 9     | Dynamic variables, file and archive remotes, `--refresh-content`    |
| 10    | Released binaries and an `install.sh` one-liner                     |

## Documentation

- [Product goals](docs/goals.md) — the product model and what it is for.
- [Command-line surface](docs/cmdline.md) — commands, global options, output
  streams, exit statuses.
- [Repository format](docs/repoformat.md) — the manifest, and what an action
  may declare.
- [Environment variables](docs/environment.md) — the inputs batfiles reads.

[`docs/`](docs/README.md) describes behavior that runs, and nothing else.
Anything specified but not built is in [`docs/future/`](docs/future/), which
binds nothing.

## Development

[go-task](https://taskfile.dev) drives everything, and CI runs the identical
entry point.

```sh
task ci      # fmt + lint + test + deny + build + build:release (what CI runs)
task test    # project tests
task fmt     # formatting check
task lint    # clippy with warnings denied, for the host and for Windows
task build   # debug build
```

Without `task` installed, the underlying commands are `cargo fmt --all`,
`cargo clippy --all-targets`, and `cargo test`.

## License

[MIT](./LICENSE)
