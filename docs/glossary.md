# Glossary

Terms used throughout the guides and references, each with a link to the
reference that defines it.

| Term | Meaning |
| --- | --- |
| Action | One `[[actions]]` entry in a manifest: something to install, chosen by its `type`. See [actions](repoformat.md#actions). |
| Address | A dotted name that selects an action or group, such as `editor` or `team.settings` for an action an inclusion contributes. See [addresses](cmdline.md#addresses). |
| Backup | Content that was in a destination's way, set aside beside it as `<dest>.batfiles-backup-<time>`. Batfiles never deletes backups. See [conflicts and backups](safety.md#conflicts-and-backups). |
| Bootstrap | Adopting a repository's suggested disabled actions and groups on a machine's first run, done by `clone` and `sync --bootstrap`. See [what the bootstrap decides](commands/clone.md#what-the-bootstrap-decides). |
| Checkout stub | The `install.sh` and `install.ps1` that `init` writes into a repository, which get batfiles if needed and bootstrap that checkout. See [leaf stub](installer.md#leaf-stub). |
| Condition | A `when` or `unless` expression over variables and host facts that decides whether an action or remote takes part in a run. See [conditions](repoformat.md#conditions). |
| Destination home | The directory `~` names in a `dest`: your home directory unless `--home-dir` or `BATFILES_HOME` chooses another. See [location selection](environment.md#location-selection). |
| Dry run | A preview, with `--dry-run`, that reports intended work without changing installed content. See [dry-run behavior](cmdline.md#dry-run-behavior). |
| Dynamic variable | A variable whose value comes from running a command, cached between runs. See [dynamic variables](repoformat.md#dynamic-variables). |
| Group | A name shared by actions so they can be applied, skipped, or disabled together. See [groups](repoformat.md#groups). |
| Inclusion | An `include-remote` action, which runs actions from a remote's own manifest. Its `id` prefixes their addresses. See [`include-remote`](actions/include-remote.md#include-remote). |
| Leaf | Your own dotfiles repository: the one whose `batfiles.toml` a command reads. See [repository layout](repoformat.md#repository-layout). |
| Machine state | Choices and caches stored on one machine, outside any repository: `vars.toml`, `disabled.toml`, and the dynamic-variable cache. See [state files](state.md). |
| Manifest | A repository's `batfiles.toml`. See [repository format](repoformat.md). |
| Materialize | Fetch or update a remote into `remotes/<id>` in the leaf. Only `sync` does this. See [materialization](repoformat.md#materialization). |
| Partial plan | A dry run that could not read some included actions because their remote has not been materialized yet. See [plan completeness](cmdline.md#plan-completeness). |
| Remote | A Git repository, file, or archive declared under `[remotes]`, used as a source of files or for an inclusion. See [remotes](repoformat.md#remotes). |
| Seed | Content installed once and then allowed to diverge: copies and downloads. Later runs keep it unless you refresh it. See [seeds](safety.md#seeds-do-not-replace-and-so-do-not-refuse). |
| Variable | A named string value from the manifest, machine state, environment, or `--var`, used in conditions. See [variables](repoformat.md#variables). |
