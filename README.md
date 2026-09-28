# Batfiles

A dotfiles manager built around plain files and explicit composition.

Your dotfiles stay ordinary files in an ordinary Git repository. A
`batfiles.toml` at the root of that repository declares where each one belongs
in your home directory, and `batfiles sync` makes the home directory match.
There is no hidden ownership database, no per-repository install hook, and
nothing in the repository you cannot read with `cat`.

> **Status: early, but it installs a real repository.**
>
> Batfiles is being rebuilt from scratch. The author's own dotfiles are declared
> entirely in a `batfiles.toml` — links, seeded copies, a downloaded file, and
> shell plugin repositories. `sync` installs a repository,
> `--dry-run` says what it would install, what you have disabled or asked to
> skip is left out, and `apply-action` and `apply-group` install one piece of it
> on its own.
>
> A record can also carry a `when` or an `unless`, so an action, or one
> repository in a plugin list, belongs to some machines and not others. What a
> condition reads can be written down, or found out by running a command.
>
> Composition is built: a `[remotes]` section names other Git repositories,
> files, and archives, `sync` brings each one into your repository's own
> `remotes/` tree, an action can install from one, and `include-remote` splices
> a Git remote's own actions into your manifest at the position that includes it. `init` starts a new repository
> in the current directory, and `clone` brings an existing one down onto a fresh
> machine, adopts the bootstrap policy it declares, and installs it — all in one
> command. See the [command reference](docs/cmdline.md#what-runs-today) for
> supported commands and the [roadmap](docs/future/roadmap.md) for remaining
> work.

> [!NOTE]
> There is a sample repository to read and install:
> [batfiles-samples/simple-dotfiles](https://github.com/batfiles-samples/simple-dotfiles).

## What works today

Implemented so far: `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, `fetch-archive`, `git-clone`, `git-clone-list`, `include-remote`.

- **Install local files and directories.** `symlink` and `symlink-dir` create
  links; `copy` and `copy-dir` seed editable copies; `create-dir` creates a
  container. See the [action reference](docs/repoformat.md#actions) and
  [destination policy](docs/safety.md#replacing-what-is-already-there).
- **Download files and archives.** `fetch-file` downloads a file and
  `fetch-archive` unpacks a plain or gzipped tarball, from an `http://`,
  `https://`, or `file://` URL, optionally checking a SHA-256 digest. Both install only at vacant destinations using
  [staging and publication](docs/safety.md#staging-and-publication).
  Archives follow the [extraction safety rules](docs/safety.md#archive-extraction).
- **Manage Git repositories and plugin lists.** `git-clone` clones or updates a
  repository; `git-clone-list` processes a text list of repositories. Both use
  the [conservative Git update policy](docs/safety.md#git-updates), with optional
  [branch, tag, or commit selection](docs/repoformat.md#ref-following-one-branch-tag-or-commit).
  Lists can continue after [recoverable entry failures](docs/cmdline.md#clone-list-entry-failures),
  so check warnings even when the command succeeds.
- **Install content from other repositories.** `[remotes]` declares Git
  repositories, files, and archives that `sync` materializes under
  `remotes/<id>`, fetching a file or archive again only when its declaration
  changes. Actions reference their content with paths such as
  `@core/files/zshrc`, or `@pathogen` for a file. `include-remote` takes the
  actions a Git remote's own manifest declares into the list at its position, addressable as
  `corp.zshrc`, either all of them or the ones its `install-actions`,
  `install-groups`, `exclude-actions`, and `exclude-groups` select. The remote's
  conditions read what it declared in its own `[vars]`, which the inclusion's
  `vars` and the leaf's `[vars]` override. See
  [remotes](docs/repoformat.md#remotes) and
  [include-remote](docs/repoformat.md#include-remote); add `remotes/` to your
  `.gitignore`.
- **Apply all or part of a manifest.** `sync` executes actions in declaration
  order. `apply-action --id zshrc` and `apply-group --group shell` select one
  part. See [selection by command](docs/cmdline.md#selection-by-command) for
  exclusions and the [execution failure policy](docs/cmdline.md#execution-failures).
- **Preview a run.** `--dry-run` reports intended work using the current
  filesystem. See [dry-run behavior](docs/cmdline.md#dry-run-behavior) for its
  guarantees and reporting limits.
- **Disable actions and groups.** The enable/disable commands persist choices
  in `disabled.toml`; skip options and environment variables apply to one run.
  See [selection](docs/cmdline.md#selecting-what-a-run-does).
- **Configure variables and conditions.** `vars set`, `get`, `unset`, and `list`
  manage or inspect variables. Manifest defaults, per-inclusion overrides,
  machine values, environment variables, and CLI overrides follow the
  [variable precedence rules](docs/environment.md#variable-precedence).
  Actions, clone-list entries, and remotes accept [`when` or `unless`](docs/repoformat.md#conditions).
- **Discover values with commands.** A table under `[vars]` is a
  [dynamic variable](docs/repoformat.md#dynamic-variables): a command whose
  output or exit status is the value, cached in `dynamic-vars.toml` for as long
  as its `cache` says. An included remote's run only where the leaf allows them.
  `vars refresh` runs them ahead of a run, all of them or the ones it names; see
  [`vars refresh`](docs/cmdline.md#vars-refresh).
- **Validate declarations.** Invalid manifests fail when read, under the
  [manifest validation rules](docs/repoformat.md#reading-the-manifest).
- **Start a repository.** `init` lays `batfiles.toml`, `.gitignore`, `bin/`, and
  `files/` into the current directory and runs `git init`, refusing rather than
  overwriting anything already there. See [`init`](docs/cmdline.md#init).
- **Set up a new machine.** `clone <url>` clones a repository into the selected
  batfiles directory — which must not already exist — and synchronizes it in the
  same command. A repository's
  [`[default-disabled]`](docs/repoformat.md#default-disabled-bootstrap-entries)
  says what a fresh machine starts with switched off; `clone`'s enable and
  disable options and the `BATFILES_*` bootstrap lists overrule it. See
  [`clone`](docs/cmdline.md#clone).
- **Report the installed version.** `version` prints the version.
- **Platform support.** Symlink actions are supported only on Unix. On Windows,
  executing either symlink action fails the run with an error naming the action
  type. Directory, copy, fetching, and Git actions have Windows implementations.
  CI checks Windows compilation, but does not run Windows tests.

Every command works. An option that parses and is not built yet is
[refused](docs/cmdline.md#unimplemented-options) with exit status 2.

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

# And one this repository does not want everywhere. `work` is a variable, set
# here for every machine and overridden on the ones where it is true.
[vars]
work = "false"

[[actions]]
type = "create-dir"
id = "work-cache"
group = "shell"
dest = "~/.cache/work-tools"
when = "work"
```

With that repository at `~/dotfiles`, which is the fallback when the current
directory has no `batfiles.toml`:

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

You can also run `batfiles sync` from the root of any repository containing a
`batfiles.toml`; explicit `--batfiles-dir` and `BATFILES_DIR` selections take
precedence. The full order is documented under
[location selection](docs/environment.md#location-selection).

On a machine that has no repository yet, `batfiles clone <url>` does both halves
at once: it clones into `~/dotfiles` — or wherever `--batfiles-dir` names — and
then runs exactly the synchronization above. Nothing may be at that path
already, because `clone` is what creates it.

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

A condition is the repository's own say in the same question, and `-v` reports it
the same way. `batfiles vars set work true` on the work laptop, or `--var
work=true` for one run, is what opens it:

```console
$ batfiles sync -v
create-dir work-cache (group shell) - skipped: when "work" is false
$ batfiles sync -v --var work=true
create-dir work-cache (group shell)
created /home/you/.cache/work-tools
```

Groups are also how you install several actions on their own. `batfiles
apply-group --group editor` runs that group and nothing else, and `batfiles
apply-action --id nvim` runs the one action — including when you have disabled
it, since asking for something by name is how you say so for one invocation:

```console
$ batfiles disable-group editor
disabled group `editor`
$ batfiles apply-action --id nvim
linked /home/you/.config/nvim -> /home/you/dotfiles/editor/nvim
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
[the roadmap](docs/future/roadmap.md):

| Slice | What arrives                                                        |
|-------|---------------------------------------------------------------------|
| 9     | `--refresh-content`, backups, and conflict handling                 |
| 10    | Released binaries and an `install.sh` one-liner                     |

## Documentation

- [Product goals](docs/goals.md) — the product model and what it is for.
- [Command-line surface](docs/cmdline.md) — commands, global options, output
  streams, exit statuses.
- [Repository format](docs/repoformat.md) — the manifest, and what an action
  may declare.
- [Local state files](docs/state.md) — `vars.toml`, `disabled.toml`, and how batfiles
  rewrites the documents it owns.
- [Environment variables](docs/environment.md) — the inputs batfiles reads.
- [Architecture](docs/architecture.md) — the rules the implementation is
  written to, and what its tests look like.

[`docs/`](docs/README.md) describes behavior that runs, and nothing else.
Anything specified but not built is in [`docs/future/`](docs/future/), which
binds nothing.

## Development

[go-task](https://taskfile.dev) drives everything, and CI runs the identical
entry point.

```sh
task ci           # fmt + lint + test + test:docker + deny + builds (what CI runs)
task test         # project tests
task test:docker  # the acceptance, on a pristine machine in a container
task fmt          # formatting check
task lint         # clippy with warnings denied, for the host and for Windows
task build        # debug build
```

`task test:docker` clones a repository onto a container that has nothing on it,
which is where the defaults under a real `$HOME` are exercised. It needs docker
or podman, and says it did not run rather than failing where there is neither.
On macOS and Windows, where `task build` produces a binary no Linux container
can execute, the image builds batfiles itself.

Two cargo subcommands are needed beyond the pinned toolchain: `cargo install
cargo-deny cargo-xwin --locked`. The second is what lets an ubuntu machine run
clippy against Windows — the TLS stack under `fetch-file` compiles C, so that
check needs headers targeting MSVC, which `cargo xwin` fetches and caches.

Without `task` installed, the underlying commands are `cargo fmt --all`,
`cargo clippy --all-targets`, and `cargo test`.

## License

[MIT](./LICENSE)
