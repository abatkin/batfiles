# Batfiles documentation

Keep ordinary files in Git, declare where they belong in `batfiles.toml`, and
run `batfiles sync`.

## Start here

1. [Install batfiles](guides/install.md), or bootstrap an existing repository.
2. [Create your first repository](getting-started.md) and preview an installation.
3. Learn [everyday commands](guides/everyday.md) and [machine-specific choices](guides/machines.md).

These docs follow `main`. A feature not yet in a stable release is labeled
Unreleased, with the build it needs. Check your installed version with
`batfiles version`.

## Find a task

| Task | Reference |
| --- | --- |
| Run or preview a command | [Commands and options](cmdline.md), [output](cmdline.md#output-streams), [exit statuses](cmdline.md#exit-statuses) |
| Write a manifest | [Repository format](repoformat.md), [action types](repoformat.md#actions) |
| Choose what runs on a machine | [Selection](cmdline.md#selecting-what-a-run-does), [conditions](repoformat.md#conditions) |
| Compose repositories | [Guide](guides/composition.md), [Remotes](repoformat.md#remotes), [include-remote](actions/include-remote.md#include-remote) |
| Configure locations and variables | [Environment](environment.md), [variable precedence](environment.md#variable-precedence) |
| Choose actions and refresh content | [Guide](guides/content.md), [Installation safety](safety.md) |
| Diagnose unexpected behavior | [Troubleshooting](guides/troubleshooting.md) |
| Look up a term | [Glossary](glossary.md) |
| Inspect machine state | [State files and cache](state.md) |
| Install a binary or bootstrap a checkout | [Hosted installer](installer.md#hosted-installer), [checkout stub](installer.md#leaf-stub) |

## Contributing and releasing

- [AGENTS.md](https://github.com/abatkin/batfiles/blob/main/AGENTS.md): branch workflow, checks, and documentation ownership.
- [Product principles](https://github.com/abatkin/batfiles/blob/main/README.md#product-principles): the product model.
- [Architecture](https://github.com/abatkin/batfiles/blob/main/docs/contributing/architecture.md): implementation rules, modules, and tests.
- [Writing documentation](https://github.com/abatkin/batfiles/blob/main/docs/contributing/authoring.md): guides, references, coverage, and examples.
- [Release management](https://github.com/abatkin/batfiles/blob/main/dist/README.md): repository setup and release procedure.
- [Distribution](https://github.com/abatkin/batfiles/blob/main/docs/contributing/distribution.md): release assets, build tasks, hosting, and workflows.
- [Potential enhancements](https://github.com/abatkin/batfiles/blob/main/docs/contributing/enhancements.md): unscheduled ideas, not supported behavior.
