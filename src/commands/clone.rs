//! Clone a leaf repository into a vacant destination, then synchronize it with bootstrap
//! disabled-state adoption.

use crate::cli::CloneArgs;
use crate::commands::Session;
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::filesystem::paths;
use crate::output::Verb;
use crate::run;
use crate::run::bootstrap::BootstrapDecisions;
use crate::run::lock::RunLock;
use crate::source::git;

/// Clone the URL into the selected leaf repository, following `--ref` where one is given, and
/// synchronize it.
///
/// Validate the destination and bootstrap options before cloning. Take the run lock after
/// cloning and before reading any state. Keep the clone if the ref cannot be followed, the lock
/// is held, or synchronization fails.
pub(crate) fn run(args: &CloneArgs, session: &Session<'_>) -> Result<(), Error> {
    let roots = session.new_run_roots()?;
    let invocation = session.invocation(&roots, false, &args.execution);
    let url = args.url.as_str();
    let git_ref = args.git_ref.as_deref();

    // Validate bootstrap options before downloading anything.
    let bootstrap = BootstrapDecisions::read(&args.bootstrap, session.env, session.reporter)?;

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
        git::ref_suffix(git_ref)
    ));

    // Report a missing manifest as a clone error before entering synchronization.
    if !paths::occupied(&invocation.roots.manifest_path())? {
        return Err(Error::ClonedWithoutManifest { path: dest.clone() });
    }

    let _lock = RunLock::acquire(&invocation.roots.machine.cache_dir)?;
    run::sync(
        &invocation,
        &args.skips.actions.skip_actions,
        &args.skips.groups.skip_groups,
        false,
        Some(&bootstrap),
    )
}
