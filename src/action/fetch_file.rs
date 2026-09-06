//! `fetch-file`: one file downloaded to a destination where nothing is.

use std::path::Path;

use super::RunContext;
use crate::error::Error;
use crate::fetch;
use crate::install::{self, FileOrDirectory};
use crate::manifest::action::FetchFileAction;
use crate::output::Verb;

/// Carry out one `fetch-file` action.
pub(super) fn fetch_file(action: &FetchFileAction, context: &RunContext) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    install::seed(
        install::Seed {
            kind: FileOrDirectory::File,
            verb: Verb::Fetch,
            origin: action.source.clone(),
            // A download has nothing on this machine for a destination to be
            // inside of.
            source_directory: None,
            // Both parameters are annotated because the closure lives in a
            // struct field, where inference has nothing else to read them from.
            fill: |staged: install::Staged, staging: &Path| {
                fetch::download_file(
                    &action.source,
                    action.sha256.as_deref(),
                    staged.into_file(),
                    staging,
                )
            },
        },
        &dest,
        context.mode(),
        context.reporter(),
    )
}
