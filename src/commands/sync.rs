//! `sync`: bring the home directory to the state the whole manifest describes,
//! first adopting the bootstrap policy under `--bootstrap`.

use crate::cli::SyncArgs;
use crate::commands::Session;
use crate::error::Error;
use crate::run::{self, bootstrap::BootstrapDecisions};

/// Validate bootstrap options before resolving anything, then run the pipeline
/// under the run lock.
pub(crate) fn run(args: &SyncArgs, session: &Session<'_>) -> Result<(), Error> {
    let bootstrap = if args.bootstrap {
        Some(BootstrapDecisions::read(
            &args.bootstrap_options,
            session.env,
            session.reporter,
        )?)
    } else {
        None
    };
    let (roots, _lock) = session.locked_run_roots()?;
    run::sync(
        &session.invocation(&roots, args.dry_run, &args.execution),
        &args.skips.actions.skip_actions,
        &args.skips.groups.skip_groups,
        args.refresh_remotes,
        bootstrap.as_ref(),
    )
}
