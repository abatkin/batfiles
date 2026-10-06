# Batfiles Documentation

These references specify current behavior and implementation design.
[Documentation ownership](../AGENTS.md#documentation) defines where each rule
belongs. Start with the [project README](../README.md) for installation and an
example repository.

- [Product principles](../README.md#product-principles): the product model and
  the principles that guide changes.
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

[Potential enhancements](enhancements.md) tracks unscheduled ideas for future
work; it does not specify supported behavior. [AGENTS.md](../AGENTS.md) owns the
branch workflow and canonical commands. [Release management](../dist/README.md)
covers repository setup and the procedure for cutting a release.
