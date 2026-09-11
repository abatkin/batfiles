//! `fetch-file`: one file downloaded to a destination where nothing is.

use super::RunContext;
use crate::error::Error;
use crate::fetch;
use crate::install;
use crate::manifest::action::FetchFileAction;
use crate::output::Verb;

/// Carry out one `fetch-file` action.
pub(super) fn fetch_file(action: &FetchFileAction, context: &RunContext) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    install::seed_file(
        install::Seed {
            verb: Verb::Fetch,
            origin: action.source.clone(),
            // A download has nothing on this machine for a destination to be
            // inside of.
            source_directory: None,
        },
        &dest,
        context.mode(),
        context.reporter(),
        |file, staging| {
            fetch::download_file(&action.source, action.sha256.as_deref(), file, staging)
        },
    )
}
