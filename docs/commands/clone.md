# `clone`

```text
batfiles clone <url> [--ref <ref>] [--skip-action <id>]... [--skip-group <group>]...
    [--enable-action <id>]... [--disable-action <id>]...
    [--enable-group <group>]... [--disable-group <group>]...
```

Clone a leaf repository into the selected batfiles directory, adopt its
bootstrap policy, and [sync](sync.md#sync). [Location selection](../environment.md#location-selection)
excludes working-directory discovery for this command.

The destination must not exist, even as an empty directory or broken symlink.
Batfiles rejects it before launching Git; missing parents are created.

| Option | Purpose |
| --- | --- |
| `--ref <ref>` | Check out a branch, tag, or commit before reading the manifest; never empty |
| `--skip-action <id>` | Skip an action during this synchronization; repeatable |
| `--skip-group <group>` | Skip a group during this synchronization; repeatable |
| `--disable-action <id>` / `--enable-action <id>` | Persist a bootstrap action choice; repeatable |
| `--disable-group <group>` / `--enable-group <group>` | Persist a bootstrap group choice; repeatable |

The [shared execution options](../cmdline.md#shared-action-execution-options) also apply.
`clone` accepts neither `--dry-run` nor `--refresh-remotes`.

`--ref` follows the [manifest ref rules](../actions/git-clone.md#ref-following-one-branch-tag-or-commit):
a published branch becomes a local tracking branch; other resolved refs are
detached. Without it, the origin's `HEAD` selects the branch. Batfiles records
no ref and never updates the leaf repository itself.

## What the bootstrap decides

Bootstrap applies the leaf's [default-disabled candidates](../repoformat.md#default-disabled-bootstrap-entries)
and explicit enable/disable inputs using [adoption precedence](../environment.md#bootstrap-adoption-precedence),
then persists the result under the [state lifecycle rules](../state.md#bootstrap-adoption).
Included manifests' bootstrap sections are ignored. Candidate conditions use
the leaf's effective variables; a condition failure warns and leaves the
candidate unapplied. Ordinary condition exclusions are reported at `-v`.

Each decision is reported with its source and the same wording as an
[enable/disable command](enable-disable.md#enable-and-disable-actions-or-groups):

```text
default-disabled: disabled action `p10k`
BATFILES_DISABLE_GROUPS: disabled group `gui`
--enable-group: enabled group `gui` (was disabled)
```

Malformed option addresses fail before cloning; malformed environment addresses
warn and are dropped. Once cloned, the repository is kept on any later failure:

- An unresolved `--ref` fails before manifest loading and leaves the default
  branch checked out. Correct it with Git, then run `sync --bootstrap`.
- A missing `batfiles.toml` fails before bootstrap, identifying the clone as a
  Git repository that is not a batfiles repository.
- A synchronization failure leaves both the clone and written bootstrap state
  in place. Fix the cause and retry with `sync`.

See [global options](../cmdline.md#global-options) and
[output conventions](../cmdline.md#output-streams).
