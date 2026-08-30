//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Everything an action does is [`crate::action`]'s; this
//! is the list, and the one place anything iterates it (`guidance.md`, "Seams
//! the late slices need"). Which of its entries are carried out is
//! [`crate::selection`]'s.

use crate::action::{self, RunContext};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::mode::RunMode;
use crate::output::Reporter;
use crate::selection::Selection;

/// Read the leaf manifest and execute the actions it selects, in declaration
/// order, stopping at the first failure.
///
/// The manifest is read and checked whole before any of it is acted on, so a
/// repository whose manifest is missing, malformed, or invalid fails with the
/// file named rather than partway through. That is a property of syncing rather
/// than of dispatch, which is why reading it is here and not something the
/// caller arranges beforehand. The machine-local disabled lists are read the
/// same way and for the same reason.
///
/// `mode` is carried to the actions rather than consulted here: a dry run is
/// the same loop over the same list, and it passes over the same entries.
pub(crate) fn run(
    roots: &Roots,
    mode: RunMode,
    skip_actions: &[String],
    skip_groups: &[String],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let disabled = Disabled::load(&roots.disabled())?;
    let selection = Selection::new(skip_actions, skip_groups, env, disabled, reporter);
    // A complaint about the invocation, so it comes before the work rather than
    // after a failure that would swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);

    let context = RunContext::new(roots, mode, reporter)?;
    for (index, entry) in manifest.actions.iter().enumerate() {
        // Which record is speaking, ahead of what it says — and, where the
        // record is not going to speak at all, why. The user asked for the skip,
        // so restating it at normal verbosity would be noise; `-v` is where the
        // whole account of a run lives.
        let heading = entry.describe(index + 1);
        match selection.skipped(entry) {
            Some(why) => reporter.detail(1, &format!("{heading} - skipped: {why}")),
            None => {
                reporter.detail(1, &heading);
                action::run(entry, &context)?;
            }
        }
    }
    Ok(())
}
