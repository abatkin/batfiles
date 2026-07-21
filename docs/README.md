# Batfiles Documentation

Each document owns a distinct part of the batfiles specification:

- [Product goals](goals.md) defines the product model, guiding principles, and
  intended scope.
- [Repository format](repoformat.md) defines `batfiles.toml`, manifest schemas,
  shared names, IDs, and value types.
- [Command-line surface](cmdline.md) defines commands, options, dry-run
  behavior, and address forms.
- [Environment variables](environment.md) defines environment inputs, location
  selection, runtime variable precedence, and bootstrap precedence.
- [Local state and cache files](state.md) defines the schemas and lifecycle of
  machine-local configuration and cache files.
- [Safety model](safety.md) defines trust boundaries, destination resolution,
  replacement and backup policy, conservative Git updates, archive handling,
  and failure recovery principles.

Rules should be specified in their owning document and linked from the others.
This keeps safety-sensitive behavior such as precedence, dry-run execution, and
cache mutation from drifting between separate descriptions.
