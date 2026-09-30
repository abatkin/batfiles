# Batfiles Safety Model

## Purpose and scope

Batfiles manages user-owned configuration on the user's behalf. Its safety
model is intended to prevent accidental data loss while keeping explicit
configuration useful and predictable. It is not a sandbox or a security
boundary: a repository selected by the user can name destinations outside the
home directory, included repositories can contribute installation actions, and
allowed dynamic variables can execute arbitrary commands as the current user.

These are unbuilt proposals. Implemented safety behavior, including
[conflicts and backups](../safety.md#conflicts-and-backups) and
[refreshing seeds](../safety.md#refreshing-seeds), is specified in
[installation safety](../safety.md).

## Trust model

Batfiles assumes that the user trusts the leaf repository and has deliberately
selected any included repositories. It also assumes that paths explicitly
written by the user express the user's intent. Batfiles does not attempt to
protect a user from a malicious repository that they have chosen to install.

That trust does not extend to every byte received from the network. Downloads
and archives must still be parsed defensively. An archive entry is data, not an
instruction to write outside the extraction root, create a device, or consume
unbounded resources. A configured SHA-256 digest verifies downloaded bytes;
without one, batfiles cannot promise content identity or authenticity beyond
the transport and source selected by the user.

Batfiles runs with the invoking user's permissions and does not elevate
privileges. It does not sandbox Git or filesystem access, and [dynamic
variables](../safety.md#what-is-not-sandboxed) are not sandboxed either.

## Destination paths

Current [path and destination safety](../safety.md) applies to new action types.

## Repository source paths

Remote source resolution is implemented. New remote types must follow the
current [source syntax](../repoformat.md#sources-and-destinations) and
[path safety rules](../safety.md#path-resolution).

## Git repositories

That a Git remote's materialization follows the current [Git update
policy](../safety.md#git-updates) is built and specified there.

Network and repository trust still apply. Batfiles does not guarantee signed
commits or immutable branch contents. Updating a declared Git remote can
change files and included actions on the next plan, so users should pin or
otherwise control sources where upstream mutability is unacceptable.

## Downloads and archives

Remote file and archive materialization must follow the current
[staging](../safety.md#staging-and-publication) and
[archive validation](../safety.md#archive-extraction) rules.

No metadata field supplied by an archive is trusted as a resource bound.
Extraction should stream data, count actual uncompressed bytes and entries,
and stop when it reaches an implementation-defined extraction budget. A limit
failure occurs while staging and therefore does not modify the destination.
The exact default limits and any explicit override remain an implementation
decision; no fixed limit can prevent every CPU, memory, disk, quota, or
concurrent-process exhaustion case on every supported system.

These archive rules do not contradict the destination path policy: the user
may explicitly choose an absolute destination, but the archive cannot choose a
different one.

## Action order and overlapping destinations

Expand included actions in place before capturing selection and preparing clone
lists. Preserve the current [execution order](../cmdline.md#sync): each action
inspects the filesystem left by earlier successful actions. Inclusion does not
add cross-action destination conflict detection.
