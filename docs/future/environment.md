# Environment variables

Everything batfiles reads from the environment — capture, location selection,
run-only skips, the bootstrap lists and their adoption precedence, one-shot
variables and every layer of their precedence, dynamic variables' declarations
and the environment their commands run in, the `facts` and `env` namespaces a
condition reads, color, and Git inheritance — is specified in [the environment
reference](../environment.md). What stays here is below.

## General precedence

For proposed inputs, precedence is command-line arguments, then environment
variables, configuration files, and built-in defaults. Variable declarations
follow the built [variable precedence](../environment.md#variable-precedence).

## Run-only skips

The existing [run-only skip inputs](../environment.md#run-only-skips) will also
resolve qualified addresses in included remotes. See the proposed
[address forms](cmdline.md#address-forms).

## Bootstrap enable and disable lists

The four `BATFILES_*` bootstrap lists and the
[adoption precedence](../environment.md#bootstrap-adoption-precedence) they take
part in are built, and are specified in
[`docs/environment.md`](../environment.md#bootstrap-enable-and-disable-lists).
Nothing about them is outstanding; what they resolve against is, at
[default-disabled entries](repoformat.md#default-disabled-bootstrap-entries).

## Color

Color selection is built. It is specified in
[`docs/environment.md`](../environment.md#color).

## Condition namespaces

Both are built. The `facts` namespace and the `env` namespace are specified in
[the environment reference](../environment.md#host-facts-in-conditions), along
with the keys `facts` contains, the empty-string rule a missing key follows in
either, and the two channels one environment variable can reach a condition
through.

## Bootstrap use of `PATH`

A generated `install.sh` uses a `batfiles` binary found on `PATH`, and the
binary reads `BATFILES_BASE` for `init` and `update`. Both are specified in
[distribution](distribution.md#resolving-a-binary).

## Deliberate exclusions

The specs intentionally define no environment-variable equivalent for:

- `--dry-run`;
- `--refresh-vars`;
- `--refresh-remotes`;
- `--refresh-content`;
- explicit apply commands; or
- verbosity (`--verbose`/`--quiet`).

General persisted enable/disable commands also have no ambient environment
equivalent. The [`BATFILES_ENABLE_*` and
`BATFILES_DISABLE_*`](../environment.md#bootstrap-enable-and-disable-lists)
variables are the narrow exception, and a deliberately narrow one: they are read
only by a bootstrap command.
