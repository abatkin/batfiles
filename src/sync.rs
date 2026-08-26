//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Everything an action does is [`crate::action`]'s; this
//! is the list, and the one place anything iterates it (`guidance.md`, "Seams
//! the late slices need").

use crate::action::{self, Context};
use crate::error::Error;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::output::Reporter;

/// Read the leaf manifest and execute every action in it, in declaration order,
/// stopping at the first failure.
///
/// The manifest is read and checked whole before any of it is acted on, so a
/// repository whose manifest is missing, malformed, or invalid fails with the
/// file named rather than partway through. That is a property of syncing rather
/// than of dispatch, which is why reading it is here and not something the
/// caller arranges beforehand.
pub(crate) fn run(roots: &Roots, reporter: &Reporter) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let context = Context::new(roots, reporter)?;
    for entry in &manifest.actions {
        action::run(entry, &context)?;
    }
    Ok(())
}
