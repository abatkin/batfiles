# Batfiles

A dotfiles manager built around plain files and explicit composition.

Your dotfiles stay ordinary files in an ordinary Git repository. A
`batfiles.toml` at the root of that repository declares where each one belongs
in your home directory, and `batfiles sync` makes the home directory match.
There is no hidden ownership database, no per-repository install hook, and
nothing in the repository you cannot read with `cat`.

> **Status: early, and not yet useful as a dotfiles manager.**
>
> Batfiles is being rebuilt from scratch. Two of the plan's eleven slices are
> done and the third is under way. `sync` installs a repository, `--dry-run`
> says what it would install, and what you have disabled or asked to skip is
> left out. Every other command parses its arguments and then exits saying it is
> not implemented yet. The plan, and the reason there is a rewrite, are in
> [`rewrite/README.md`](rewrite/README.md).

> [!NOTE]
> There is a sample repository to read and install:
> [batfiles-samples/simple-dotfiles](https://github.com/batfiles-samples/simple-dotfiles).

## What works today

Implemented so far: `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`.

- **`sync` reads a repository and installs it.** It executes the manifest's
  actions in declaration order, each one seeing the filesystem the previous
  ones left. The first failure stops the run; what earlier actions did stays
  done.
- **Symlinks are created, repaired, or left alone.** A link batfiles would have
  made is repointed when the manifest changes, because a symlink holds no
  content of its own and what it pointed at is untouched. A link that is
  already right is left alone and says nothing, since an installed repository
  is the ordinary case.
- **A whole directory can be linked child by child.** `symlink-dir` names a
  directory in the repository and one destination directory, and links every
  direct child into it — optionally dotting each name on the way. Adding a file
  to that directory installs it on the next `sync` with no change to the
  manifest, which is the point of it.
- **A directory can be asked for on its own.** `create-dir` makes one and
  nothing else — `mkdir -p`, for a plugin root or a cache that some other tool
  fills in. An existing directory is left exactly as it is, contents and all.
- **Files can be seeded instead of linked.** `copy` and `copy-dir` install a
  copy the user then owns — a machine-local override, a template to fill in —
  and install it *only* where nothing is. Editing it afterwards is the point, so
  a later `sync` finds it occupied and leaves it alone rather than putting the
  original back. Permissions come across, including the executable bit.
- **Nothing else is replaced.** A destination holding a regular file, a
  directory, or a symlink pointing outside the repository is refused by name.
  Until there is a backup policy to give it back with, batfiles does not
  destroy what it did not create.
- **The manifest is read strictly.** An unknown key, an unknown action type, a
  section belonging to an unbuilt part of the format, or a `source` or `dest`
  that cannot mean what it says is an error — never a setting that looks
  accepted and does nothing. All of it is caught while the file is read, so a
  manifest batfiles will not honor stops the run before it installs half of it.
- **A run can be asked what it would do.** `sync --dry-run` reports the whole
  plan — one line per link, copy, and directory, in the tense that says it has
  not happened — and writes nothing. Every action inspects the real filesystem
  and then stops short of the write, so what you read is what the run decided,
  not a simulation of one.
- **An option that is not live yet is refused rather than ignored.** `sync
  --refresh-content` exits 2 naming the option, because silently accepting it
  would let you believe your content had been refreshed.
- **Actions and groups can be turned off, for good or for one run.**
  `disable-action`, `enable-action`, `disable-group`, and `enable-group` record
  names in a machine-local `disabled.toml`, atomically and idempotently, and
  every later `sync` passes those actions over. `--skip-action` and
  `--skip-group` — or `BATFILES_SKIP_ACTIONS` and `BATFILES_SKIP_GROUPS` — do
  the same for one invocation without writing anything down. A skip that matches
  nothing warns, since it was typed for this run; a pre-registered
  `disabled.toml` entry that matches nothing is silent, since naming something a
  later branch introduces is what that file is for.
- **`version` prints the version.**
- **Unix only, mostly.** Windows compiles and every command runs there, but
  either symlink action is refused by name rather than performed — the platform
  needs a file-against-directory distinction and a privilege check that are not
  built, and no step schedules them yet. The other three action types work
  everywhere, so a Windows run can create directories and seed copies but cannot
  install a link.

Everything else — `init`, `clone`, `apply-action`, `apply-group`, and `vars` —
parses its arguments and exits 2.

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
├── files/{ackrc,curlrc,inputrc}
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

# Every direct child of `files/`, dotted on the way into the home. Adding one
# there needs no change here.
[[actions]]
type = "symlink-dir"
id = "rcfiles"
group = "shell"
source-dir = "files"
dest-dir = "~"
dot-prefix = true
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
linked /home/you/.ackrc -> /home/you/dotfiles/files/ackrc
linked /home/you/.curlrc -> /home/you/dotfiles/files/curlrc
linked /home/you/.inputrc -> /home/you/dotfiles/files/inputrc
```

Missing parent directories are created. Run it again and it says nothing at
all, because nothing changed; `-v` reports what it looked at, and heads each
action's lines with the record that produced them and the `group` it names.

A group is how you talk about several actions at once. `batfiles sync
--skip-group editor` leaves the `nvim` link out of one run, and `batfiles
disable-group editor` leaves it out of every run until you enable it again:

```console
$ batfiles sync -v --skip-group editor
symlink nvim (group editor) - skipped: `editor` from --skip-group
```

And where something is already in the way:

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
| 3     | Default-disabled bootstrap entries, and `apply-action`/`apply-group` |
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
- [Local state files](docs/state.md) — `disabled.toml`, and how batfiles
  rewrites the documents it owns.
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
