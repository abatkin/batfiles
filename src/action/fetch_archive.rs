//! `fetch-archive`: one archive downloaded and unpacked where nothing is.

use super::RunContext;
use crate::archive;
use crate::error::Error;
use crate::fetch;
use crate::install;
use crate::manifest::action::FetchArchiveAction;
use crate::output::Verb;

/// Carry out one `fetch-archive` action.
pub(super) fn fetch_archive(
    action: &FetchArchiveAction,
    context: &RunContext,
) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    install::seed_directory(
        install::Seed {
            verb: Verb::Extract,
            origin: action.source.clone(),
            // A download has nothing on this machine for a destination to be
            // inside of.
            source_directory: None,
        },
        &dest,
        context.mode(),
        context.reporter(),
        |staging| {
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
    )
}
