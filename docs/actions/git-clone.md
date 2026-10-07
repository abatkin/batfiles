# `git-clone`

Declares one Git repository cloned at a destination, and kept up to date there.

```toml
[[actions]]
type = "git-clone"
id = "oh-my-zsh"
source = "https://github.com/ohmyzsh/ohmyzsh.git"
dest = "~/.oh-my-zsh"
```

| Field    | Type   | Required | Description                                                   |
|----------|--------|:--------:|---------------------------------------------------------------|
| `source` | string |   yes    | A repository for git to clone. Not a repository path.         |
| `dest`   | string |   yes    | The clone directory, exactly. Never empty; `~` is the home.   |
| `ref`    | string |    no    | A branch, tag, or commit to follow. Never empty.              |

`source` is passed unchanged to Git: HTTPS, SSH/scp-style, `git://`, or a local
path. Only empty sources are rejected by the manifest; repository path rules
do not apply. `dest` follows the shared [destination syntax](../repoformat.md#sources-and-destinations).

Batfiles uses Git from `PATH` with the [documented environment](../environment.md#variables-passed-on-to-git).
It does not initialize or update submodules; use
`git submodule update --init --recursive` if needed.

## `ref`: following one branch, tag, or commit

Without `ref`, a clone follows its current branch and updates from its upstream.
With `ref`, every run fetches all remotes, then resolves the requested target:

1. For a valid branch name, search remote-tracking branches, preferring `origin`.
   Check out a local branch of that name, create tracking when needed, and
   fast-forward on later runs.
2. Otherwise resolve a tag, commit, full `refs/…` name, or expression such as
   `main~1`, and check it out detached. `HEAD` is resolved this way too.
3. An unresolved ref fails naming the request. A detached checkout already at
   the requested object is unchanged.

[Git updates](../safety.md#git-updates) defines worktree protection and failure
handling. [Clone validation](../safety.md#clone-validation) defines usable existing
checkouts; other destinations are [conflicts](../safety.md#conflicts-and-backups).
Existing clones retain their configured remotes.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
