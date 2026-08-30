//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Everything an action does is [`crate::action`]'s; this
//! is the list, and the one place anything iterates it (`guidance.md`, "Seams
//! the late slices need").

use crate::action::{self, RunContext};
use crate::error::Error;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::mode::RunMode;
use crate::output::Reporter;

/// Read the leaf manifest and execute every action in it, in declaration order,
/// stopping at the first failure.
///
/// The manifest is read and checked whole before any of it is acted on, so a
/// repository whose manifest is missing, malformed, or invalid fails with the
/// file named rather than partway through. That is a property of syncing rather
/// than of dispatch, which is why reading it is here and not something the
/// caller arranges beforehand.
///
/// `mode` is carried to the actions rather than consulted here: a dry run is
/// the same loop over the same list.
pub(crate) fn run(roots: &Roots, mode: RunMode, reporter: &Reporter) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let context = RunContext::new(roots, mode, reporter)?;
    for entry in &manifest.actions {
        action::run(entry, &context)?;
    }
    Ok(())
}
