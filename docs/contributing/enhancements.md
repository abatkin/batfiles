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
[`facts.hostname`](../environment.md#host-facts-in-conditions) uses `gethostname`,
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
