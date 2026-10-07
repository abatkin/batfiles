# `fetch-archive`

Declares one archive downloaded and unpacked at a destination where nothing is.

```toml
[[actions]]
type = "fetch-archive"
id = "fzf"
source = "https://example.com/fzf-0.65.2.tar.gz"
dest = "~/.local/fzf"
archive-root = "*"
```

| Field          | Type   | Required | Description                                                            |
|----------------|--------|:--------:|------------------------------------------------------------------------|
| `source`       | string |   yes    | An `http://`, `https://`, or [`file://`](../repoformat.md#file-urls) URL. Not a repository path. |
| `dest`         | string |   yes    | Where the unpacked directory goes, exactly. Never empty; `~` is the home. |
| `sha256`       | string |    no    | 64 hexadecimal digits: the digest the fetched archive must have.       |
| `archive-root` | string |    no    | A prefix every entry is written without, spelled as an entry path is, or `*` for the archive's single top-level directory. |
| `include`      | glob or list | no | Entries to unpack, by their path once the root is stripped; absent, every entry. See [entry filters](../repoformat.md#entry-filters). |
| `exclude`      | glob or list | no | Entries not to unpack.                                                 |
| `executable`   | glob or list | no | Files to make executable, by their path once the root is stripped.     |

The shared [transfer](../repoformat.md#the-transfer-both-fetching-actions-share) supplies the
archive. The [seed policy](../safety.md#seeds-do-not-replace-and-so-do-not-refuse)
installs one whole directory: an occupied `dest`, including one an earlier
`create-dir` made, causes no request. [Refresh](../safety.md#refreshing-seeds)
replaces the whole tree. Extraction follows [archive safety](../safety.md#archive-extraction),
[permissions](../safety.md#installed-permissions), and staged publication.

Formats are detected from leading bytes:

| Format | Support |
| --- | --- |
| Tar | Plain, gzip, or bzip2; concatenated compressed streams; V7, ustar, GNU, and pax |
| Zip | Stored, deflated, or bzip2 entries; no encryption or other compression methods |

Plain tar is recognized by its first header's checksum. Zip must begin with an
entry or an empty central-directory ending; self-extracting prefixes are not
supported. Unsupported bodies fail with the detected format, such as xz or zstd.
Zip names use UTF-8 when flagged or valid, otherwise code page 437; backslashes
in names are refused. Unix modes determine zip symlink and permission metadata.

`archive-root` strips a prefix. `"*"` requires a single top-level directory and
strips it; multiple roots fail naming them. An explicit prefix installs only
entries beneath it. A prefix with nothing beneath it fails. Prefixes follow
entry-path syntax: absolute paths and `..` are load errors.

Filters and `executable` match paths after root stripping:

```toml
[[actions]]
type = "fetch-archive"
source = "https://example.com/tool.tar.gz"
dest = "~/.local/tool"
archive-root = "*"
include = ["bin", "lib"]
exclude = "**/*.md"
executable = ["bin", "lib/*.so"]
```

Directories needed to hold selected entries retain their archive modes.
Selecting nothing, or selecting a hard link whose target is excluded, fails.
Filters do not relax archive safety. Unmatched patterns are reported at `-v`
only when extraction runs, never during a dry run.

`executable` adds owner, group, and other execute bits to matched files and
files beneath matched directories, regardless of archive modes. It does not
change directories, symlinks, or excluded files; hard links share their target's
mode. A pattern marking no file is reported at `-v`. It has no effect on Windows.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
