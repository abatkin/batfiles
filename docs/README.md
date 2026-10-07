# Batfiles documentation

Start with the [project README](../README.md) to install batfiles and try a
small manifest. The references below are for looking up a specific task or rule.

## Using batfiles

| Task | Reference |
| --- | --- |
| Run, preview, or troubleshoot a command | [Commands and options](cmdline.md), [output](cmdline.md#output-streams), [exit statuses](cmdline.md#exit-statuses) |
| Write a manifest | [Repository format](repoformat.md), [action types](repoformat.md#actions) |
| Choose what runs on a machine | [Selection](cmdline.md#selecting-what-a-run-does), [conditions](repoformat.md#conditions) |
| Compose repositories | [Remotes](repoformat.md#remotes), [include-remote](repoformat.md#include-remote) |
| Configure locations and variables | [Environment](environment.md), [variable precedence](environment.md#variable-precedence) |
| Understand backups, refresh, and Git updates | [Installation safety](safety.md) |
| Inspect or reset machine state | [State files and cache](state.md) |
| Install a binary or bootstrap a checkout | [Hosted installer](distribution.md#hosted-installer), [checkout stub](distribution.md#leaf-stub) |

## Contributing and releasing

- [AGENTS.md](../AGENTS.md): branch workflow, checks, and documentation ownership.
- [Product principles](../README.md#product-principles): the product model.
- [Architecture](architecture.md): implementation rules, modules, and tests.
- [Release management](../dist/README.md): repository setup and release procedure.
- [Distribution](distribution.md): release assets, build tasks, hosting, and workflows.
- [Potential enhancements](enhancements.md): unscheduled ideas, not supported behavior.
