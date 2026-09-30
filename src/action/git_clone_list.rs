//! Execute a validated clone list in order, warning on recoverable entry failures.

use super::RunContext;
use crate::clone_list::PreparedList;
use crate::error::Error;
use crate::git::{self, GitError};

/// Create the destination directory and process entries in list order. `list`
/// was read during preparation; an empty one declares no repositories.
pub(super) fn git_clone_list(list: &PreparedList<'_>, context: &RunContext) -> Result<(), Error> {
    let dest_dir = context.destination(list.dest_dir());
    if !context.ensure_directory(&dest_dir)? {
        return Ok(());
    }

    let name = list.name();

    if list.is_empty() {
        context
            .reporter()
            .detail(1, &format!("no repositories to clone in {name}"));
    }
    for entry in list.entries() {
        let declared = &entry.declared;
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
            &context.resolver(),
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

/// Return whether an entry failure allows processing to continue. Git launch and filesystem
/// failures stop the run.
fn is_recoverable_entry_error(error: &Error) -> bool {
    match error {
        Error::DestinationExists { .. } => true,
        // Preserve the original failure's recoverability even if restoring the old node failed.
        Error::SetAside { source, .. } => is_recoverable_entry_error(source),
        Error::Git(failure) => match failure {
            GitError::Failed { .. }
            | GitError::NotAClone { .. }
            | GitError::CloneElsewhere { .. }
            | GitError::CloneIncomplete { .. }
            | GitError::RefUnresolvable { .. } => true,
            // A Git launch failure also prevents later entries from running.
            GitError::Unavailable { .. } => false,
        },
        _ => false,
    }
}
