# Potential enhancements

This document tracks potential future enhancements and follow-up work. These
ideas are unscheduled, do not specify current behavior, and do not block use of
the implemented features.

## Extraction budgets

Bound archive extraction and compressed-file expansion by counting actual
uncompressed bytes and archive entries, rather than trusting metadata. Stop
while staging so that exceeding a budget leaves the destination unchanged.
Choose default limits and decide whether explicit overrides are needed. Account
for entry inventories held in memory as well as content written to disk; no
fixed budget prevents every resource-exhaustion case. Current protections are
specified in [archive safety](../safety.md#archive-extraction).

## Continue after independent failures

Allow local actions to proceed after network failures or other independent
errors. Define which errors permit continuation, avoid repeated diagnostics for
one failed parent, and specify final status and partial-success output.
Currently, `sync` materializes admitted remotes before installing local actions,
so an unavailable remote can stop unrelated work. Disabling an inclusion does
not disable its remote's materialization; the remote's own condition controls it.
Destination conflicts already continue under `--no-overwrite`; see the current
[execution failure policy](../cmdline.md#execution-failures).

## xz and zstd tar archives

Extend [archive format support](../actions/fetch-archive.md#fetch-archive) to tar.xz and
tar.zst, which are currently refused by name. Evaluate pure-Rust decoders such
as `lzma-rust2` and `ruzstd`, and add format detection and extraction coverage.

## Fully qualified Windows host name

Consider exposing the DNS-qualified Windows host name. The current
[`facts.hostname`](../environment.md#host-facts) uses `gethostname`,
whose Windows implementation requests `ComputerNamePhysicalDnsHostname` and
drops the DNS suffix. Calling `GetComputerNameExW` with
`ComputerNameDnsFullyQualified` would require a Windows API dependency and an
unsafe FFI boundary, plus validation on a suitably configured Windows machine.
Weigh a second fact against changing the existing one: manifests comparing the
short name would otherwise break.

## Git command-local overrides

Review support for `GIT_CONFIG_COUNT` values such as one-off proxy or header
settings. The current [Git environment policy](../environment.md#variables-passed-on-to-git)
clears these values. Any change must preserve destination isolation and update
that policy.

## Templates in more fields

[Templates](../repoformat.md#templates) cover destinations, fetch and clone
sources, refs, and digests. Candidates for more:

- `[vars]` values, so a variable can be built from others. Needs an evaluation
  order across layers and inclusion scopes, and cycle detection.
- Clone-list fields: an entry's repository, `ref`, and `dest-name`. Entries are
  read during preparation, so failures and scope need defining there.
- `include-remote` variable overrides, resolved in the leaf scope before the
  inclusion's scope is built.
- Repository-path `source` and `source-dir`. These are checked against declared
  remotes at load, which a templated remote reference would defer.

## Per-inclusion variable listings

Consider whether `vars list` should show a section for each inclusion's scope.
The [current listing](../commands/vars.md#vars-list) shows the leaf repository's flat set;
action commands report inclusion values at `-vv`. A listing with inclusion
sections would need to read every inclusion this machine would open, which it
does not do today.

## Versioned documentation

Consider separate documentation editions if users need to stay on multiple
supported releases. The [current site](distribution.md#the-pages-workflow)
publishes one book from main and labels unreleased behavior. Versioned editions
would need a policy for corrections, stable URLs, search scope, and retaining
old editions without publishing stale installer entry points.

## A stateless "once" for `run` actions

A `creates = "<path>"` field on a [`run` action](../actions/run.md#run) that
skips the command while the path exists, as a "once" that needs no `id` or
`runs.toml` entry and that deleting the path resets.

## Forcing `run` actions to run again

A `sync` option that waives every `run` action's trigger for one run, as
`apply-action` waives one. Today a re-run takes `apply-action` per action, or
renaming its `id`.

## Stopping a timed-out command's descendants

A timed-out `run` action or dynamic variable kills only the process batfiles
started. Killing its process group, or job object on Windows, would also stop
what it started in the background.
