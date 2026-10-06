# Batfiles Documentation

Current behavior is specified here. [Future proposals](future/) are advisory
and describe unbuilt features. [Documentation
ownership](../AGENTS.md#documentation) defines promotion and placement.

- [Product goals](goals.md): product model and intended scope, distinguished
  from implemented features.
- [Command-line surface](cmdline.md): commands, options, selection, output,
  dry-run, and exit statuses.
- [Environment](environment.md): environment parsing, location/color
  precedence, and how dynamic variables' commands run.
- [Repository format](repoformat.md): manifest schema, action fields, static
  and dynamic variables, clone lists.
- [Installation safety](safety.md): path resolution, destination handling,
  conflicts, backups, refresh, staging, permissions, archive validation, and Git
  updates.
- [Local state](state.md): state schemas, lifecycle, the run lock, and atomic replacement.
- [Distribution](distribution.md): the release tree, its targets and assets,
  the tasks and workflow that build and publish a release, and the Pages site
  that serves its installers.
- [Architecture](architecture.md): the rules the implementation is written to,
  source organization, and test environments.

Remaining work is in [the roadmap](future/roadmap.md); `AGENTS.md` owns the
branch workflow and the canonical commands.
