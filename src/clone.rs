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
use crate::run_lock::RunLock;

/// Clone `url` into the selected leaf repository, following `git_ref` where one is given, and
/// synchronize it.
///
/// Validate the destination and bootstrap options before cloning. Take the run lock after
/// cloning and before reading any state. Keep the clone if the ref cannot be followed, the lock
/// is held, or synchronization fails.
pub(crate) fn run(
    invocation: &Invocation<'_>,
    url: &str,
    git_ref: Option<&str>,
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

    git::clone_repository(url, dest, git_ref, invocation.reporter)?;
    invocation.reporter.info(&format!(
        "{} {} from {url}{}",
        Verb::Clone.for_mode(RunMode::Perform),
        dest.display(),
        git::at(git_ref)
    ));

    // Report a missing manifest as a clone error before entering synchronization.
    if !paths::occupied(&invocation.roots.manifest_path())? {
        return Err(Error::ClonedWithoutManifest { path: dest.clone() });
    }

    let _lock = RunLock::acquire(&invocation.roots.state.cache_dir)?;
    execute::sync(
        invocation,
        skip_actions,
        skip_groups,
        false,
        Some(&bootstrap),
    )
}
