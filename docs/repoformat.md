# Batfiles repository format

Manifest syntax and action fields. Start with the [README example](https://github.com/abatkin/batfiles/blob/main/README.md#example).

[Layout](#repository-layout) · [Schema](#top-level-schema) · [Names](#names-and-ids) ·
[Remotes](#remotes) · [Paths](#sources-and-destinations) · [Filters](#entry-filters) ·
[Actions](#actions) · [Clone lists](#the-clone-list-format) ·
[Variables](#variables) · [Conditions](#conditions) · [Templates](#templates) ·
[Bootstrap defaults](#default-disabled-bootstrap-entries)

## Repository layout

A repository is an ordinary file tree with `batfiles.toml` at its root.
Other files matter only when an action references them. Batfiles owns generated
`remotes/`; add it to `.gitignore`.

The **leaf repository** is selected by [location precedence](environment.md#location-selection).
A remote's manifest is optional until an [`include-remote`](actions/include-remote.md#include-remote)
asks to read it.

## Reading the manifest

Every document is read and validated in full before its contents are used,
including before its dynamic commands run. An included manifest is read only
when its inclusion opens, after the leaf's variables have resolved.

- Missing or unreadable manifests fail the command. Missing [state files](state.md)
  are instead empty documents.
- Malformed TOML or invalid schema fails with the file and position; the file
  is left untouched.
- Unknown record fields, invalid value shapes, and cross-record violations
  such as duplicate IDs are load errors. Action diagnostics use one-based
  positions in the manifest.
- An empty manifest is valid. Commands that do not read the repository are
  unaffected by its manifest.

## Top-level schema

All four sections are optional. There is no format-version field.

```toml
[remotes]                  # map<ID, Remote>

[[actions]]                # ordered list<Action>

[vars]                     # map<variable name, string | dynamic variable>

[default-disabled]         # leaf bootstrap policy
[[default-disabled.actions]]
[[default-disabled.groups]]
```

Records are closed: only documented fields and variants are accepted. Keys in
`[remotes]` and `[vars]` are user-chosen names, validated by the rules below.

## Names and IDs

| Kind | Syntax | Constraints |
| --- | --- | --- |
| Action ID, group, remote ID | `[A-Za-z0-9][A-Za-z0-9_-]*` | No whitespace, dots, or commas |
| Variable name | `[A-Za-z_][A-Za-z0-9_]*` | Cannot be `facts`, `env`, `vars`, `true`, or `false` |

Action IDs must be unique within a repository; a duplicate error names both
records. Remote IDs must also be distinct ignoring case on every platform,
because they name materialization paths. Group, action, and remote names occupy
separate namespaces.

Variable names are case-sensitive. `_hidden` is a variable name but not an ID;
`9front` and `oh-my-zsh` are IDs but not variable names. Dots join IDs into
[addresses](cmdline.md#addresses).

## Remotes

`[remotes]` is a map from a name to a record describing content this repository
does not hold: another repository, a file, or an archive. The key is the remote's
[ID](#names-and-ids):

```toml
[remotes.core]
type = "git"
url = "git@github.com:me/dotfiles-core.git"
ref = "main"
```

Every remote is a closed record selected by its required `type`:

| Field    | Type               | Required | Description                                                     |
|----------|--------------------|:--------:|-----------------------------------------------------------------|
| `type`   | remote-type string |   yes    | Selects the record variant: [`git`](#git), [`file`](#file), or [`archive`](#archive). |
| `when`   | condition          |    no    | See [conditions](#conditions).                                  |
| `unless` | condition          |    no    | At most one of the two.                                         |

A `type` the schema does not name is refused as the manifest is read, because
there is no record behind it to check, and so is a field the selected variant
does not have.

### `git`

A repository for git to clone.

| Field                 | Type    | Required | Description                                                                 |
|-----------------------|---------|:--------:|-----------------------------------------------------------------------------|
| `url`                 | [template string](#templates) |   yes    | A repository for git to clone. Never empty.                                 |
| `ref`                 | [template string](#templates) |    no    | A branch, tag, or commit to follow. Never empty.                            |
| `allow-dynamic-vars`  | boolean |    no    | Whether an included manifest's [dynamic-variable](#dynamic-variables) commands run. Defaults to `false`. |
| `allow-env-templates` | boolean |    no    | Whether an included manifest's templates may read `env`; see [templates and the host environment](actions/include-remote.md#templates-and-the-host-environment). Defaults to `false`. |
| `allow-commands`      | boolean |    no    | Whether an included manifest's [`run`](actions/run.md#in-an-included-manifest) actions run their commands. Defaults to `false`. |

`url` follows [`git-clone` source syntax](actions/git-clone.md#git-clone), and `ref` follows its
[ref rules](actions/git-clone.md#ref-following-one-branch-tag-or-commit). `branch` is not a field.

`allow-dynamic-vars` permits dynamic-variable commands in an included manifest.
Without it, command declarations there declare nothing, and `-v` names those
skipped. Its [`github-release`](#github-release-declarations) declarations still
resolve: a lookup runs no code, so a remote with no permissions can supply a
dynamic value, as it can already supply any static value. Leaf declarations need
no permission. See
[dynamic-command execution](environment.md#how-dynamic-commands-are-run).

Each permission applies only to a checkout of the repository `url` names; see
[permissions follow the origin](safety.md#permissions-follow-the-origin).

A remote supplies content; an action decides where to install it. Only the
leaf's `[remotes]` is materialized. An included manifest's map is
[ignored](actions/include-remote.md#an-included-manifests-own-remotes).

### `file`

One file, fetched from a URL.

```toml
[remotes.pathogen]
type = "file"
url = "https://raw.githubusercontent.com/tpope/vim-pathogen/master/autoload/pathogen.vim"
```

| Field    | Type   | Required | Description                                                     |
|----------|--------|:--------:|-----------------------------------------------------------------|
| `url`    | [template string](#templates) |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL.       |
| `sha256` | [template string](#templates) |    no    | 64 hexadecimal digits: the digest the fetched bytes must have.  |
| `executable` | boolean | no  | Whether the file lands executable. Defaults to `false`.         |
| `decompress` | boolean | no  | Whether the download is a gzip or bzip2 stream holding the file. Defaults to `false`. |

The materialization is the file itself, at `remotes/<id>`, so a
[repository path](#sources-and-destinations) names it whole, as `@pathogen` or
`{ remote = "pathogen" }`:

```toml
[[actions]]
type = "symlink"
source = "@pathogen"
dest = "~/.vim/autoload/pathogen.vim"
```

A path under a file remote, or a `source-dir` naming one, is a load error. It is
fetched by the [shared transfer](#the-transfer-both-fetching-actions-share), with
[fetched-file permissions](safety.md#installed-permissions); `executable` and
`decompress` mean what they do for [`fetch-file`](actions/fetch-file.md#fetch-file).

### `archive`

A zip or tarball fetched from a URL and unpacked.

```toml
[remotes.fzf]
type = "archive"
url = "https://example.com/fzf-0.65.2.tar.gz"
archive-root = "*"
```

| Field          | Type   | Required | Description                                                            |
|----------------|--------|:--------:|------------------------------------------------------------------------|
| `url`          | [template string](#templates) |   yes    | An `http://`, `https://`, or [`file://`](#file-urls) URL.              |
| `sha256`       | [template string](#templates) |    no    | 64 hexadecimal digits: the digest the fetched archive must have.       |
| `archive-root` | string |    no    | A prefix every entry is written without, or `*` for the archive's single top-level directory. |
| `include`      | glob or list | no | Entries to unpack, by their path once the root is stripped; absent, every entry. See [entry filters](#entry-filters). |
| `exclude`      | glob or list | no | Entries not to unpack.                                                 |
| `executable`   | glob or list | no | Files to make executable, by their path once the root is stripped. See [`fetch-archive`](actions/fetch-archive.md#fetch-archive). |

The archive is unpacked into `remotes/<id>/` as [`fetch-archive`](actions/fetch-archive.md#fetch-archive)
unpacks into its `dest`, with the same formats, `archive-root`, filters,
`executable`, and [archive safety](safety.md#archive-extraction). Actions read
paths within it as within a Git clone, such as `@fzf/bin/fzf`.

File and archive remotes have no manifest, so [`include-remote`](actions/include-remote.md#include-remote)
cannot name them.

### Materialization

`sync` materializes (fetches or updates) every remote whose
[condition](#a-remotes-condition) holds at `remotes/<id>` before any action,
even if no action uses it. Two IDs naming one URL have separate materializations.

Git remotes clone or update under the [Git policy](safety.md#git-updates),
following `ref` when supplied. An unusable checkout at a tool-owned path is
refused under every conflict policy; see [clone validation](safety.md#clone-validation).

File and archive remotes use a sibling stamp, `remotes/<id>.batfiles-source`,
to record their fetching declaration and detect changes:

| At `remotes/<id>` | Stamp | `sync` |
| --- | --- | --- |
| Nothing | Any, or none | Fetch |
| Node of the stamp's recorded kind | Matches declaration | Keep; report unchanged at `-v` |
| Node of the stamp's recorded kind | Differs from declaration | Refetch and replace |
| Anything else | Missing, unreadable, invalid, or wrong kind | Refuse before fetching |

The stamp covers the type, URL, digest, and type-specific options. It records
[resolved](#templates) values, so a changed URL refetches. Digests compare
case-insensitively; a single pattern and a one-element list compare
equally, but pattern order matters. [Replacement](safety.md#replacing-a-materialization)
is staged and verified; a failed fetch leaves the old materialization and stamp
in place and fails the run. [`sync --refresh-remotes`](commands/sync.md#sync) forces
refetching when a declaration is unchanged.

An unclaimed node, including a clone left under an ID now declared as a file
or archive, must be moved aside or removed before retrying. A Git
materialization removes any obsolete stamp beside it.

Materialization failure, including a failed [template](#templates), stops the
run before any action. Apply commands and
[dry runs](cmdline.md#dry-run-behavior) fetch nothing and use existing trees,
however stale. An action whose source remote is absent fails naming the remote;
a missing inclusion is handled by [plan completeness](cmdline.md#plan-completeness).

### A remote's condition

```toml
[remotes.corporate]
type = "git"
url = "git@git.example.com:it/dotfiles.git"
when = "work"
```

A remote's [condition](#conditions) decides whether it may be materialized or
read. Every execution command evaluates it, including apply commands. An
excluded remote is neither updated nor read, even if a previous materialization
remains; that tree is not deleted.

An action sourcing an excluded remote fails naming the remote and reason.
Usually it should carry the same condition. An inclusion of the excluded remote
instead contributes nothing. See [exclusion reporting](cmdline.md#exclusion-reporting).

## Sources and destinations

A **repository path** identifies content within the declaring repository or a
declared remote. `symlink`, `copy`, and `git-clone-list` use one as `source`;
`symlink-dir` and `copy-dir` use one as `source-dir`.
Fetching actions and `git-clone` have their own source syntax.

```toml
source = "files/zshrc"                             # declaring repository
source = "@core/files/zshrc"                       # declared remote
source = { remote = "core", path = "files/zshrc" } # same remote path
```

The structured form is closed and requires `remote`; omitting `path` means
`@<remote>`. Both remote spellings resolve and appear in diagnostics identically.

- A remote reference must name a remote in the same manifest. Undeclared
  references fail at load time; absent materializations fail at execution.
- A file remote is named whole (`@pathogen` or `{ remote = "pathogen" }`).
  A path within it or a `source-dir` naming it is invalid.
- Tree paths must be nonempty, relative, and remain strictly inside the tree.
  `.` and `..` are allowed only within that constraint. `.`, `./`, `shell/..`,
  and a tree remote with no path are invalid.
- Use `/` separators. Anchored paths and drive prefixes are rejected according
  to the host's path syntax. Symlinks within a repository may point outside it.
- An initial `@` is reserved with no escape, including inside a structured
  `path`. Elsewhere it is ordinary text, as in `files/@work/zshrc`.

Diagnostics identify the source tree. Sources are checked when an action runs,
including dry runs. A final broken symlink counts as present for linking; copies
must read its target. `source-dir` must resolve to a directory. Clone-list
sources are read during [preparation](cmdline.md#clone-list-preparation).

`dest` and `dest-dir` are [template strings](#templates) whose resolved values
use the selected home as their relative base:

| Form | Resolution |
| --- | --- |
| `~` | Selected home |
| `~/path` or `path` | Relative to selected home |
| Absolute path | Used as written |
| Empty value or `~other` | Invalid |

Destinations may leave the selected home. Roots are anchored and paths normalized
lexically. [Location selection](environment.md#location-selection) defines roots;
[installation safety](safety.md) defines filesystem resolution, source
containment, conflicts, and staging.

## Entry filters

`include` and `exclude` select tree entries for `symlink-dir`, `copy`,
`copy-dir`, `fetch-archive`, and archive remotes. Each accepts one glob or a list;
archive `executable` patterns use the same matching rules.

```toml
include = "*rc"
exclude = ["private", "**/*.bak"]
```

Paths are relative to the root specified by the action, with `/` separators on
every platform. Patterns match the whole path, case-sensitively.

| Syntax | Matches |
| --- | --- |
| `*` | Any run of bytes within one segment, including a leading `.` |
| `?` | One byte within a segment |
| `[abc]`, `[a-z]`, `[!abc]` | One byte in, or outside, the class |
| `{a,b}` | Either alternative within one segment |
| `**` | Zero or more whole segments; must be a segment of its own |

Only `**` crosses separators: `*.bak` matches at the root, `**/*.bak` at any
depth. Classes and alternatives cannot contain `/`. Backslash is literal;
use `[*]` for a literal `*`. Byte matching means `?.txt` and `[é].txt` both
miss `é.txt`; use the literal character or `*`.

An entry is selected when it or an ancestor directory matches `include`, or
`include` is absent, and neither it nor an ancestor matches `exclude`.
Exclusion wins. `include = []` selects nothing. Unselected, nonexcluded
directories are created only when needed to hold selected descendants.
For example, including `bin` and excluding `bin/secret` installs the rest of
`bin`; including `scripts/lib` creates `scripts` holding only `lib`.

Empty patterns, leading `/`, empty/`.`/`..` segments (including a trailing `/`),
and malformed globs are load errors. Patterns matching nothing are not errors;
`-v` names the pattern and tree.

## Actions

[symlink](actions/symlink.md#symlink) · [symlink-dir](actions/symlink-dir.md#symlink-dir) · [create-dir](actions/create-dir.md#create-dir) ·
[copy](actions/copy.md#copy) · [copy-dir](actions/copy-dir.md#copy-dir) · [fetch-file](actions/fetch-file.md#fetch-file) ·
[fetch-archive](actions/fetch-archive.md#fetch-archive) · [git-clone](actions/git-clone.md#git-clone) ·
[git-clone-list](actions/git-clone-list.md#git-clone-list) · [include-remote](actions/include-remote.md#include-remote) ·
[run](actions/run.md#run)

`[[actions]]` is an ordered list. Each entry is a closed record selected by its
required `type` field.

| Field   | Type               | Required | Description                                                            |
|---------|--------------------|:--------:|------------------------------------------------------------------------|
| `type`  | action-type string |   yes    | Selects the action variant. `symlink`, `symlink-dir`, `create-dir`, `copy`, `copy-dir`, `fetch-file`, `fetch-archive`, `git-clone`, `git-clone-list`, `include-remote`, and `run` are the ones that exist. |
| `id`    | `ID`               |    no    | Makes the action addressable.                                          |
| `group` | `ID`               |    no    | Places the action in one group. See [groups](#groups).                 |
| `when`  | condition          |    no    | Runs the action only where the condition is true. See [conditions](#conditions). |
| `unless`| condition          |    no    | Runs it only where the condition is false. At most one of the two.     |

Each variant accepts only its documented fields. A field belonging to another
variant is an error when the manifest is read: for example, `symlink` takes
`source`, while `symlink-dir` takes `source-dir`. Only `symlink-dir` and
`copy-dir` accept `dot-prefix`, which adds a leading `.` to each installed name.

`symlink` and `copy` install one file or directory at one destination.
`symlink-dir` and `copy-dir` install each direct child into a destination
directory. Choose the type explicitly; it is not inferred from the source's
filesystem type. To download and unpack an archive, use
[`fetch-archive`](actions/fetch-archive.md#fetch-archive); [`fetch-file`](actions/fetch-file.md#fetch-file) installs the
downloaded bytes as one file.

[`include-remote`](actions/include-remote.md#include-remote) and [`run`](actions/run.md#run) install
nothing of their own. An inclusion names a [remote](#remotes) and takes what that
repository's manifest declares, at its position in this list; a `run` action
runs a command, as often as its trigger says.

### Groups

A group consists of the actions whose `group` field names it. Each action has at
most one group; there is no `[groups]` declaration. Group values use the
[ID syntax](#names-and-ids), and may share names with actions.
Membership does not change declaration order or require adjacency.

Use groups to apply, disable, or skip members together. See
[selection](cmdline.md#selecting-what-a-run-does).

### The transfer both fetching actions share

Both fetching actions use these rules:

- HTTP, HTTPS, and [`file://`](#file-urls) URLs; up to five redirects.
- Status `200 OK` is required. Partial, empty-status, and conditional responses
  such as 206, 204, and 304 fail.
- An optional `sha256` is checked against the downloaded bytes. A mismatch
  reports both digests and installs nothing. Archives are verified before
  extraction, and compressed files before decompression.
- No `Accept-Encoding` request header. Proxy environment variables are honored.
- TLS certificates use the operating system's trust store, including corporate
  CAs trusted by that machine.
- Thirty seconds to connect, thirty seconds to receive response headers, and
  ten minutes total for the body.

An incomplete or failed transfer is not published. See
[staging and publication](safety.md#staging-and-publication).

#### File URLs

A `file://` URL reads a whole local file and verifies `sha256` if supplied.
HTTP status, redirects, proxies, and timeouts do not apply.

Use an absolute path: `file:///home/you/tool.tar.gz`,
`file://localhost/home/you/tool.tar.gz`, or `file:///C:/tools/a.zip` on Windows.
Paths are percent-decoded; encode spaces as `%20`, `?` as `%3F`, and `#` as
`%23`. Queries, fragments, and hosts other than `localhost` are load errors.
Missing or unreadable files fail when the action runs, naming the path.

#### An included manifest's own `[remotes]`

See [An included manifest's own `[remotes]`](actions/include-remote.md#an-included-manifests-own-remotes).

## The clone list format

A `git-clone-list` source is a text file with one repository per line:

```text
# vim plugins
https://github.com/tpope/vim-fugitive.git
https://github.com/romkatv/powerlevel10k.git dest-name=p10k id=p10k
https://github.com/company/internal-zsh-tools.git when="work" # work only
```

```text
<repository> [<key>=<value> ...] [# <comment>]
```

The repository is the first whitespace-delimited field, passed unchanged to
Git. Blank lines are ignored. The first `#` outside quoted values begins a
comment; encode a repository URL's literal `#` as `%23`.

| Key | Type | Description |
| --- | --- | --- |
| `id` | ID | Entry address suffix and diagnostic name |
| `ref` | string | Branch, tag, or commit; follows [ref rules](actions/git-clone.md#ref-following-one-branch-tag-or-commit) |
| `dest-name` | string | Explicit clone directory name |
| `when` / `unless` | condition | Entry gate; at most one |

Unknown keys, repeated keys, empty `key=` values, and fields lacking `=` fail.
Values may be quoted with `'` or `"` to contain spaces or `#`. Inside quotes,
only `\\`, `\"`, and `\'` are escapes; other escapes and unclosed quotes fail.

Conditions use the enclosing action's scope and the shared
[condition rules](#conditions). See [preparation](cmdline.md#clone-list-preparation)
for evaluation timing. An explicit `id` permits [individual selection](cmdline.md#addresses)
and appears in diagnostics as `id=p10k, plugins.txt line 3`; otherwise entries
are named by file and line. IDs are never derived from directory names:
`ack.vim` is a valid directory name but not an ID.

### What a clone is called

Without `dest-name`, use everything after the repository's last `/` or `:`,
removing a trailing `.git`. This derivation is textual, independent of URL kind.

Derived or explicit names must be one directory component: nonempty, not `.`,
`..`, or `.git`, and containing no `/`, `\`, or `:`. An invalid derived name
asks for an explicit `dest-name`.

Destination names must be unique ignoring case on every platform; entry IDs
must also be unique. Duplicate errors name both lines. One repository under two
distinct destination names is valid, and destination spelling is preserved.
The first error stops list loading, identifying the file, line, and fault.

## Variables

`[vars]` maps [variable names](#names-and-ids) to strings or dynamic declarations:

```toml
[vars]
work = "false"
profile = "personal"
rank = "3"
email = { command = ["git", "config", "user.email"], cache = "1d" }
fzf_tag = { github-release = "junegunn/fzf" }
```

Variables feed `when`, `unless`, and [templates](#templates). A `[vars]` value
is not a template, so one variable cannot refer to another; the one exception,
a `github-release` declaration's [`github-url`](#the-github-url-field), reads
only `env` and `facts`. Static values must be strings, including empty
strings: `work = true` and `rank = 3` are load errors. A table must match the
dynamic-variable schema.

See [variable precedence](environment.md#variable-precedence) for machine
values, environment and CLI overrides, and inclusion scopes. Dynamic
declarations are captured even when no condition uses them; see
[evaluation](state.md#when-declarations-are-evaluated).

### Dynamic variables

A table under `[vars]` declares a variable whose value is captured rather than
written: by running a [command](#command-declarations), or by looking up a
[GitHub release](#github-release-declarations). A table is exactly one kind:
`github-release` together with `command`, `capture`, or `command-timeout` is a
load error, as is `github-url` or `timeout` beside `command`, and the error
names the conflicting field. A table with neither `command` nor
`github-release` is a load error too. Both kinds share `cache`:

| Field   | Type     | Required | Default | Description                                                          |
|---------|----------|:--------:|---------|----------------------------------------------------------------------|
| `cache` | duration |    no    | `"1d"`  | How long a captured value is reused before it is captured again.     |

[Cache rules](state.md#dynamic-varstoml-dynamic-variable-cache) define reuse,
refresh, and failure fallback.

A declaration with no captured or cached value still declares the variable;
it reads as the empty string and overrides lower layers. Consequently `when`
on it closes but `unless` opens. Use `when` when installation requires a
successful answer.

#### Command declarations

A `command` declaration's value is what a command produces. Both TOML
spellings are the same record:

```toml
[vars]
email = { command = ["git", "config", "user.email"], cache = "24h" }

[vars.has_op]
command = "command -v op >/dev/null"
capture = "status"
cache = "1h"
command-timeout = "5s"
```

| Field             | Type                               | Required | Default    | Description                                                                     |
|-------------------|------------------------------------|:--------:|------------|---------------------------------------------------------------------------------|
| `command`         | string or non-empty list of strings |   yes    |            | A shell command line, or a program and its arguments run without a shell.       |
| `capture`         | `"stdout"` or `"status"`           |    no    | `"stdout"` | The trimmed standard output, or `"true"`/`"false"` from the exit status.        |
| `command-timeout` | duration                           |    no    | `"5s"`     | How long the command may run before it is killed. Greater than zero.            |

Unknown fields, empty command lists, invalid capture modes or durations, and
zero timeouts are load errors.

[Execution](environment.md#how-dynamic-commands-are-run) defines shell selection,
working directory, streams, limits, and capture semantics. Remote command
declarations require [`allow-dynamic-vars`](#git).

#### GitHub release declarations

A `github-release` declaration's value is the tag of a GitHub repository's
latest release, so a manifest can follow a project's releases without a fixed
URL. A [template](#templates) builds the download URL from it:

```toml
[vars.fzf_tag]
github-release = "junegunn/fzf"
cache = "1d"

[remotes.fzf]
type = "archive"
url = "https://github.com/junegunn/fzf/releases/download/${fzf_tag}/fzf-${fzf_tag.substring(1)}-linux_${{'x86_64': 'amd64', 'aarch64': 'arm64'}[facts.arch]}.tar.gz"
```

| Field            | Type                                      | Required | Default                | Description                                                        |
|------------------|-------------------------------------------|:--------:|------------------------|--------------------------------------------------------------------|
| `github-release` | string                                    |   yes    |                        | `OWNER/REPO` whose latest release's tag is the value.              |
| `github-url`     | [restricted template](#the-github-url-field) |    no    | `"https://github.com"` | The GitHub host to ask, as an `http://` or `https://` URL.         |
| `timeout`        | duration                                  |    no    | `"10s"`                | How long the whole lookup may take. Greater than zero.             |

`OWNER/REPO` is checked as the manifest loads: an owner of ASCII letters,
digits, and `-`; one `/`; and a repository of those, `_`, and `.`, but not `.`
or `..`. A scheme, a host, or a `.git` suffix is a load error. It is not a
template.

The value is the tag exactly as GitHub names it, such as `v0.65.2`, with
nothing stripped; remove a prefix in the template, as
`${fzf_tag.substring(1)}` does above. "Latest" is GitHub's: the most recent
release that is neither a draft nor a prerelease.
[How a lookup is made](environment.md#how-github-release-lookups-are-made)
defines the request and its failures. A lookup runs no code, so a remote's
declarations need no permission. Asset naming (`amd64` or `x86_64`, `darwin` or
`macos`, with or without a `v`) is the manifest's to spell; batfiles does not
guess it.

When an asset's name holds no version, no variable is needed:
`https://github.com/OWNER/REPO/releases/latest/download/ASSET` is a plain URL
that always names the latest release's asset.

What a download built from the tag does on a new release depends on what
fetches it. A [file or archive remote](#materialization) refetches when its
resolved URL changes, so the next `sync` after a release downloads it. A
[seed](safety.md#seeds-do-not-replace-and-so-do-not-refuse), such as
`fetch-archive`, keeps what it first installed until
[`--refresh-content`](safety.md#refreshing-seeds). A fixed `sha256` matches one
release, and a templated one can select a known digest for each tag; see
[what is not sandboxed](safety.md#what-is-not-sandboxed).

##### The `github-url` field

`github-url` names the host a declaration asks: GitHub Enterprise Server, for
example. Declarations may mix hosts. No environment variable changes the
default.

```toml
[vars.internal_cli_tag]
github-release = "platform/cli"
github-url = "${env.GHE_URL}"
```

It is a template that reads only `env` and `facts`, which exist before any
variable resolves. A reference to a variable, bare or through `vars`, is a load
error: "`github-url` may read `env` and `facts`, not variables". In an included
manifest, reading `env` needs the remote's
[`allow-env-templates`](actions/include-remote.md#templates-and-the-host-environment),
as any template there does; a literal URL needs nothing.

The value must be an `http://` or `https://` URL naming a host, with a numeric
port if any, optionally followed by a path; one trailing `/` is removed. The
release path is appended to it, so a query or fragment is refused, as are
credentials before an `@`: a lookup sends none, and the resolved URL is
recorded in the cache. A literal is checked when the manifest loads, and its
error quotes it unless it holds an `@`; a templated value is checked each
time the declaration resolves, including under `vars list --no-refresh`, because
the resolved URL is part of what a cache entry
[records](state.md#dynamic-varstoml-dynamic-variable-cache).

A `github-url` that fails to evaluate or resolves to an invalid URL fails the
lookup rather than the run: the variable reads as empty, with a warning, and no
cached value is used, since there is no URL to match an entry against. An
unset environment variable is the empty string, so `${env.GHE_URL}` on a
machine without it warns rather than stopping a `sync`. Errors name the field
and its template, never the resolved value.

#### Duration values

`cache`, `command-timeout`, and `timeout` are strings holding a number and a unit,
optionally repeated: `30s`, `5m`, `1h`, `1d`, `1w`, `1h 30m`. Units may be
abbreviated or spelled out (`2 hrs`, `90 minutes`), separated by whitespace or
commas, and sub-second units are accepted (`500ms`). A clock-style `HH:MM:SS`
form is accepted too (`01:30:00`). A fraction is allowed on the last unit
written, and only for hours or smaller: `1.5h` and `1m 30.5s` are durations,
`1.5d` and `1.5h 30m` are not. ISO 8601 durations such as `PT1H` are not.

A day is exactly 24 hours and a week exactly 7 days. Months and years have no
fixed length, so they are not durations. A negative duration, written `-1h` or
`1h ago`, is invalid. Zero is a valid `cache`, where it means a value is never
fresh, and an invalid `command-timeout` or `timeout`, which would stop every
capture before it could answer.

## Conditions

Actions, clone-list entries, remotes, and default-disabled candidates may carry
one condition:

| Field | The record applies when |
| --- | --- |
| `when` | True |
| `unless` | False |

Writing both is a load error.

```toml
[[actions]]
type = "symlink"
id = "gitconfig-work"
source = "git/gitconfig.work"
dest = "~/.gitconfig"
when = "work && facts.os == 'macos'"
```

### The expression

Values use [Simple Expressions](https://github.com/abatkin/expressions-rs).
Every expression is parsed during document loading, even if never evaluated;
a syntax error names the file, the record or clone-list line, and the position
in the expression.

User variables are string-valued bare identifiers. Reserved namespaces
[`facts`](environment.md#host-facts),
[`env`](environment.md#host-environment), and `vars` also return
strings. Use member syntax for identifier-compatible keys (`facts.os`) or
index syntax for any key (`env["XDG_CURRENT_DESKTOP"]`).

### Identifiers

| Lookup | Declared/set | Missing |
| --- | --- | --- |
| Bare user variable | Its string value, including empty | Evaluation error |
| `vars.<name>` | Same user value | Empty string |
| `facts.<key>`, `env.<key>` | Host value | Empty string |

Use a bare identifier to catch undeclared names, or `vars` for optional ones:

```toml
when = "work"                     # error if undeclared
when = "vars.work"                # false if undeclared
when = "vars.work || vars.school"
```

All variable names allow member syntax; index syntax is also accepted.
[Precedence](environment.md#variable-precedence) chooses each user value.

### Truthiness

The condition result and every `&&`, `||`, and `!` operand use this table:

| Value | Boolean interpretation |
| --- | --- |
| Boolean | Itself |
| Number | False at zero, otherwise true |
| `"true"`, `"1"`, `"yes"`, `"on"` | True |
| `"false"`, `"0"`, `"no"`, `"off"`, `""` | False |
| Anything else | Evaluation error |

Comparison and `+` retain the expression language's rules. For example,
`profile = "personal"` needs a comparison, not `when = "profile"`.

### When a condition cannot be evaluated

Undeclared bare identifiers, invalid truthiness, arithmetic overflow, and other
evaluation failures close the gate for **both `when` and `unless`**. The record
is skipped, with [a warning](cmdline.md#exclusion-reporting) that permits
[continuation](cmdline.md#execution-failures). Diagnostics include the condition
and remedy but never the offending value.

A failed dynamic declaration with no cached value is still declared and reads
as the empty string; it does not cause an undeclared-name error.
[Selection](cmdline.md#selection-by-command) determines when action conditions
are evaluated or waived. Bootstrap conditions are evaluated only when their
candidates are offered.

## Templates

A **template string** is a field in which each `${...}` is an
[expression](#the-expression) whose value is written into the text:

```toml
[remotes.tool]
type = "file"
url = "https://downloads.example.com/tool/${facts.os}-${facts.arch}/tool.gz"
decompress = true
```

`$${` writes a literal `${`; any other `$` is ordinary text.

### Template fields

| Record | Template fields |
| --- | --- |
| [`git`](#git) remote | `url`, `ref` |
| [`file`](#file) and [`archive`](#archive) remotes | `url`, `sha256` |
| [`fetch-file`](actions/fetch-file.md#fetch-file), [`fetch-archive`](actions/fetch-archive.md#fetch-archive) | `source`, `dest`, `sha256` |
| [`git-clone`](actions/git-clone.md#git-clone) | `source`, `dest`, `ref` |
| [`symlink`](actions/symlink.md#symlink), [`copy`](actions/copy.md#copy), [`create-dir`](actions/create-dir.md#create-dir) | `dest` |
| [`symlink-dir`](actions/symlink-dir.md#symlink-dir), [`copy-dir`](actions/copy-dir.md#copy-dir), [`git-clone-list`](actions/git-clone-list.md#git-clone-list) | `dest-dir` |
| [`run`](actions/run.md#run) | `command`: the shell string, or each list element; `cwd`; each `env` value |
| [`github-release`](#github-release-declarations) variable | `github-url`, which reads only `env` and `facts` |

No other field is a template.

### When a template is evaluated

A template reads what a [condition](#identifiers) on the same record reads.
Remotes and leaf actions use the leaf's variables; an
[inclusion's](actions/include-remote.md#variables-for-one-inclusion) actions use
the inclusion's. A template in an included manifest reads `env` only with its
remote's [permission](actions/include-remote.md#templates-and-the-host-environment).
Each `${...}` must give a string, number, or boolean.

Templates are parsed when the manifest loads, so a syntax error is a load error
naming the record and field.
Each is evaluated once, when needed, including in dry runs:

- a `github-url` each time its declaration resolves, against `env` and `facts`
  alone; see [the field](#the-github-url-field);
- a remote's when `sync` or `clone` [materializes](#materialization) it;
- an action's when the action is dispatched, including by
  [`apply-action`](commands/apply-action.md#apply-action); a `run` action's only
  when its [trigger](actions/run.md#triggers) fires.

Output shows resolved values, except a `run` action's command, which is shown
as written. A literal value is checked when the manifest
loads; a templated one is checked by the same rule once it resolves.

### When a template cannot be used

A template that fails to evaluate, or resolves to an invalid value, fails its
record instead of skipping it as a [condition](#when-a-condition-cannot-be-evaluated)
does: an [execution failure](cmdline.md#execution-failures) for an action, a
[materialization failure](#materialization) for a remote. A `github-url` is the
exception: it fails its lookup, and the variable
[reads as empty](#the-github-url-field).

```text
error: create-dir action 2: dest "~/${nope}" cannot be evaluated at character 3: `nope` is not declared. ...
```

Errors never show a variable's value or anything computed from it, including
an invalid resolved value.

## Default-disabled bootstrap entries

`[default-disabled]` proposes initial disables for a fresh machine:

```toml
[[default-disabled.actions]]
id = "p10k"

[[default-disabled.groups]]
group = "gui"
unless = "facts.os == 'macos'"
```

### Action entry

| Field | Type | Required | Description |
| --- | --- | :---: | --- |
| `id` | address | yes | Action or clone-list entry to disable |
| `when` / `unless` | condition | no | Offer the candidate only when its gate passes; at most one |

### Group entry

| Field | Type | Required | Description |
| --- | --- | :---: | --- |
| `group` | address | yes | Group to disable |
| `when` / `unless` | condition | no | Offer the candidate only when its gate passes; at most one |

Records are closed and [address syntax](cmdline.md#addresses) is validated, but
names are never resolved. Candidates may name included content or actions a
later repository revision introduces.

Only the leaf's candidates are considered, by
[`clone` or `sync --bootstrap`](commands/clone.md#what-the-bootstrap-decides).
[Bootstrap adoption](state.md#bootstrap-adoption) defines eligibility and
persistence; [precedence](environment.md#bootstrap-adoption-precedence) defines
overrides. Conditions are always parsed at manifest load, and evaluated only
when candidates are offered during bootstrap. Other commands ignore the section.
