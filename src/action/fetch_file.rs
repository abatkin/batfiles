//! `fetch-file`: seed a destination with a downloaded file.

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
        install::SeedDescription {
            verb: Verb::Fetch,
            origin: action.source.clone(),
            source: None,
        },
        &dest,
        context,
        |file, staging| {
            let source = fetch::FileSource {
                url: &action.source,
                sha256: action.sha256.as_deref(),
                executable: action.executable,
                decompress: action.decompress,
            };
            fetch::fetch_file(&source, file, staging, &dest, context.reporter())
        },
    )
}
