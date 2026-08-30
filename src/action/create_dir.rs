//! `create-dir`: one directory, made where nothing is.

use super::RunContext;
use crate::error::Error;
use crate::manifest::action::CreateDirAction;

/// Carry out one `create-dir` action: the whole of it is one directory.
///
/// No source, and no platform check — every platform batfiles builds for makes
/// directories. What is at the destination is
/// [`crate::directory::ensure_directory`]'s question rather than
/// [`crate::paths::Occupancy::at`]'s: this action replaces nothing, so a
/// directory already there, however it is reached, is what was asked for.
pub(super) fn create_dir(action: &CreateDirAction, context: &RunContext) -> Result<(), Error> {
    context.ensure_directory(&context.destination(&action.dest))
}
