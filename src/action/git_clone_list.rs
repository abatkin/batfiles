//! `git-clone-list`: every repository a list names, cloned under one directory.
//!
//! The list itself has already been read and checked by the time a run reaches
//! this — before the first action wrote anything, which is what a repository
//! file lets batfiles do (`crate::execute`). What is left for this module is the
//! cloning, and that is 4.5's.

use super::RunContext;
use crate::error::Error;
use crate::manifest::action::GitCloneListAction;

/// Say that one `git-clone-list` was not carried out, and let the run continue.
///
/// A warning rather than a failure, on the terms [`crate::git`]'s skips already
/// use: the user asked for repositories that are not there, which is worth
/// knowing at a volume nothing hides, and stopping the run would strand every
/// action after this one over a plugin directory. What it costs is that a
/// successful `sync` does not yet mean the home matches the whole manifest, and
/// that is why this is a warning and not a note.
///
/// It is never silence. The record is accepted and its list is checked, so a
/// repository and its list can be written and corrected today; passing over the
/// action without a word is the one thing this must not do.
// CARRY(4.5): replace this with the loop over the entries `clone_list::read`
// already returns, cloning each through `git::clone_or_update`.
pub(super) fn git_clone_list(
    action: &GitCloneListAction,
    context: &RunContext,
) -> Result<(), Error> {
    context.reporter().warn(&format!(
        "not cloning into {}: git-clone-list is read and checked, and cloning arrives at step 4.5",
        context.destination(&action.dest_dir).display()
    ));
    Ok(())
}
