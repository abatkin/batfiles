//! `apply-action` and `apply-group`: run the pipeline for one record or one
//! group, using the remotes already materialized.

use crate::cli::{ApplyActionArgs, ApplyGroupArgs};
use crate::commands::Session;
use crate::error::Error;
use crate::run;

/// `apply-action`, under the run lock.
pub(crate) fn action(args: &ApplyActionArgs, session: &Session<'_>) -> Result<(), Error> {
    let (roots, _lock) = session.locked_run_roots()?;
    run::apply_action(
        &session.invocation(&roots, args.dry_run, &args.execution),
        &args.id,
    )
}

/// `apply-group`, under the run lock.
pub(crate) fn group(args: &ApplyGroupArgs, session: &Session<'_>) -> Result<(), Error> {
    let (roots, _lock) = session.locked_run_roots()?;
    run::apply_group(
        &session.invocation(&roots, args.dry_run, &args.execution),
        &args.group,
        &args.skips.skip_actions,
    )
}
