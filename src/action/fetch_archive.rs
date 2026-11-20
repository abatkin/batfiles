//! `fetch-archive`: seed a destination with a downloaded archive's contents.

use crate::error::Error;
use crate::filesystem::install;
use crate::manifest::actions::FetchArchiveAction;
use crate::manifest::check::ValueRule;
use crate::manifest::entry_filter::{EntryFilter, ExecutableFilter};
use crate::manifest::template::Resolver;
use crate::output::Verb;
use crate::run::context::RunContext;
use crate::source::archive;
use crate::source::fetch;

/// Carry out one `fetch-archive` action. Return whether it installed or replaced the contents.
pub(super) fn fetch_archive(
    action: &FetchArchiveAction,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    let source = fields.field("source", &action.source, ValueRule::Url)?;
    let sha256 = fields.optional("sha256", action.sha256.as_ref(), ValueRule::Digest)?;
    let dest = context.destination(&fields.field("dest", &action.dest, ValueRule::Destination)?);
    let mut filter = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref());
    let mut executable = ExecutableFilter::new(action.executable.as_ref());
    let mut extracted = false;
    let changed = install::seed_directory(
        install::SeedDescription {
            verb: Verb::Extract,
            origin: source.clone().into_owned(),
            source: None,
        },
        &dest,
        &context.seeding(),
        |staging| {
            install::with_scratch(&dest, context.reporter(), |scratch| {
                let at = scratch.path().to_path_buf();
                fetch::download(&source, sha256.as_deref(), scratch, &at)?;
                extracted = true;
                archive::extract(
                    scratch.file(),
                    staging,
                    action.archive_root.as_deref(),
                    filter.as_mut(),
                    executable.as_mut(),
                    &source,
                )
            })
        },
    )?;
    if extracted {
        if let Some(filter) = &filter {
            filter.report_unmatched(&source, context.reporter());
        }
        if let Some(executable) = &executable {
            executable.report_unmatched(&source, context.reporter());
        }
    }
    Ok(changed)
}
