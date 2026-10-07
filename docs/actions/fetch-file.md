# `fetch-file`

**Unreleased:** `executable` and `decompress` require a build newer than 0.1.0.

Declares one file downloaded to a destination where nothing is.

```toml
[[actions]]
type = "fetch-file"
id = "pathogen"
source = "https://raw.githubusercontent.com/tpope/vim-pathogen/master/autoload/pathogen.vim"
dest = "~/.vim/autoload/pathogen.vim"
```

| Field    | Type   | Required | Description                                                          |
|----------|--------|:--------:|----------------------------------------------------------------------|
| `source` | string |   yes    | An `http://`, `https://`, or [`file://`](../repoformat.md#file-urls) URL. Not a repository path. |
| `dest`   | string |   yes    | Where the file goes, exactly. Never empty; `~` is the home.          |
| `sha256` | string |    no    | 64 hexadecimal digits: the digest the fetched bytes must have.       |
| `executable` | boolean | no  | Whether the file lands executable. Defaults to `false`.              |
| `decompress` | boolean | no  | Whether the download is a gzip or bzip2 stream holding the file. Defaults to `false`. |

The shared [transfer](../repoformat.md#the-transfer-both-fetching-actions-share),
[seed](../safety.md#seeds-do-not-replace-and-so-do-not-refuse),
[staging](../safety.md#staging-and-publication), and
[permission](../safety.md#installed-permissions) rules apply. An occupied
destination costs no transfer unless [refresh](../safety.md#refreshing-seeds) is
requested. On Unix, `executable` installs the file `0755` rather than `0644`.

Without `decompress`, downloaded bytes are installed unchanged, including any
archive. With it, gzip or bzip2 compression is detected from the leading bytes,
including concatenated streams, and the decompressed file is installed, as a
release publishing `tool-linux.gz` intends:

```toml
[[actions]]
type = "fetch-file"
source = "https://example.com/tool-linux.gz"
dest = "~/.local/bin/tool"
decompress = true
executable = true
```

Uncompressed data, a zip, or content that decompresses to a tar fails; use
`fetch-archive` for archives. `sha256` checks the downloaded bytes before
decompression.

See [common action fields](../repoformat.md#actions) for `id`, `group`, and
conditions.
