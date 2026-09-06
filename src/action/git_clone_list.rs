//! `git-clone-list`: every repository a list names, cloned under one directory.
//!
//! The list itself has already been read and checked by the time a run reaches
//! this — before the first action wrote anything, which is what a repository
//! file lets batfiles do (`crate::execute`) — and what it read travels on the
//! record. Nothing here opens the file again: reading it twice would validate
//! one document and install from another.
//!
//! What a clone does, and what it refuses, is [`crate::git`]'s and is the same
//! here as for `git-clone`. What is this action's alone is the decision a list
//! has to make and one repository never did: whether a failure costs the run or
//! costs one entry.

use super::RunContext;
use crate::clone_list::Entry;
use crate::error::Error;
use crate::git::{self, Failure};
use crate::manifest::action::GitCloneListAction;

/// Carry out one `git-clone-list` action.
///
/// The directory comes first, so an empty list still leaves the place its
/// repositories would go — a plugin directory a shell reads is worth having
/// whether or not anything is in it yet. Entries then run in list order, each
/// one a child of it.
pub(super) fn git_clone_list(
    action: &GitCloneListAction,
    context: &RunContext,
) -> Result<(), Error> {
    let dest_dir = context.destination(&action.dest_dir);
    context.ensure_directory(&dest_dir)?;

    // Filled by the pass that read and checked the list, which asks the same
    // two questions of the same `Selection` that the execution loop asks: an
    // action that runs is an action that was filled. `None` here is a bug in
    // batfiles rather than anything a repository can write.
    let entries = action
        .entries
        .as_ref()
        .expect("a git-clone-list runs only where the pass that reads lists has filled it");

    if entries.is_empty() {
        context
            .reporter()
            .detail(1, &format!("no repositories to clone in {}", action.source));
    }
    for entry in entries {
        let dest = dest_dir.join(&entry.name);
        match git::clone_or_update(
            &entry.url,
            &dest,
            entry.git_ref.as_deref(),
            context.repository(),
            context.mode(),
            context.reporter(),
        ) {
            Ok(()) => {}
            // One repository the run could not have, out of a list of them. The
            // user is told and the rest are still installed, which is the whole
            // reason this action classifies rather than propagating.
            Err(failure) if survives(&failure) => context.reporter().warn(&format!(
                "not cloning {} ({}): {failure}",
                entry.url,
                declared(action, entry)
            )),
            Err(failure) => return Err(failure),
        }
    }
    Ok(())
}

/// Whether a failure costs this entry or the whole run.
///
/// Here rather than on either error type, because this is the only caller that
/// has to tell them apart, and because the answer spans the nest either way:
/// [`Error::DestinationExists`] is raised by three modules and belongs to none
/// of them.
///
/// **What is classified is the failures, and only those.** A dirty worktree, a
/// branch tracking nothing, and a history that has diverged never reach here —
/// [`crate::git`] warns about each and answers `Ok`.
///
/// The inner match has no wildcard on purpose: a git failure added later fails
/// to compile until someone says whether a list can carry on past it.
fn survives(error: &Error) -> bool {
    match error {
        // A destination this entry may not have: something is at it that
        // batfiles did not put there and will not replace.
        Error::DestinationExists { .. } => true,
        Error::Git(failure) => match failure {
            // Something wrong with one repository, or with one `git` that ran
            // and failed. The next entry is a different repository.
            Failure::Failed { .. }
            | Failure::NotAClone { .. }
            | Failure::CloneElsewhere { .. }
            | Failure::CloneIncomplete { .. }
            | Failure::RefUnresolvable { .. } => true,
            // No `git` at all, so no entry after this one could clone either.
            Failure::Unavailable { .. } => false,
        },
        // Reading or writing the machine, and anything else that is not about
        // this repository.
        _ => false,
    }
}

/// Where an entry is written, for the warning that has to send a reader to it.
///
/// The list as the manifest spells it and the line as the file counts it, which
/// together are what a reader opens and edits. The `id` is included where there
/// is one because that is the name the entry answers to.
fn declared(action: &GitCloneListAction, entry: &Entry) -> String {
    match &entry.id {
        Some(id) => format!("id={id}, {} line {}", action.source, entry.line),
        None => format!("{} line {}", action.source, entry.line),
    }
}
