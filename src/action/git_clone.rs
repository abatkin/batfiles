//! `git-clone`: clone or update a repository at a destination.

use crate::error::Error;
use crate::manifest::action::GitCloneAction;
use crate::run::context::RunContext;
use crate::source::git;

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
