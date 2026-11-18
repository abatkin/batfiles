//! `fetch-file`: seed a destination with a downloaded file.

use crate::error::Error;
use crate::filesystem::install;
use crate::manifest::actions::FetchFileAction;
use crate::manifest::check::ValueRule;
use crate::manifest::template::Resolver;
use crate::output::Verb;
use crate::run::context::RunContext;
use crate::source::fetch;

/// Carry out one `fetch-file` action.
pub(super) fn fetch_file(
    action: &FetchFileAction,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<(), Error> {
    let source = fields.field("source", &action.source, ValueRule::Url)?;
    let sha256 = fields.optional("sha256", action.sha256.as_ref(), ValueRule::Digest)?;
    let dest = context.destination(&fields.field("dest", &action.dest, ValueRule::Destination)?);
    install::seed_file(
        install::SeedDescription {
            verb: Verb::Fetch,
            origin: source.clone().into_owned(),
            source: None,
        },
        &dest,
        &context.seeding(),
        |file, staging| {
            let source = fetch::FileSource {
                url: &source,
                sha256: sha256.as_deref(),
                executable: action.executable,
                decompress: action.decompress,
            };
            fetch::fetch_file(&source, file, staging, &dest, context.reporter())
        },
    )
}
