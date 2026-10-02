# Distribution

What remains of how a machine gets a `batfiles` binary: the GitHub Pages copies
of the hosted installers. The [release tree](../distribution.md), the [hosted
installers](../distribution.md#hosted-installer) for Unix and
[Windows](../distribution.md#on-windows), the [leaf
stubs](../distribution.md#leaf-stub), [`batfiles update`](../cmdline.md#update),
and [self-hosting](../distribution.md#self-hosting) are built. The [product
goals](../goals.md#product-model) state the scope; [slice
10](roadmap.md#slice-10--distribution) numbers the work.

## GitHub Pages

When the repository has GitHub Pages enabled, the release workflow also
publishes copies of the two hosted installers at the site root, for a shorter
one-liner; a fork without Pages skips that job, and everything keeps working
from the release URL alone. Open: how the job coexists with other content on the
same Pages site, and that only a stable release replaces the copies.

## On promotion

The Pages copies join [`docs/distribution.md`](../distribution.md), beside the
release tree they copy from.
