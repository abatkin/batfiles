# Batfiles Product Goals

This document states intended scope, not status. What is built today is in the
project README's [What works today](../README.md#what-works-today); anything
marked *intended* here is specified in [`docs/future/`](future/) and binds
nothing.

## Purpose

Batfiles is a dotfiles manager that turns one user-owned repository, plus any
explicitly selected reusable sources, into a predictable installation plan for
a home directory.

The central promise is simple: dotfiles remain normal files in normal Git
repositories, while one automation-friendly command can bootstrap or
synchronize a machine without relying on ad hoc install hooks, required
prompts, or a hidden ownership database.

## Product Model

The user points batfiles at a **leaf repository**. That repository is the
machine's entry point and owns the configuration that is applied. It may:

- contain local files;
- declare actions to install those files into the home directory;
- declare reusable **remotes** backed by Git repositories, individual files,
  or archives;
- reference specific files from those remotes; and
- explicitly include some or all of a Git remote's actions.

Declaring a remote only materializes it inside the leaf repository's
tool-owned `remotes/` tree. It never installs remote content by itself.
Composition is explicit, scoped, and limited to one level: included remotes do
not (on their own) recursively pull in their own remotes or included action sets.

A standalone `batfiles` binary performs planning and installation, and
plain-file repositories contain the actual dotfiles and declarative
`batfiles.toml` configuration.

Each release publishes that binary for Linux, macOS, and Windows in a
[release tree](distribution.md). Getting it onto a machine is *intended*, and
specified in [distribution](future/distribution.md):

1. **One command bootstraps a new machine.** A hosted installer, piped into a
   shell, uses a `batfiles` it finds or downloads a verified one to
   `~/.local/bin`, then runs `clone` against the user's repository.
2. **A checkout can install itself.** A small stub that `init` writes into the
   leaf repository finds or fetches `batfiles` the same way and synchronizes
   its own checkout. It is frozen, so a repository never has to maintain it.
3. **Nothing depends on one site.** Releases follow a static layout that any
   host can serve, so a user can install from a site they control with the same
   one-liner. A fork's releases point at the fork without configuration.
4. **Upgrading is the user's choice.** `batfiles update` replaces the binary
   when the user runs it, and nothing runs it for them. A repository may pin a
   minimum release in its stub.
5. **The machine stays the user's.** Installers write only to their install
   directory and never edit shell startup files, which are the dotfiles the
   first synchronization installs.

Windows gets an equivalent installer and stub.

## Core Goals

### Keep repositories plain and understandable

- Repositories remain directly inspectable and editable file trees.
- Directory names have no magic meaning; behavior comes from explicit actions.
- Symlinked files stay connected to their repository, so editing an installed
  file edits the source-controlled file.
- Configuration is strict: malformed records and unknown fields fail clearly
  instead of being silently ignored.

### Make reuse explicit and composable

- Support Git, file, and archive remotes as named sources.
- Let a leaf action reference a particular path from a particular remote.
- Let a leaf repository splice a Git remote's actions into its own ordered
  action list ([`include-remote`](repoformat.md#include-remote)), taking all of
  them or [part of one](repoformat.md#selecting-part-of-a-remote), with
  [per-inclusion variables](repoformat.md#variables-for-one-inclusion) over [the
  remote's own](repoformat.md#variables-an-included-remote-declares).
- Resolve every repository-backed source path against exactly one repository;
  never implicitly merge or search all sources.

### Cover the practical installation operations

The declarative action model covers installing repository files, seeding copies,
creating directories, cloning Git repositories singly and from manifests,
fetching files and archives, and including a reusable remote's actions. The
[action schema](repoformat.md#actions) specifies the types a manifest may
declare; the project README answers "what can `sync` actually do" and is checked
against the `Action` enum by `tests/hygiene.rs`.

Normal synchronization is convergence-oriented but intentionally asymmetric:
symlinks are repaired and Git clones are advanced under the conservative
[update policy](safety.md#git-updates), while copied files, fetched content, and
created directories are left alone once they exist unless a run explicitly
[refreshes](safety.md#refreshing-seeds) the copies and fetched content.
Something in a destination's way is kept as a backup rather than lost. Explicit
action or group application uses the same behavior as synchronization.

### Produce one predictable plan

- Preserve declaration order, including remote actions expanded in place, and
  execute actions in that order.
- Build the complete knowable structural plan before executing actions, leaving
  out what is disabled, skipped, or gated off by a false condition. The command
  reference defines [what a run selects](cmdline.md#selecting-what-a-run-does).
- Let each action inspect the filesystem as it begins, using the state left by
  earlier successful actions, and act on what it finds there — rather than
  freezing a decision for every action from the state at the start of the run.
- Do not perform cross-action destination conflict detection; repository
  authors are responsible for intentional or accidental overlaps.
- Say what a run would do without doing it, and whether the plan it could build
  is complete or partial: [dry-run behavior](cmdline.md#dry-run-behavior).

### Adapt declaratively to each machine

- Allow string variables and expression-based `when`/`unless` conditions on
  remotes, actions, and manifest entries.
- Provide built-in machine facts and access to environment variables.
- Layer repository defaults, per-inclusion overrides, persisted machine-local
  choices, environment overrides, and one-shot command-line values with clear
  [precedence](environment.md#variable-precedence).
- Keep every variable a string, whichever layer produced it; a condition is the
  one place a string becomes a decision ([variables](repoformat.md#variables)).
- Cache command-backed variables for facts that must be discovered locally,
  with explicit refresh controls, and let the leaf repository decide whether a
  remote's [dynamic variables](repoformat.md#dynamic-variables) run, with a
  `vars refresh` command to run them ahead of a run.
- Allow actions and groups to be persistently enabled or disabled or skipped for
  one run, and (*intended*) default-disabled during first-machine bootstrap.
- Express machine variation through these declarative controls rather than
  arbitrary per-repository install hooks.

### Define safety policy separately

The [safety model](safety.md) separately defines destination resolution, symlink
traversal, archive handling, replacement behavior, Git updates, and failure
recovery. Keeping those rules in one place prevents individual action
specifications from developing inconsistent safety guarantees. Safety against
concurrent writers and the trust rules remote content will need are
[*intended*](future/safety.md).

### Stay automation-friendly

- Require no interaction by default; prompting occurs only when explicitly
  requested.
- Use clear [exit statuses](cmdline.md#exit-statuses), send warnings and errors
  to [standard error](cmdline.md#output-streams), and keep requested data and
  dry-run output suitable for scripts.
- Keep input precedence explicit and predictable; the environment specification
  defines the authoritative [precedence rules](environment.md).
- Select the leaf repository, destination home, config directory, and cache
  directory explicitly through options or environment variables, with
  conventional defaults.

### Minimize and separate local state

Batfiles should persist only deliberate machine-local choices and disposable
cache data:

- machine-local variable overrides and disabled action/group lists are user
  configuration;
- dynamic variable results are disposable cache data; and
- installed home-directory content has no persistent ownership record.

State-file updates should be atomic whole-document replacements. Configuration
and cache locations should follow XDG conventions while remaining overridable
for testing and unusual installations. The [state specification](state.md) owns
the documents and their lifecycle.
