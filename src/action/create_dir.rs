//! `create-dir`: one directory, made where nothing is.

use super::RunContext;
use crate::error::Error;
use crate::manifest::action::CreateDirAction;

/// Carry out one `create-dir` action: the whole of it is one directory.
pub(super) fn create_dir(action: &CreateDirAction, context: &RunContext) -> Result<(), Error> {
    // A skipped conflict has been reported, and there is nothing else to do.
    context.ensure_directory(&context.destination(&action.dest))?;
    Ok(())
}
