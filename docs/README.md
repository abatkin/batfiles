# Batfiles Documentation

Current behavior is specified here. [Future proposals](future/) are advisory
and describe unbuilt features. [Documentation ownership](../rewrite/docs.md)
defines promotion and placement during the rewrite.

- [Product goals](goals.md): product model and intended scope, distinguished
  from implemented features.
- [Command-line surface](cmdline.md): commands, options, selection, output,
  dry-run, and exit statuses.
- [Environment](environment.md): environment parsing and location/color precedence.
- [Repository format](repoformat.md): manifest schema, action fields, static
  variables, clone lists.
- [Installation safety](safety.md): path resolution, destination handling,
  staging, permissions, archive validation, and Git updates.
- [Local state](state.md): state schemas, lifecycle, and atomic replacement.

[Rewrite guidance](../rewrite/guidance.md) owns implementation design and takes
precedence during the rewrite. It moves to `docs/architecture.md` at slice 8;
`AGENTS.md` retains workflow and links to the design guidance.
