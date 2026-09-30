//! `git-clone`: one repository, cloned where nothing is and updated where it
//! already is.

use super::RunContext;
use crate::error::Error;
use crate::git;
use crate::manifest::action::GitCloneAction;

/// Carry out one `git-clone` action.
pub(super) fn git_clone(action: &GitCloneAction, context: &RunContext) -> Result<(), Error> {
    let dest = context.destination(&action.dest);
    git::clone_or_update(
        &action.source,
        &dest,
        action.git_ref.as_deref(),
        context.repository(),
        &context.resolver(),
    )
}
