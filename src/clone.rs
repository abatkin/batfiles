//! `clone`: put a leaf repository on a machine that has none, and synchronize
//! it.
//!
//! The command is `sync` with a download in front of it. Once the repository is
//! on the machine the run that follows is the one `sync` performs, from the same
//! roots, with the same selection, variables, and reporting, so nothing here
//! decides anything about installation.
//!
//! What it does decide is the destination, and it is the only repository command
//! that does not discover one: [`crate::app`] resolves the roots without
//! working-directory discovery, because discovery names a directory that already
//! holds a manifest and this command requires one that holds nothing.
//!
//! CARRY(8.3): the leaf's own `[default-disabled]` candidates are not adopted
//! yet, so the synchronization below installs them like any other action.

use crate::error::Error;
use crate::execute::{self, Invocation};
use crate::git;
use crate::mode::RunMode;
use crate::output::Verb;
use crate::paths;

/// Clone `url` into the selected leaf repository and synchronize it.
///
/// The destination must hold nothing at all, and that is checked before Git is
/// launched. Afterwards nothing is unwound: a repository that arrived is kept
/// whether or not the synchronization that follows succeeds, so the fix for a
/// manifest this machine cannot carry out is an edit and a `sync` rather than a
/// second download.
pub(crate) fn run(
    invocation: &Invocation<'_>,
    url: &str,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    let dest = &invocation.roots.batfiles_dir;
    // Presence, not kind: `clone` creates the directory it clones into, so
    // anything at all there -- an empty directory, a file, a link to nothing --
    // is something batfiles did not put there and will not write over.
    if paths::occupied(dest)? {
        return Err(Error::CloneDestinationExists { path: dest.clone() });
    }

    git::clone_repository(url, dest)?;
    invocation.reporter.info(&format!(
        "{} {} from {url}",
        // The mode is the constant it is for the whole command: `clone` accepts
        // no `--dry-run`, since a run with nothing to read cannot describe a
        // plan.
        Verb::Clone.say(RunMode::Perform),
        dest.display()
    ));

    // A Git repository without a manifest is a wrong URL rather than a missing
    // file, and the synchronization below would report it the other way around.
    if !paths::occupied(&invocation.roots.manifest())? {
        return Err(Error::ClonedWithoutManifest { path: dest.clone() });
    }

    execute::sync(invocation, skip_actions, skip_groups)
}
