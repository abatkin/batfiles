# Release management

For the repository owner: the settings releases rely on, how to cut one, and
how to build and check the pieces locally. What a release contains and what
each task does is specified in [`docs/contributing/distribution.md`](../docs/contributing/distribution.md).

## Repository setup

One-time settings on GitHub. The workflows work without them, but without them
anyone who can push a tag can publish a release, and a published release can
be changed afterwards.

- [X] **Immutable releases.** Settings → General → Releases → *Enable release
  immutability*. A published release's assets and tag are then locked, and its
  tag name can never be reused, even after deleting the release. Drafts stay
  editable, which is what `dist:publish` relies on, and the title, notes, and
  pre-release and latest flags stay editable too. The setting covers every
  release, pre-releases included, which is why release candidates are numbered
  rather than retried.
- [X] **A tag ruleset for `v*`.** Settings → Rules → Rulesets → *New tag
  ruleset*: target tags matching `v*`, enforcement *Active*, and the rules
  *Restrict creations*, *Restrict updates*, and *Restrict deletions*, with
  *Repository admin* on the bypass list. Then only you can create or move a
  release tag.
- [X] **The `release` environment.** Settings → Environments → `release`
  (created by the first release run, or create it first). Under *Deployment
  branches and tags*, choose *Selected branches and tags* and add the tag
  pattern `v*`. Optionally add yourself under *Required reviewers*, so each
  publish waits for your approval on the workflow run.
- [X] **Read-only workflow token.** Settings → Actions → General → *Workflow
  permissions* → *Read repository contents and packages permissions*. Each job
  that writes asks for its own permissions.
- [ ] **`BATFILES_BASE`, only if needed.** A repository variable, set only to
  serve releases from somewhere other than this repository's GitHub releases.

