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

/// Execute every action in declaration order, stopping at the first failure.
pub(crate) fn run(roots: &Roots, manifest: &Manifest, reporter: &Reporter) -> Result<(), Error> {
    let context = Context::new(roots, reporter)?;
    for entry in &manifest.actions {
        action::run(entry, &context)?;
    }
    Ok(())
}
