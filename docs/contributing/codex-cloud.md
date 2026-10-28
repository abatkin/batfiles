# Set up Codex Cloud

Prepare a Cloud environment for `abatkin/batfiles` with the tools needed by
[`task ci`](../../AGENTS.md#canonical-commands). Use an Ubuntu x86_64 VM with
a non-root development user and passwordless sudo. The installer adds system
packages and tools under `/usr/local/bin`; run it in the Cloud VM, not on your
everyday machine.

## Create the environment

In **Settings > Codex Cloud > Environments > Create environment**, select
`abatkin/batfiles` and name the environment `batfiles`. Keep access set to
**Only me**. No project secrets or environment variables are required.

The selected checkout must contain
[`scripts/codex-install.sh`](../../scripts/codex-install.sh). A local branch is
not available to Cloud until it has been pushed; use that branch during setup,
or wait until the change has been reviewed and merged into the default branch.

Set **Install script** to the following, with the repository as the working
directory:

```sh
sh scripts/codex-install.sh
```

The script installs the compiler dependencies, Task, ShellCheck, `cargo-deny`,
and `cargo-xwin`. It uses [rust-toolchain.toml](../../rust-toolchain.toml) for
Rust's version, components, and Windows target, and the existing documentation
installer for mdBook and lychee. If PowerShell is present it installs a missing
PSScriptAnalyzer module. It fetches locked Cargo dependencies and runs
`task lint` and `task deny` to prepare the Windows SDK and advisory caches.
Rerunning it refreshes those dependencies and keeps tools already at the
requested versions.

No background service or **Start skill** is needed for this CLI project.
See OpenAI's [Cloud environment guide](https://learn.chatgpt.com/docs/environments/cloud-environments)
for the current setup and publishing interface.

## Network access

Enable **package manager domains**. Additional destinations used by the setup
and checks include:

| Purpose | Domains |
| --- | --- |
| Ubuntu packages | The mirrors configured in the VM's APT sources, typically `archive.ubuntu.com` and `security.ubuntu.com` |
| Rust installation and crates | `sh.rustup.rs`, `static.rust-lang.org`, `index.crates.io`, `static.crates.io` |
| Tool releases and RustSec advisories | `github.com`, `api.github.com`, `release-assets.githubusercontent.com`, `objects.githubusercontent.com`, `raw.githubusercontent.com` |
| Windows SDK downloads | `aka.ms`, `download.visualstudio.microsoft.com`, `download.microsoft.com` |
| PSScriptAnalyzer, when PowerShell is installed | `www.powershellgallery.com`, `cdn.powershellgallery.com` |

Keep these available during tasks: `task deny` refreshes advisories, and changes
to dependencies or tool pins can require downloads. If a download redirects
to another host, use the failed request's hostname to extend the allowlist and
rerun setup. The test suite itself uses local fixtures; documentation link
checks are offline. A working Docker or Podman runtime additionally needs its
configured image registry for the pristine-machine test.

## Validate and publish

Run this as the development user in the Cloud setup session. The
[permission tests](architecture.md#test-environments) require an unprivileged
account:

```sh
task ci
```

Check the output for the repository's documented
[optional checks](../../AGENTS.md#canonical-commands): container acceptance
reports a skip without a working Docker or Podman, and PowerShell analysis
reports a skip without `pwsh`. The Cloud installer does not install either
runtime. GitHub CI remains responsible for the native macOS and Windows jobs.

After the checks pass, save and **Publish** the environment. Start a new Cloud
task and run `task ci` again to verify that tools and network settings persist.
When toolchain pins, dependencies, or this installer change, edit the
environment, rerun installation and validation, then **Republish**. Repository
refreshes do not rerun the Install script automatically.