The [Pages site](../docs/contributing/distribution.md#github-pages) needs two more; without
them, the Pages workflow deploys nothing and says so:

- [X] **Pages source.** Settings → Pages → *Build and deployment* → *Source*:
  *GitHub Actions*.
- [X] **The `github-pages` environment.** Settings → Environments →
  `github-pages`, which choosing the source creates. By default it accepts
  deployments from the default branch only. Under *Deployment branches and
  tags*, add the tag pattern `v*`, so that the release workflow can deploy.

The rest put the site at `batfiles.dev`, in this order:

- [X] **Verify the domain.** Your profile's Settings → Pages → *Add a domain*:
  `batfiles.dev`. Create the TXT record it shows,
  `_github-pages-challenge-abatkin.batfiles.dev`, then *Verify*. Keep the
  record; it stops another account from claiming the domain while the site
  is down.
- [X] **DNS.** At the apex, `A` records for `185.199.108.153`,
  `185.199.109.153`, `185.199.110.153`, and `185.199.111.153`, and `AAAA`
  records for `2606:50c0:8000::153`, `2606:50c0:8001::153`,
  `2606:50c0:8002::153`, and `2606:50c0:8003::153`, with any other apex `A` or
  `AAAA` records, such as a registrar's parking page, removed. Optionally a
  `CNAME` for `www` to `abatkin.github.io`, which Pages then redirects to the
  apex. If the domain has `CAA` records, one must allow `letsencrypt.org`.
- [X] **Custom domain.** Settings → Pages → *Custom domain*: `batfiles.dev`,
  then *Save*. No `CNAME` file is involved; an Actions-built site ignores one.
- [X] **HTTPS.** Once the DNS check passes and the certificate is issued, which
  can take up to an hour, tick *Enforce HTTPS*. `.dev` is HTTPS-only in every
  browser, so the site is unreachable from one until then.

## Cutting a release

`Cargo.toml` always holds `X.Y.Z`, the release being worked toward. Tags carry
the rest; see [versions](../docs/contributing/distribution.md#versions).

### A release candidate

From any branch, with the commit to release checked out and nothing
uncommitted:

```sh
task release:rc                 # tags vX.Y.Z-rc.<n>, locally
git push origin vX.Y.Z-rc.<n>   # the command it prints
```

Then watch the *Release* workflow, and approve the `release` environment if it
asks. After publishing, its `verify` job checks the release's checksums and
stamps, and its `smoke` jobs install it with its own one-liners on Linux x86_64,
Linux aarch64, macOS, and Windows, where a stable release must also be what the
installed binary's `update --check` finds. A candidate is published as a pre-release and never becomes `latest`, so
trying its installer takes the version:

```sh
curl -fsSL https://github.com/abatkin/batfiles/releases/download/vX.Y.Z-rc.<n>/install.sh |
    BATFILES_VERSION=X.Y.Z-rc.<n> sh
```

If the run fails before publishing, delete any draft it left
(`gh release delete vX.Y.Z-rc.<n>`), fix the problem, and run `task release:rc`
again; it takes the next number.

### A stable release

1. Remove each `**Unreleased:**` label from the published documentation, as
   [authoring](../docs/contributing/authoring.md#links-and-examples) describes;
   `dist:tag` refuses a stable tag while any remain. Merge the work, with
   `Cargo.toml` at `X.Y.Z`, into `main`.
2. Tag the merged commit and push the tag:

   ```sh
   git switch main && git pull
   task release:stable             # tags vX.Y.Z, locally
   git push origin vX.Y.Z
   ```

3. Watch the *Release* workflow as for a candidate. After `verify`, its `pages`
   job starts the *Pages* workflow on `main`; watch that run too. It deploys the
   site with the new installers and checks that `https://batfiles.dev` serves
   them. A site change on `main` deploys on its
   own, and *Run workflow* on the *Pages* workflow redeploys by hand.
4. In the next change, set `Cargo.toml` to the next version. Until then, both
   `release:` tasks refuse, since `vX.Y.Z` is taken.

## Building and checking locally

`task ci` runs the `tests/dist/` target, which exercises every script but
`dist:binary` and `dist:publish` against stand-in binaries, and the
pristine-machine container, which installs a real binary with the one-liner
under `dash`. CI also runs `task test` on macOS, against the real BSD tools.
`task lint` needs `shellcheck`. To build real binaries:

```sh
task dist:binary TARGET=x86_64-unknown-linux-musl
task dist:binary TARGET=x86_64-pc-windows-msvc CROSS=xwin
```

The musl build needs `musl-gcc` (Fedora's `musl-gcc` package, Debian's and
Ubuntu's `musl-tools`); the Windows one needs `cargo xwin`, as `task lint`
does. Building for macOS needs a Mac. `VERSION=X.Y.Z-rc.<n>` builds a binary
that reports a pre-release. Binaries accumulate in `target/assets/`, and
assembly takes all of them, so clear it before assembling a different set.

To assemble them and check the result as a release tree served from disk:

```sh
tree=$PWD/target/tree
task dist:assemble VERSION=0.1.0 BASE="file://$tree" OUT=target/release-tree
mkdir -p "$tree/download" "$tree/latest"
cp -R target/release-tree "$tree/download/v0.1.0"
cp -R target/release-tree "$tree/latest/download"
task dist:verify URL="file://$tree" VERSION=0.1.0 LATEST=yes
```

To check a published release, and a downloaded binary's provenance:

```sh
task dist:verify URL=https://github.com/abatkin/batfiles/releases VERSION=X.Y.Z LATEST=yes
gh attestation verify batfiles-x86_64-unknown-linux-musl --repo abatkin/batfiles
```

Use `LATEST=no` for a pre-release.

To build the Pages site from it:

```sh
task dist:pages OUT=target/site FROM="file://$tree"
```

To try the installer against the local tree above without touching your own
`~/.local/bin`:

```sh
BATFILES_BIN=/tmp/try-batfiles/batfiles sh target/tree/latest/download/install.sh
```
