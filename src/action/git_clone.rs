//! `git-clone`: one repository, cloned where nothing is and updated where it
//! already is.
//!
//! The whole of the action is resolving its destination: what to do at one is
//! [`crate::git`]'s, because `git-clone-list` and 6.2's remotes want the same
//! clone-or-update decision and none of the three should re-derive it.
//!
//! This is the one action that does not publish through [`crate::install`].
//! A seed declines an occupied destination by design, and an existing clone is
//! an occupied destination that this action has work to do at — so the mode is
//! read by the git helper instead, which is why that helper is the fourth entry
//! in `guidance.md`'s table and the only one slice 4 adds.

use super::RunContext;
use crate::error::Error;
use crate::git;
use crate::manifest::action::GitCloneAction;

/// Carry out one `git-clone` action.
///
/// Nothing here asks about the mode: under `--dry-run` the helper runs no git
/// at all, so neither the clone nor the fetch is merely skipped — both are
/// unreachable, and the network is not touched.
pub(super) fn git_clone(action: &GitCloneAction, context: &RunContext) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    git::clone_or_update(
        &action.source,
        &dest,
        action.git_ref.as_deref(),
        // For classifying what is at the destination, which is the same
        // question every other action asks and gets the same answer to: a
        // symlink resolving into the repository is batfiles' to replace, and
        // one resolving out of it is not.
        context.repository(),
        context.mode(),
        context.reporter(),
    )
}
