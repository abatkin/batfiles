//! `fetch-archive`: one archive downloaded and unpacked where nothing is.

use std::path::Path;

use super::RunContext;
use crate::archive;
use crate::error::Error;
use crate::fetch;
use crate::install::{self, FileOrDirectory};
use crate::manifest::action::FetchArchiveAction;
use crate::output::Verb;

/// Carry out one `fetch-archive` action.
pub(super) fn fetch_archive(
    action: &FetchArchiveAction,
    context: &RunContext,
) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    install::seed(
        install::Seed {
            kind: FileOrDirectory::Directory,
            verb: Verb::Extract,
            origin: action.source.clone(),
            // A download has nothing on this machine for a destination to be
            // inside of.
            source_directory: None,
            // Both parameters are annotated because the closure lives in a
            // struct field, where inference has nothing else to read them from.
            fill: |_: install::Staged, staging: &Path| {
                install::with_scratch(&dest, context.reporter(), |scratch| {
                    let at = scratch.path().to_path_buf();
                    fetch::download(&action.source, action.sha256.as_deref(), scratch, &at)?;
                    archive::extract(
                        scratch.file(),
                        staging,
                        action.archive_root.as_deref(),
                        &action.source,
                    )
                })
            },
        },
        &dest,
        context.mode(),
        context.reporter(),
    )
}
