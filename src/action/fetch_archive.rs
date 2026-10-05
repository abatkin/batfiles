//! `fetch-archive`: seed a destination with a downloaded archive's contents.

use super::RunContext;
use crate::archive;
use crate::entry_filter::EntryFilter;
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
    let mut filter = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref());
    let mut extracted = false;
    install::seed_directory(
        install::SeedDescription {
            verb: Verb::Extract,
            origin: action.source.clone(),
            source: None,
        },
        &dest,
        context,
        |staging| {
            install::with_scratch(&dest, context.reporter(), |scratch| {
                let at = scratch.path().to_path_buf();
                fetch::download(&action.source, action.sha256.as_deref(), scratch, &at)?;
                extracted = true;
                archive::extract(
                    scratch.file(),
                    staging,
                    action.archive_root.as_deref(),
                    filter.as_mut(),
                    &action.source,
                )
            })
        },
    )?;
    if let Some(filter) = filter.as_ref().filter(|_| extracted) {
        filter.report_unmatched(&action.source, context.reporter());
    }
    Ok(())
}
