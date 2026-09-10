# Batfiles

A dotfiles manager built around plain files and explicit composition.

Your dotfiles stay ordinary files in an ordinary Git repository. A
`batfiles.toml` at the root of that repository declares where each one belongs
in your home directory, and `batfiles sync` makes the home directory match.
There is no hidden ownership database, no per-repository install hook, and
nothing in the repository you cannot read with `cat`.

> **Status: early, but it installs a real repository.**
>
> Batfiles is being rebuilt from scratch. Four of the plan's eleven slices are
> done, and the author's own dotfiles are declared entirely in a
> `batfiles.toml` — links, seeded copies, a downloaded file, and the plugin
> repositories a shell script used to clone. `sync` installs a repository,
> `--dry-run` says what it would install, what you have disabled or asked to
> skip is left out, and `apply-action` and `apply-group` install one piece of it
> on its own.
>
> A record can also carry a `when` or an `unless`, so an action, or one
> repository in a plugin list, belongs to some machines and not others.
>
> What is not built is the composition: git remotes, and including one
> repository's actions into another. `init` and `clone` are
> not built either, so a fresh machine still clones its repository by hand.
> Every command but the ones named above parses its arguments and then exits
> saying it is not implemented yet. The plan, and the reason there is a rewrite,
> are in [`rewrite/README.md`](rewrite/README.md).

