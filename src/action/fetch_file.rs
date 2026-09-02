//! `fetch-file`: one file downloaded to a destination where nothing is.
//!
//! The same seed `copy` makes, filled from a URL instead of from the
//! repository. Everything that makes an install safe — the missing-only check,
//! the staging node, the single move into place — is [`install::seed`]'s and is
//! reached by the identical route; what is here is the description of this one
//! seed, and [`crate::fetch`] is what fills it.

use std::path::Path;

use super::RunContext;
use crate::error::Error;
use crate::fetch;
use crate::install::{self, FileOrDirectory};
use crate::manifest::action::FetchFileAction;
use crate::output::Verb;

/// Carry out one `fetch-file` action.
///
/// Nothing here asks about the mode. Under `--dry-run` `seed` reports what it
/// would fetch and creates no staging node, so the download is not merely
/// skipped — it is unreachable, and no request is made.
pub(super) fn fetch_file(action: &FetchFileAction, context: &RunContext) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    install::seed(
        install::Seed {
            kind: FileOrDirectory::File,
            verb: Verb::Fetch,
            origin: action.source.clone(),
            // A download has nothing on this machine for a destination to be
            // inside of.
            not_inside: None,
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
