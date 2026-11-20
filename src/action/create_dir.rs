//! `create-dir`: ensure a destination directory exists.

use crate::error::Error;
use crate::manifest::actions::CreateDirAction;
use crate::manifest::check::ValueRule;
use crate::manifest::template::Resolver;
use crate::run::context::RunContext;

/// Create the destination directory if needed, following the run's conflict policy. Return
/// whether it created the directory.
pub(super) fn create_dir(
    action: &CreateDirAction,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    let dest = fields.field("dest", &action.dest, ValueRule::Destination)?;
    // A skipped conflict has been reported, and there is nothing else to do.
    Ok(context
        .ensure_directory(&context.destination(&dest))?
        .created())
}
