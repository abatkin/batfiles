# Install a tool from GitHub releases

Many tools publish their builds as GitHub release assets. Batfiles can install
the latest release and keep it current: a `github-release` variable holds the
latest release's tag, and a template builds the download URL from it, so a new
release arrives on the next `sync` with no edit to your manifest.

The example installs [fzf](https://github.com/junegunn/fzf) on Linux, links it
onto your `PATH`, and regenerates its shell integration whenever it changes.
The link is a symlink action, which needs Unix. Asset names differ from project
to project; look at a release's assets to see how yours spells its version,
operating system, and architecture.

## Declare the release and its download

```toml
[vars.fzf_tag]
github-release = "junegunn/fzf"
cache = "1d"

[remotes.fzf]
type = "archive"
url = "https://github.com/junegunn/fzf/releases/download/${fzf_tag}/fzf-${fzf_tag.substring(1)}-linux_${{'x86_64': 'amd64', 'aarch64': 'arm64'}[facts.arch]}.tar.gz"

[[actions]]
type = "symlink"
source = "@fzf/fzf"
dest = "~/.local/bin/fzf"

[[actions]]
type = "run"
id = "fzf-shell"
command = "mkdir -p ~/.config/fzf && ~/.local/bin/fzf --zsh > ~/.config/fzf/init.zsh"
trigger = { after = "@fzf" }
```

`fzf_tag` is the tag of the repository's latest release exactly as GitHub names
it, such as `v0.65.2`. fzf's asset names leave out the `v`, which
`${fzf_tag.substring(1)}` removes, and say `amd64` where batfiles'
[`facts.arch`](../environment.md#host-facts) says `x86_64`, which the dictionary
translates. Batfiles never guesses an asset's name: the template spells it.

The `run` action follows the remote, so its command runs on the first `sync`
and again only when a new release is downloaded.

## Install and upgrade

```sh
batfiles sync
```

The first `sync` looks the tag up, downloads and unpacks the archive under
`remotes/fzf`, links `~/.local/bin/fzf`, and runs `fzf-shell`. The tag is cached
for a day (`cache = "1d"`), so later runs make no request until it is stale.
When a new release is published, the next `sync` after the cached tag expires
resolves a new URL, downloads it, and runs `fzf-shell` again. To check now:

```sh
batfiles sync --refresh-vars
batfiles vars list
```

`vars list` shows the tag and where it came from, such as
`fzf_tag = "v0.65.2" (batfiles.toml, github-release)`.

A lookup is a network request, made whenever the cached tag is stale by any
command that resolves variables, including a dry run. When it fails, as it does offline, the
last cached tag is used, with a warning; with nothing cached, the variable is
empty and the download fails. See
[how a lookup is made](../environment.md#how-github-release-lookups-are-made).

## Why a remote upgrades and a download action does not

A [file or archive remote](../repoformat.md#materialization) refetches whenever
its resolved URL changes, so it follows the tag. A
[`fetch-archive`](../actions/fetch-archive.md) or
[`fetch-file`](../actions/fetch-file.md) action is a *seed*: once installed it
is kept, and a new URL does not replace it. With a seed, the variable means "the
latest release when this machine was set up", until
[`--refresh-content`](../safety.md#refreshing-seeds) reinstalls every seed. Use
a remote for a tool you want kept current.

## Pin a version on one machine

A machine value overrides the lookup, so one machine can stay on a known
version while others follow the latest:

```sh
batfiles vars set fzf_tag v0.60.0
```

Use `--var fzf_tag=v0.60.0` for one run instead, or `vars unset fzf_tag` to
follow the latest again. See
[variable precedence](../environment.md#variable-precedence).

## When the asset name has no version

GitHub serves the latest release's assets under a fixed URL, so an asset whose
name holds no version needs no variable at all:

```toml
[remotes.starship]
type = "archive"
url = "https://github.com/starship/starship/releases/latest/download/starship-x86_64-unknown-linux-musl.tar.gz"
```

Such a URL never changes, though, so the remote keeps its first download until
`sync --refresh-remotes`.

## Caveats

- A fixed `sha256` matches one release, so the download fails on the next
  release until you update it; leave it out to follow releases unattended. See
  [what is not sandboxed](../safety.md#what-is-not-sandboxed).
- The lookup sends no credentials, so it sees only public repositories.
  `github-url` names another GitHub host, such as GitHub Enterprise Server; see
  [the field](../repoformat.md#the-github-url-field).
- "Latest" skips drafts and prereleases.

The [field reference](../repoformat.md#github-release-declarations) lists every
option.
