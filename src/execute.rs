//! Carrying out a repository's actions: `sync`, `apply-action`, and
//! `apply-group`.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Everything an action does is [`crate::action`]'s, and
//! which actions a run touches — the records it asked for, less the ones it
//! passes over — is [`crate::selection`]'s. What is left here is the list, and
//! the one place anything iterates it (`guidance.md`, "Seams the late slices
//! need"). The three commands differ only in the [`Selection`] they hand it.

use crate::action::{self, RunContext};
use crate::clone_list;
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::ItemAddress;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::Action;
use crate::mode::RunMode;
use crate::output::Reporter;
use crate::selection::{Selection, Target};

/// `sync`: bring the home directory to the state the whole manifest describes.
pub(crate) fn sync(
    roots: &Roots,
    mode: RunMode,
    skip_actions: &[String],
    skip_groups: &[String],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let (manifest, disabled) = load(roots)?;
    let selection = Selection::new(
        Target::Everything,
        skip_actions,
        skip_groups,
        env,
        disabled,
        reporter,
    );
    // An empty manifest, and one whose every action is disabled, are both
    // ordinary successful runs that did nothing, so the count is not consulted.
    run(&manifest, &selection, roots, mode, reporter)?;
    Ok(())
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(
    roots: &Roots,
    mode: RunMode,
    id: &str,
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Checked before the repository is opened, so a name that could never have
    // matched is reported as the malformed name it is rather than as a
    // resolution failure.
    let id = ItemAddress::try_from(id.to_owned())?;
    let (manifest, disabled) = load(roots)?;
    // The command accepts neither run-only option, and naming one action waives
    // every exclusion either document holds.
    let selection = Selection::new(Target::Action(&id), &[], &[], env, disabled, reporter);
    // Nothing filters this command, so a record it named was carried out, and a
    // name no record carries has already failed as unresolved.
    run(&manifest, &selection, roots, mode, reporter)?;
    Ok(())
}

/// `apply-group`: carry out the records naming `group` that are not themselves
/// disabled or skipped.
pub(crate) fn apply_group(
    roots: &Roots,
    mode: RunMode,
    group: &str,
    skip_actions: &[String],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let group = ItemAddress::try_from(group.to_owned())?;
    let (manifest, disabled) = load(roots)?;
    // `--skip-group` is not accepted, so there is no group-shaped run-only list
    // to hand over; naming the group waives the one there would have been.
    let selection = Selection::new(
        Target::Group(&group),
        skip_actions,
        &[],
        env,
        disabled,
        reporter,
    );
    let carried_out = run(&manifest, &selection, roots, mode, reporter)?;

    // The group exists — a name no record names has already failed as
    // unresolved — and every member of it was passed over, which at normal
    // verbosity would otherwise be silence in answer to a command that named
    // one thing. `-v` has already said which record and why.
    if carried_out == 0 {
        reporter.info("nothing to apply: every action in the group is disabled or skipped");
    }
    Ok(())
}

/// Read the two documents every one of these commands works from.
///
/// The manifest is read and checked whole before any of it is acted on, so a
/// repository whose manifest is missing, malformed, or invalid fails with the
/// file named rather than partway through. The machine-local disabled lists are
/// read the same way and for the same reason. That is a property of executing
/// actions rather than of dispatch, which is why reading them is here and not
/// something the caller arranges beforehand.
///
/// All three commands load both, so one rule covers them: a state file that
/// cannot be read fails any command that executes actions. Which of the
/// disabled lists a run then consults is [`Selection`]'s, and follows from what
/// the command named.
fn load(roots: &Roots) -> Result<(Manifest, Disabled), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let disabled = Disabled::load(&roots.disabled())?;
    Ok((manifest, disabled))
}

/// Read and check every clone list this run would install from, before the
/// first action writes anything.
///
/// A `git-clone-list` names a file in the repository, which is on disk and
/// readable now, so the rule the manifest itself follows extends to it: a
/// document a run cannot make sense of stops the run before it has done any
/// work, rather than partway through. That is what a list buys over the
/// repositories being spread across the manifest — one malformed line is caught
/// while the home is still untouched.
///
/// It costs one thing, worth saying out loud: a list produced by an earlier
/// action in the same run is not a list this can read. Both repositories this
/// exists for keep theirs in git, which is the case the format is for.
///
/// Only the records the run would carry out. A list belonging to a disabled or
/// skipped action is not read, because a run that never reaches an action must
/// not be failed by it — the same reason its `source` is not required to exist.
fn read_clone_lists(
    manifest: &Manifest,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
) -> Result<(), Error> {
    for action in &manifest.actions {
        if let Action::GitCloneList(list) = action
            && selection.wants(action)
            && selection.skipped(action).is_none()
        {
            clone_list::read(&context.source(&list.source)?)?;
        }
    }
    Ok(())
}

/// The one action-execution loop: everything the target asked for, in
/// declaration order, stopping at the first failure. Answers with how many
/// records it carried out.
///
/// A target that resolved to nothing has already failed by the time this
/// returns, so the count that survives is the one only the caller can read: a
/// command that matched records and carried none of them out is working exactly
/// as asked, and whether that is worth saying depends on what was asked.
///
/// `mode` is carried to the actions rather than consulted here: a dry run is
/// the same loop over the same list, and it passes over the same entries.
fn run(
    manifest: &Manifest,
    selection: &Selection<'_>,
    roots: &Roots,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<usize, Error> {
    // A complaint about the invocation, so it comes before any of the work —
    // including the context, whose own failure would otherwise swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    let context = RunContext::new(roots, mode, reporter)?;
    read_clone_lists(manifest, selection, &context)?;
    // Counted rather than pre-collected, so the loop keeps iterating the
    // manifest's own list and a heading keeps naming a record by its position
    // in it (`guidance.md`, "Seams the late slices need").
    let mut wanted = 0;
    let mut carried_out = 0;

    for (index, entry) in manifest.actions.iter().enumerate() {
        if !selection.wants(entry) {
            continue;
        }
        wanted += 1;

        // Which record is speaking, ahead of what it says — and, where the
        // record is not going to speak at all, why. The user asked for the
        // skip, so restating it at normal verbosity would be noise; `-v` is
        // where the whole account of a run lives.
        let heading = entry.describe(index + 1);
        match selection.skipped(entry) {
            Some(why) => reporter.detail(1, &format!("{heading} - skipped: {why}")),
            None => {
                reporter.detail(1, &heading);
                action::run(entry, &context)?;
                carried_out += 1;
            }
        }
    }

    if wanted == 0
        && let Some(error) = selection.unresolved(roots.manifest())
    {
        return Err(error);
    }
    Ok(carried_out)
}
