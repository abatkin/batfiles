# Batfiles Repository Format

The part of the repository format that runs today: where the manifest lives, and
how it is read. The schema itself — actions, groups, remotes, variables, and
conditions — is in [`future/repoformat.md`](future/repoformat.md) until those
records parse.

## Repository layout

A batfiles repository is an ordinary file tree with a `batfiles.toml` at its
root.

```text
dotfiles/
├── batfiles.toml
└── ...
```

Only `batfiles.toml` has intrinsic meaning. Every other name in the tree becomes
meaningful when an action references it, and means nothing on its own.

The **leaf repository** is the one a command works on, selected by
`--batfiles-dir` or `BATFILES_DIR` and defaulting to `<selected-home>/dotfiles`;
see [location selection](environment.md#location-selection). A remote repository
may carry its own `batfiles.toml`, and nothing reads one yet.

## Reading the manifest

`sync` reads the leaf manifest, and it is the only command that does. A command
that never opens it cannot be failed by it: a malformed manifest does not stop
an enable, a disable, or a `vars` lookup, all of which work on machine-local
state instead.

- The document is read and parsed whole before anything in it is used. A command
  that cannot parse its manifest stops before it has done any work.
- A missing manifest is an error. A repository is a repository because it has
  one, so batfiles reports the path rather than proceeding as if the file were
  empty.
- A malformed document is an error, reported with the file and the position
  within it, and the file is left untouched rather than repaired or replaced.
- No field means anything yet, so any valid TOML document parses — including an
  empty one. The manifest's records arrive with the actions that read them.
