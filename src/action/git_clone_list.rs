//! Execute a validated clone list in order, warning on recoverable entry failures.

use super::RunContext;
use crate::clone_list::Entry;
use crate::error::Error;
use crate::git::{self, Failure};
use crate::manifest::action::GitCloneListAction;

/// Create the destination directory and process entries in list order.
/// Requires entries populated by execution preparation, including an empty list.
pub(super) fn git_clone_list(
    action: &GitCloneListAction,
    context: &RunContext,
) -> Result<(), Error> {
    let dest_dir = context.destination(&action.dest_dir);
    context.ensure_directory(&dest_dir)?;

    let entries = action
        .entries
        .as_ref()
        .expect("executable clone lists are populated during preparation");

    if entries.is_empty() {
        context
            .reporter()
            .detail(1, &format!("no repositories to clone in {}", action.source));
    }
    for entry in entries {
        let dest = dest_dir.join(&entry.dest_name);
        match git::clone_or_update(
            &entry.url,
            &dest,
            entry.git_ref.as_deref(),
            context.repository(),
            context.mode(),
            context.reporter(),
        ) {
            Ok(()) => {}
            Err(failure) if is_recoverable_entry_error(&failure) => {
                context.reporter().warn(&format!(
                    "not cloning {} ({}): {failure}",
                    entry.url,
                    declared(action, entry)
                ))
            }
            Err(failure) => return Err(failure),
        }
    }
    Ok(())
}

/// Return whether a failed entry may be skipped while processing the remaining list.
/// Git launch and filesystem failures stop the run. The Git match is exhaustive.
fn is_recoverable_entry_error(error: &Error) -> bool {
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
fn declared(action: &GitCloneListAction, entry: &Entry) -> String {
    match &entry.id {
        Some(id) => format!("id={id}, {} line {}", action.source, entry.line),
        None => format!("{} line {}", action.source, entry.line),
    }
}
