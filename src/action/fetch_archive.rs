//! `fetch-archive`: one archive downloaded and unpacked where nothing is.
//!
//! The same seed `copy` and `fetch-file` make, filled from an archive instead of
//! from a file. Everything that makes an install safe — the missing-only check,
//! the staging node, the single move into place — is [`install::seed`]'s and is
//! reached by the identical route.
//!
//! The one thing this action needs that the others do not is a place to put the
//! archive: what fills the staging directory is the archive's *contents*, so the
//! archive itself has to arrive somewhere else first, be checked against its
//! digest, and only then be unpacked. That place is [`install::with_scratch`]'s,
//! and it is a sibling of the destination like the staging node beside it.

use std::path::Path;

use super::RunContext;
use crate::archive;
use crate::error::Error;
use crate::fetch;
use crate::install::{self, FileOrDirectory};
use crate::manifest::action::FetchArchiveAction;
use crate::output::Verb;

/// Carry out one `fetch-archive` action.
///
/// Nothing here asks about the mode. Under `--dry-run` `seed` reports what it
/// would extract and creates no staging node, so neither the download nor the
/// extraction is merely skipped — both are unreachable, and no request is made.
///
/// This is the first seed whose `kind` is a directory and whose filler unpacks
/// rather than writes, which is what makes both arms of `install::Staged`
/// reachable by construction.
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
            not_inside: None,
            // Both parameters are annotated because the closure lives in a
            // struct field, where inference has nothing else to read them from.
            fill: |_: install::Staged, staging: &Path| {
                install::with_scratch(&dest, context.reporter(), |scratch| {
                    let at = scratch.path().to_path_buf();
                    // Whole and verified before a single entry is unpacked: a
                    // digest that does not match means the archive is never
                    // read, let alone written out.
                    fetch::download(&action.source, action.sha256.as_deref(), scratch, &at)?;
                    // Unpacked from the handle it was written and hashed
                    // through, so the bytes the digest passed are the bytes that
                    // come out.
                    archive::extract(
                        scratch.written(),
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