> [!NOTE]
> There is a sample repository to read and install:
> [batfiles-samples/simple-dotfiles](https://github.com/batfiles-samples/simple-dotfiles).

## What works today

Implemented so far: `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, `fetch-archive`, `git-clone`, `git-clone-list`.

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
- **A file can come from the network.** `fetch-file` downloads one to a
  destination, on the same missing-only terms, optionally checking it against a
  declared `sha256`. Nothing incomplete is ever installed: the download is
  built beside its destination and moved there once it is whole, so a transfer
  that stops early or a digest that does not match leaves the destination as it
  found it. What arrives is installed as a file, whatever it holds.
- **So can a whole directory.** `fetch-archive` downloads a tarball, gzipped or
  plain, and unpacks it — the same transfer and the same missing-only terms, with
  `archive-root` stripping the versioned directory a release tarball puts
  everything under. It is unpacked beside its destination and moved there once,
  and every entry's path is read before any of them is written: an entry that
  would land outside the destination fails the whole action rather than being
  quietly skipped. That includes the ones no check on a single path finds —
  nothing is written under a symlink the archive itself declares, and a target
  may not climb out past one.
- **A repository can be cloned and kept up to date.** `git-clone` clones one
  where nothing is, and on later runs brings the clone it finds forward —
  conservatively. Write `ref` to follow a particular branch, tag, or commit;
  without one it follows whatever branch the clone is on. It fetches, and
  fast-forwards only: a worktree with
  uncommitted changes, one on a branch that tracks nothing, and one holding
  commits the upstream does not are each left exactly as they are, with a
  warning rather than a failed run. batfiles will not discard work you did in a
  checkout it made for you. A destination holding something that is not a clone
  — a file, a symlink to a checkout elsewhere, a directory somebody else filled,
  or what an interrupted clone left behind — is refused by name rather than
  cloned over or fetched into.
- **Or a whole list of them.** `git-clone-list` names a plain text file in the
  repository — one repository per line, `key=value` metadata beside it — and one
  directory to clone them all under, which is how a vim or zsh plugin directory
  is usually kept. The list is read and checked as the repository is loaded, so a
  malformed line fails the run before the first action has touched anything. Each
  entry is then cloned on `git-clone`'s terms, in list order, and **one entry that
  cannot be cloned costs that entry rather than the run**: it is warned about by
  name and line, and the repositories after it are still installed. Worth knowing
  because of that: a `sync` that exits 0 may still have entries that did not
  clone, and the warnings are what say so.
- **Nothing else is replaced.** A destination holding a regular file, a
  directory, or a symlink pointing outside the repository is refused by name.
  Until there is a backup policy to give it back with, batfiles does not
  destroy what it did not create.
- **The manifest is read strictly.** An unknown key, an unknown action type, a
  section from a part of the format that does not parse yet, or a `source` or
  `dest` that cannot mean what it says is an error — never a setting that looks
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
  later branch introduces is what that file is for. Names are dotted addresses,
  so a name reaching into a repository this one will later include — `core.zshrc`
  — can be written down before there is anything for it to reach.
- **A repository can name what a fresh machine starts with switched off, and
  nothing acts on it yet.** `[default-disabled]` lists candidate actions and
  groups; batfiles checks them as it reads the manifest and does no more. The
  bootstrap that adopts them into `disabled.toml` arrives with `clone`, so
  declaring them changes no run today. It is the one section that is accepted
  without being acted on, and it is named here rather than left to be
  discovered.
- **Variables are merged from four places, and conditions read them.** A
  manifest's `[vars]` defaults, the machine-local `vars.toml` that `vars set`,
  `vars get`, and `vars unset` maintain, `BATFILES_VAR_*` in the environment,
  and `--var` on the command line are merged into one set, each overriding the
  ones before it. `sync -vv` prints the set it worked out, each variable with the
  value in force and the layers it overrode, which is how you check a precedence
  question. `vars get` writes a stored value alone to standard output, so
  `$(batfiles vars get editor)` is the value and not a sentence about it; the
  lines describing an edit name the key and never the value, which may be a token
  or a path that identifies a machine.
- **A record can say which machines it belongs to.** `when = "work"` runs an
  action only where the condition holds and `unless` is the other way round; a
  line of a plugin list takes the same two keys, which is how one list serves
  several machines. Conditions read the merged variables as bare names, the
  machine itself through `facts.os`, `facts.arch`, `facts.family`, and
  `facts.hostname`, and the environment through `env.HOME` and the like. They are
  parsed when the manifest is read, so a malformed one is a load error rather
  than a surprise partway through a run, and `sync -v` says which condition
  passed a record over. A condition that cannot be evaluated at all — a name
  nothing declares, a value that is not a boolean — stops the run for now;
  closing the gate with a warning instead arrives with the next step.
- **One action or one group can be applied on its own.** `apply-action --id
  zshrc` and `apply-group --group shell` carry out part of the same manifest,
  named rather than filtered — the same actions in the same order, with the same
  `--dry-run`. Naming a thing waives the reasons it would otherwise be passed
  over: `apply-action` runs the action whatever `disabled.toml` says about it or
  its group, and `apply-group` waives the group's own disable while still
  passing over a member disabled by its own `id`. A name nothing answers to is a
  failure rather than a run with nothing to do.
- **`version` prints the version.**
- **Unix only, mostly.** Windows compiles and every command runs there, but
  either symlink action is refused by name rather than performed — the platform
  needs a file-against-directory distinction and a privilege check that are not
  built, and no step schedules them yet. The other three action types work
  everywhere, so a Windows run can create directories and seed copies but cannot
  install a link.

Everything else — `init`, `clone`, `vars list`, and `vars refresh` — parses its
arguments and exits 2.

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
[`rewrite/steps.md`](rewrite/steps.md):

| Slice | What arrives                                                        |
|-------|---------------------------------------------------------------------|
| 5     | Variables, and `when`/`unless` conditions                           |
| 6–7   | Git remotes, and splicing a remote's actions into your own manifest |
| 8     | `init` and `clone` for new machines, with default-disabled adoption |
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

Two cargo subcommands are needed beyond the pinned toolchain: `cargo install
cargo-deny cargo-xwin --locked`. The second is what lets an ubuntu machine run
clippy against Windows — the TLS stack under `fetch-file` compiles C, so that
check needs headers targeting MSVC, which `cargo xwin` fetches and caches.

Without `task` installed, the underlying commands are `cargo fmt --all`,
`cargo clippy --all-targets`, and `cargo test`.

## License

[MIT](./LICENSE)
