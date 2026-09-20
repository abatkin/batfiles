# Environment variables

This document proposes what dynamic declarations do to variable precedence.
Everything else batfiles reads from the environment — capture, location
selection, run-only skips, the bootstrap lists and their adoption precedence,
one-shot variables and every layer of their precedence, the `facts` and `env`
namespaces a condition reads, color, and Git inheritance — is specified in
[the environment reference](../environment.md).

## General precedence

For proposed inputs, precedence is command-line arguments, then environment
variables, configuration files, and built-in defaults. Variable declarations
follow the more detailed [runtime precedence](#runtime-variable-precedence).

## Runtime variable precedence

Every layer a run stacks is built and specified in [the environment
reference](../environment.md#variable-precedence), including the two an
`include-remote` derives and what they reach. What is proposed here is how a
declaration that yields no value behaves.

All values follow the repository format's shared [string-valued variable
model](repoformat.md#string-valued-variables). Any coercion during condition
evaluation is performed by the expression language rather than by batfiles.

Declarations, not values, participate in precedence, which decides two cases the
lists above do not:

- A dynamic declaration a remote is not allowed to run contributes no variable
  at all, so a lower layer's value stands. This is what
  [`allow-dynamic-vars = false`](state.md#freshness-and-refresh-behavior) leaves
  behind: the command never runs, and the variable is not merely valueless but
  absent from that layer.
- A dynamic declaration that does run and produces no value still overrides the
  layers beneath it. The variable has no value rather than the lower layer's:
  the higher declaration won, and it produced nothing.

### Run-only skips

The existing [run-only skip inputs](../environment.md#run-only-skips) will also
resolve qualified addresses in included remotes. See the proposed
[address forms](cmdline.md#address-forms).

### Bootstrap enable and disable lists

The four `BATFILES_*` bootstrap lists and the
[adoption precedence](../environment.md#bootstrap-adoption-precedence) they take
part in are built, and are specified in
[`docs/environment.md`](../environment.md#bootstrap-enable-and-disable-lists).
Nothing about them is outstanding; what they resolve against is, at
[default-disabled entries](repoformat.md#default-disabled-bootstrap-entries).

### Color

Color selection is built. It is specified in
[`docs/environment.md`](../environment.md#color).

## Condition namespaces

Both are built. The `facts` namespace and the `env` namespace are specified in
[the environment reference](../environment.md#host-facts-in-conditions), along
with the keys `facts` contains, the empty-string rule a missing key follows in
either, and the two channels one environment variable can reach a condition
through.

## How dynamic commands are run

Dynamic variable commands inherit the `batfiles` process environment. Batfiles
does not export its resolved user-variable scope as environment variables. A
dynamic command that needs an environment input must read it from the normal
process environment; otherwise it should read files under the repository that
declared it.

Each command runs with its working directory set to the root of the repository
that declared it — the leaf repository, or the materialization of the remote
that declared the variable. A relative path in the command therefore resolves
there rather than in whatever directory `batfiles` was invoked from.

A `command` written as a string runs under `sh -c` on Unix and `cmd /C` on
Windows. It is never run under the user's login shell: a login shell runs that
user's startup files, so the same manifest would capture different values on two
machines whose owner happens to prefer a different interactive shell. A `command`
written as a list is executed directly, with no shell at all.

The child's three standard streams are connected as follows:

- **Standard input** is connected to nothing. A dynamic command that reads it
  sees end of input immediately rather than blocking on a terminal that may have
  nobody watching it.
- **Standard output** is captured for `capture = "stdout"` and discarded for
  `capture = "status"`. It is never inherited, so a dynamic command cannot write
  into the data channel described in [Output Streams](cmdline.md#output-streams).
  Only what the command had written when it exited becomes the value: a process
  it leaves running in the background neither extends the value nor delays the
  capture.
- **Standard error** is inherited, so the command's own diagnostics reach the
  user verbatim. `--quiet` disconnects it instead; batfiles still reports a
  failed capture with a warning of its own, but the command's explanation of the
  failure is lost until the same command is run again without the flag.

The `command-timeout` field bounds the whole run and defaults to `5s`. It must be
greater than zero. When it expires, batfiles kills the command and the run is a
**failure** for both capture modes: a command that was cut off never answered the
question, so `capture = "status"` reports a failure rather than the string
`"false"`. Only the command batfiles started is killed; a command string that
backgrounds a further process of its own leaves that process running.

A captured value is bounded as well as timed: a `capture = "stdout"` command that
writes more than **1 MiB** is stopped and its refresh fails. The output is not
truncated to fit, because a value cut in half is worse than no value at all, and
the limit applies while the command runs so that a command writing without end
cannot fill the temporary directory before its timeout expires.

## Bootstrap use of `PATH`

A generated `install.sh` uses a `batfiles` binary found on `PATH`. The product
goals describe the remaining [bootstrap model](../goals.md#product-model).

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
