# Troubleshooting

Start with verbose output from the command you were running. For a preview:

```sh
batfiles sync --dry-run -v
```

This reports selected roots, actions, and exclusions. Dynamic-variable commands
can still run during dry-run; use `batfiles vars list --no-refresh` to inspect
cached values without running them.

## The command printed nothing

An ordinary `sync` is quiet when it changes nothing. Add `-v` to distinguish
correct destinations from skipped actions. Copies and downloads keep existing
destinations; use [refresh-content](content.md#refresh-deliberately) when you
want to replace an editable seed from its source.

## An action did not run

Check the reason printed at `-v`: a condition, persistent disable, run-only skip,
or inclusion filter may exclude it. Use `vars list --no-refresh` to inspect leaf
values and `sync --dry-run -vv` to inspect values used by opened inclusions.
A condition that cannot be evaluated prints a warning and skips its action,
whether it is written as `when` or `unless`.

Enabling a group does not override its members' conditions. Conversely,
`apply-action` deliberately bypasses the named action's own condition and
persistent disable. See [selection](../cmdline.md#selection-by-command).

## Batfiles selected the wrong repository

Run from the directory containing `batfiles.toml`, or provide
`--batfiles-dir /path/to/dotfiles`. Discovery does not walk up from subdirectories.
`BATFILES_DIR`, if set, takes priority over discovery. `-v` shows the roots used.
See [locations](../environment.md#location-selection).

## There is something at the destination

The default conflict policy backs up unmanaged content beside its destination.
Use `--no-overwrite` to skip conflicts or `--interactive` to decide individually.
Copy and download seeds normally keep an occupied destination without replacing
it. See [destination safety](../safety.md#replacing-what-is-already-there).

Backups have names such as `settings.toml.batfiles-backup-20261006T142233Z`.
To restore one, inspect both versions, move the current destination aside, and
move the chosen backup back to its original name. Disable or change the action
first if the next synchronization would otherwise replace it again. Batfiles
never automatically deletes backups.

## The preview says its plan is partial

An included remote has not been materialized, so its actions cannot be read.
Review that repository, then use `sync` to fetch it and apply the configuration.
Repeat your preview to see the full set. A partial plan can exit successfully;
it does not mean every potential action was inspected.
See [plan completeness](../cmdline.md#plan-completeness).

## A Git checkout did not update

Read the warning, then inspect that checkout with `git status` and Git's branch
and remote tools. Batfiles preserves local modifications and diverged branches
rather than resetting them. Resolve the Git situation and retry.
A normal `sync` also does not pull the leaf repository itself.
See [Git update policy](../safety.md#git-updates).

## A dynamic value is stale

```sh
batfiles vars list --no-refresh
batfiles vars refresh
```

The first shows cached values and freshness; the second runs the declarations
in play again. Failed refreshes can retain an older value, and explicit refresh
reports failure. For a remote variable, the refresh name is `remote-id.name`,
not an inclusion's address. See [refreshing variables](../commands/vars.md#vars-refresh).

## Another run holds the lock

Wait for the other run using the same cache directory to finish. The presence
of `run.lock` alone does not mean a process holds it; the operating-system lock
is released when the process exits. Do not delete or replace the file while a
run is active. See [run lock](../state.md#run-lock).

## A run failed partway through

Earlier completed work remains. Fix the reported cause and retry; the whole run
is not rolled back. Exit status 0 can still include warnings, including failed
clone-list entries. Read [failure behavior](../cmdline.md#execution-failures)
and [exit statuses](../cmdline.md#exit-statuses) when scripting batfiles.

## Windows says symlinks are unsupported

Symlink actions require Unix. Use copies for Windows where appropriate, or
condition Unix-only actions on host facts. See [machine configuration](machines.md).
