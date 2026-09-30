//! `create-dir`: ensure a destination directory exists.

use super::RunContext;
use crate::error::Error;
use crate::manifest::action::CreateDirAction;

/// Create the destination directory if needed, following the run's conflict policy.
pub(super) fn create_dir(action: &CreateDirAction, context: &RunContext) -> Result<(), Error> {
    // A skipped conflict has been reported, and there is nothing else to do.
    context.ensure_directory(&context.destination(&action.dest))?;
    Ok(())
}
