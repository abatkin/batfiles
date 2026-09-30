//! Clone a leaf repository into a vacant destination, then synchronize it with bootstrap
//! disabled-state adoption.

use crate::bootstrap::BootstrapDecisions;
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
/// Validate the destination and bootstrap options before cloning. Keep the clone if
/// synchronization fails.
pub(crate) fn run(
    invocation: &Invocation<'_>,
    url: &str,
    options: &BootstrapOptions,
    env: &Environment,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    // Validate bootstrap options before downloading anything.
    let bootstrap = BootstrapDecisions::read(options, env, invocation.reporter)?;

    let dest = &invocation.roots.batfiles_repo;
    // Even empty directories and broken symlinks count as occupied.
    if paths::occupied(dest)? {
        return Err(Error::CloneDestinationExists { path: dest.clone() });
    }

    git::clone_repository(url, dest)?;
    invocation.reporter.info(&format!(
        "{} {} from {url}",
        Verb::Clone.for_mode(RunMode::Perform),
        dest.display()
    ));

    // Report a missing manifest as a clone error before entering synchronization.
    if !paths::occupied(&invocation.roots.manifest_path())? {
        return Err(Error::ClonedWithoutManifest { path: dest.clone() });
    }

    execute::bootstrap(invocation, &bootstrap, skip_actions, skip_groups)
}
