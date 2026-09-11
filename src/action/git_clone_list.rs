//! Execute a validated clone list in order, warning on recoverable entry failures.

use super::RunContext;
use crate::condition::Exclusion;
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

    // Preparation walks the same captured selection this run executes, ahead of
    // every action, and passes over only the lists an exclusion has already
    // closed — which are not executed either. A list arriving here unread is
    // that invariant broken rather than anything a manifest can ask for.
    let entries = action
        .entries
        .as_ref()
        .expect("an executable clone list is prepared before any action runs");

    if entries.is_empty() {
        context
            .reporter()
            .detail(1, &format!("no repositories to clone in {}", action.source));
    }
    for entry in entries {
        // Reported here rather than where preparation settled it, so the line
        // sits under the heading naming the action that holds the list. The
        // wording is the failure warning's below, since both say that one entry
        // of a list is not being cloned and why.
        if let Some(exclusion) = &entry.exclusion {
            let line = format!(
                "not cloning {} ({}): {}",
                entry.repository,
                entry.written_at(&action.source),
                exclusion.reason()
            );
            // One line, two severities: an entry this machine's variables close
            // is the list working as written, and one whose condition batfiles
            // could not decide is not, so the reader hears about it whether or
            // not the run asked for detail.
            match exclusion {
                Exclusion::Expected(_) => context.reporter().detail(1, &line),
                Exclusion::EvaluationFailed(_) => context.reporter().warn(&line),
            }
            continue;
        }

        let dest = dest_dir.join(&entry.dest_name);
        match git::clone_or_update(
            &entry.repository,
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
                    entry.repository,
                    entry.written_at(&action.source)
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
