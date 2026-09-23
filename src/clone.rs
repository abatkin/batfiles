//! `clone`: put a leaf repository on a machine that has none, and synchronize
//! it.
//!
//! After the download, the run is `sync`'s, with the same roots, selection,
//! variables, and reporting; [`crate::bootstrap`] decides within that run what
//! the machine starts with switched off. This module decides only the
//! destination, which [`crate::app`] resolves without working-directory
//! discovery: discovery finds a directory holding a manifest, and `clone`
//! requires a vacant one.

use crate::bootstrap::Bootstrap;
use crate::cli::BootstrapOptions;
use crate::env::Environment;
use crate::error::Error;
use crate::execute::{self, Invocation};
use crate::git;
use crate::mode::RunMode;
use crate::output::Verb;
use crate::paths;

/// Clone `url` into the selected leaf repository and synchronize it.
///
/// The destination must be vacant and the bootstrap options valid; both are
/// checked before Git runs. Nothing is unwound afterwards: the repository is
/// kept even if the synchronization fails, so the fix is an edit and a `sync`.
pub(crate) fn run(
    invocation: &Invocation<'_>,
    url: &str,
    options: &BootstrapOptions,
    env: &Environment,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    // Before the clone, so an invalid option fails with nothing downloaded.
    let bootstrap = Bootstrap::read(options, env, invocation.reporter)?;

    let dest = &invocation.roots.batfiles_dir;
    // Presence, not kind: `clone` creates the directory it clones into, so
    // anything at all there -- an empty directory, a file, a link to nothing --
    // is something batfiles did not put there and will not write over.
    if paths::occupied(dest)? {
        return Err(Error::CloneDestinationExists { path: dest.clone() });
    }

    git::clone_repository(url, dest)?;
    invocation.reporter.info(&format!(
        "{} {} from {url}",
        // `clone` has no `--dry-run`: with nothing cloned, there is no plan.
        Verb::Clone.say(RunMode::Perform),
        dest.display()
    ));

    // A Git repository without a manifest means a wrong URL; the
    // synchronization would report only a missing file.
    if !paths::occupied(&invocation.roots.manifest())? {
        return Err(Error::ClonedWithoutManifest { path: dest.clone() });
    }

    execute::bootstrap(invocation, &bootstrap, skip_actions, skip_groups)
}
