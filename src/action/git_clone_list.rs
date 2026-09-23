//! Execute a validated clone list in order, warning on recoverable entry failures.

use super::RunContext;
use crate::clone_list::PreparedList;
use crate::error::Error;
use crate::git::{self, Failure};

/// Create the destination directory and process entries in list order. `list`
/// was read during preparation; an empty one declares no repositories.
pub(super) fn git_clone_list(list: &PreparedList<'_>, context: &RunContext) -> Result<(), Error> {
    let dest_dir = context.destination(list.dest_dir());
    context.ensure_directory(&dest_dir)?;

    // How the list is named in every line below.
    let name = list.name();

    if list.is_empty() {
        context
            .reporter()
            .detail(1, &format!("no repositories to clone in {name}"));
    }
    for entry in list.entries() {
        let declared = &entry.declared;
        // Reported here, under the action's heading, worded like the failure
        // warning below.
        if let Some(exclusion) = &entry.exclusion {
            let line = format!(
                "not cloning {} ({}): {}",
                declared.repository,
                declared.written_at(&name),
                exclusion.reason()
            );
            exclusion.report(context.reporter(), &line);
            continue;
        }

        let dest = dest_dir.join(&declared.dest_name);
        match git::clone_or_update(
            &declared.repository,
            &dest,
            declared.git_ref.as_deref(),
            context.repository(),
            context.mode(),
            context.reporter(),
        ) {
            Ok(()) => {}
            Err(failure) if is_recoverable_entry_error(&failure) => {
                context.reporter().warn(&format!(
                    "not cloning {} ({}): {failure}",
                    declared.repository,
                    declared.written_at(&name)
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
