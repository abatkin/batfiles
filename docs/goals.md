# Batfiles Product Goals

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

The stable bootstrap experience has three pieces:

1. A small `install.sh` checked into the leaf repository uses a `batfiles`
   binary found on `PATH`, or downloads one to `~/.local/bin`, and invokes
   synchronization.
2. A standalone `batfiles` binary performs planning and installation.
3. Plain-file repositories contain the actual dotfiles and declarative
   `batfiles.toml` configuration.

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
  action list, with action/group selection and per-inclusion variables.
- Resolve every repository-backed source path against exactly one repository;
  never implicitly merge or search all sources.

### Cover the practical installation operations

The declarative action model covers installing repository files, seeding copies,
creating directories, cloning Git repositories singly and from manifests,
fetching files and archives, and including a reusable remote's actions. The
[future repository format](future/repoformat.md#actions) enumerates the intended
set.

**Implemented so far: `symlink`, `symlink-dir`.**

That line is the answer to "what can `sync` actually do", and it gains an action
each time one is built. `tests/hygiene.rs` checks it against the `Action` enum,
here and in the project `README.md`, so it cannot fall behind the build.

Normal synchronization is convergence-oriented but intentionally asymmetric:
symlinks can be repaired, while copied files, fetched content, created
directories, and clones are preserved after creation. Explicit action or group
application uses the same behavior as synchronization.

### Produce one predictable plan

- Preserve declaration order, including remote actions expanded in place.
- Execute actions in that order, with each action observing changes made by
  earlier actions.
- Do not perform cross-action destination conflict detection; repository
  authors are responsible for intentional or accidental overlaps.
- Filter disabled, skipped, or condition-false items before adding them to the
  structural plan.
- Build the complete knowable structural plan before executing actions.
  Dynamic-variable resolution is part of planning and may execute commands or
  update its cache.
- Determine concrete filesystem effects as the first phase of each action's
  execution, using the state left by earlier successful actions.
- Provide dry-run output that clearly says what would be created, updated,
  skipped, backed up, fetched, or cloned, and whether the plan is complete or
  partial. The command-line specification defines the shared
  [dry-run behavior](future/cmdline.md#dry-run-behavior).

### Adapt declaratively to each machine

- Allow string variables and expression-based `when`/`unless` conditions on
  remotes, actions, and manifest entries.
- Provide built-in machine facts and access to environment variables.
- Layer repository defaults, per-inclusion overrides, persisted machine-local
  choices, environment overrides, and one-shot command-line values with clear
  precedence.
- Use the repository format's shared [string-valued variable
  model](future/repoformat.md#string-valued-variables) consistently.
- Support cached command-backed variables for facts that must be discovered
  locally, with explicit refresh controls and a way for the leaf repository to
  forbid executing a remote's dynamic variable commands.
- Allow actions and groups to be persistently enabled or disabled, skipped for
  one run, or default-disabled during first-machine bootstrap.
- Express machine variation through these declarative controls rather than
  arbitrary per-repository install hooks.

### Define safety policy separately

The [safety model](future/safety.md) separately defines the guiding rules for
destination resolution, symlink traversal, archive handling, replacement and
backup behavior, Git updates, and failure recovery. Keeping those rules in one
place prevents individual action specifications from developing inconsistent
safety guarantees.

### Stay automation-friendly

- Require no interaction by default; prompting occurs only when explicitly
  requested.
- Use clear [exit statuses](cmdline.md#exit-statuses), send warnings and errors
  to [standard error](cmdline.md#output-streams), and keep requested data and
  dry-run output suitable for scripts.
- Keep input precedence explicit and predictable; the environment specification
  defines the authoritative [precedence rules](future/environment.md#general-precedence).
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
for testing and unusual installations.
