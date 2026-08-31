//! Carrying out a repository's actions: `sync`, `apply-action`, and
//! `apply-group`.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Everything an action does is [`crate::action`]'s; this
//! is the list, and the one place anything iterates it (`guidance.md`, "Seams
//! the late slices need"). The three commands differ only in what they hand
//! that loop: a [`Target`] saying which entries were asked for, and a
//! [`Selection`] saying which of those are passed over anyway.
//!
//! Those two are the same question from opposite sides, and one rule joins
//! them: **an explicit request waives the exclusions naming what it asked
//! for.** `apply-action` names one action, so nothing excludes it;
//! `apply-group` names one group, so a disable on that group is waived while a
//! disable on one of its members is not. `sync` asks for everything and
//! therefore waives nothing.

use crate::action::{self, RunContext};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::ItemId;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::Action;
use crate::mode::RunMode;
use crate::output::Reporter;
use crate::selection::Selection;

/// Which of the manifest's entries a command asked for.
///
/// Holds the borrowed name rather than resolving to an index, because a group
/// names any number of records and an action's position is what the report
/// calls it. Widened to an address at 3.7, which is what lets either arm name
/// something an included remote contributed.
#[derive(Debug)]
enum Target<'a> {
    /// Every record, which is `sync`.
    Everything,
    /// The one record carrying this `id`.
    Action(&'a ItemId),
    /// Every record naming this group.
    Group(&'a ItemId),
}

impl Target<'_> {
    /// Whether this record is one of the ones asked for.
    fn wants(&self, action: &Action) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(id) => action.id() == Some(id),
            Self::Group(group) => action.group() == Some(group),
        }
    }
}

/// What one pass over the list came to.
///
/// The two counts answer different questions and only the caller can tell which
/// it is asking. A target that matched nothing is a command resolving nothing,
/// and what to call that failure depends on whether an action or a group was
/// named; a target that matched records and carried none of them out is a
/// command working exactly as asked. `sync` asks neither question, an empty
/// manifest being an ordinary successful run that did nothing.
#[derive(Debug)]
struct Outcome {
    /// Records the target named.
    wanted: usize,
    /// Records that were not passed over, and so actually ran.
    carried_out: usize,
}

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
    let selection = Selection::new(skip_actions, skip_groups, env, disabled, reporter);
    // A complaint about the invocation, so it comes before the work rather than
    // after a failure that would swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    // An empty manifest, and one whose every action is disabled, are both
    // ordinary successful runs that did nothing, so neither count is consulted.
    run(&manifest, &Target::Everything, &selection, roots, mode, reporter)?;
    Ok(())
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(
    roots: &Roots,
    mode: RunMode,
    id: &str,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Checked before the repository is opened, so a name that could never have
    // matched is reported as the malformed name it is rather than as a
    // resolution failure. 3.7 widens this to an address.
    let id = ItemId::try_from(id.to_owned())?;
    let (manifest, _disabled) = load(roots)?;
    let outcome = run(
        &manifest,
        &Target::Action(&id),
        &Selection::waiving_everything(),
        roots,
        mode,
        reporter,
    )?;
    // Nothing filters this command, so a record that was named was carried out:
    // the only way to come away with nothing is for no record to carry the ID.
    if outcome.wanted == 0 {
        return Err(Error::UnknownAction {
            path: roots.manifest(),
            id,
        });
    }
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
    let group = ItemId::try_from(group.to_owned())?;
    let (manifest, disabled) = load(roots)?;
    let selection = Selection::for_one_group(skip_actions, env, disabled, reporter);
    selection.warn_unmatched(&manifest.actions, reporter);
    let outcome = run(
        &manifest,
        &Target::Group(&group),
        &selection,
        roots,
        mode,
        reporter,
    )?;

    // A group is nothing but the actions naming it, so one no action names does
    // not exist, and an empty one and an absent one are the same failure.
    if outcome.wanted == 0 {
        return Err(Error::UnknownGroup {
            path: roots.manifest(),
            group,
        });
    }
    // The group exists and every member was passed over, which at normal
    // verbosity would otherwise be silence in answer to a command that named
    // one thing. `-v` has already said which record and why.
    if outcome.carried_out == 0 {
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
/// `apply-action` waives both lists and so has no use for the second document,
/// and reads it anyway. Loading what every command loads keeps one preparation
/// path and one rule — a state file that cannot be read fails any command that
/// executes actions — where the alternative buys a per-command account of which
/// files get opened, and nothing depends on it.
fn load(roots: &Roots) -> Result<(Manifest, Disabled), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let disabled = Disabled::load(&roots.disabled())?;
    Ok((manifest, disabled))
}

/// The one action-execution loop: everything the target asked for, in
/// declaration order, stopping at the first failure.
///
/// `mode` is carried to the actions rather than consulted here: a dry run is
/// the same loop over the same list, and it passes over the same entries.
fn run(
    manifest: &Manifest,
    target: &Target<'_>,
    selection: &Selection,
    roots: &Roots,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<Outcome, Error> {
    let context = RunContext::new(roots, mode, reporter)?;
    // Counted rather than pre-collected, so the loop keeps iterating the
    // manifest's own list and a heading keeps naming a record by its position
    // in it (`guidance.md`, "Seams the late slices need").
    let mut outcome = Outcome {
        wanted: 0,
        carried_out: 0,
    };

    for (index, entry) in manifest.actions.iter().enumerate() {
        if !target.wants(entry) {
            continue;
        }
        outcome.wanted += 1;

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
                outcome.carried_out += 1;
            }
        }
    }
    Ok(outcome)
}
