# Environment variables

The environment inputs batfiles reads today. The rest — the location variables,
one-shot variable overrides, run-only skips, bootstrap adoption, and the host
facts conditions use — are in [`future/environment.md`](future/environment.md),
along with the table naming every variable in the intended set.

## Color

Color selection follows this precedence:

```text
--color > BATFILES_COLOR > non-empty NO_COLOR > auto
```

`BATFILES_COLOR` accepts `auto`, `always`, or `never`. An absent or empty
`BATFILES_COLOR` is treated as unset, and the next input in precedence decides.
Any other unrecognized value produces a diagnostic and falls back instead of
silently selecting a different color mode.

`NO_COLOR` follows the cross-tool convention: presence alone is insufficient;
its value must be non-empty. It acts as `never` only when neither `--color` nor
`BATFILES_COLOR` supplies a higher-precedence choice. Color inputs affect only
presentation.

`auto` is left unresolved by this precedence and answered by whatever is about
to print: it enables color when standard output is a terminal. Today the only
colored output is what clap renders — help, `--version`, and usage errors — and
it applies its own terminal detection. Batfiles' own warnings and errors are
uncolored until there is something that formats them.

Color is resolved before the arguments are parsed, because clap may need to
render a usage error for arguments it could not parse, and that output should
honor the requested color too. This is why `--color` is recovered from the raw
arguments rather than read off the parsed command.
