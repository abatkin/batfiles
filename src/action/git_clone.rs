//! `git-clone`: clone or update a repository at a destination.

use crate::error::Error;
use crate::manifest::actions::GitCloneAction;
use crate::manifest::check::ValueRule;
use crate::manifest::template::Resolver;
use crate::run::context::RunContext;
use crate::source::git;

/// Carry out one `git-clone` action. Return whether it cloned or moved the checkout.
pub(super) fn git_clone(
    action: &GitCloneAction,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    let source = fields.field("source", &action.source, ValueRule::GitSource)?;
    let git_ref = fields.optional("ref", action.git_ref.as_ref(), ValueRule::GitRef)?;
    let dest = context.destination(&fields.field("dest", &action.dest, ValueRule::Destination)?);
    git::clone_or_update(
        &source,
        &dest,
        git_ref.as_deref(),
        context.repository(),
        &context.conflict_resolver(),
    )
}
