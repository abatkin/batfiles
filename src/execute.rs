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
//! them: **an exclusion no finer-grained than what the command named is
//! waived.** `apply-action` names one action and nothing is finer than that, so
//! nothing excludes it; `apply-group` names one group, so a disable on that
//! group is waived while a disable on one of its members is not; `sync` asks for
//! everything, so every exclusion outranks it and none is waived. A target says
//! which of the three it is ([`Target::named`]), so the two sides cannot
//! disagree about what the command named.

use std::path::PathBuf;

use crate::action::{self, RunContext};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::ItemAddress;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::Action;
use crate::mode::RunMode;
use crate::output::Reporter;
use crate::selection::{Named, Selection};

/// Which of the manifest's entries a command asked for.
///
/// Holds the borrowed name rather than resolving to an index, because a group
/// names any number of records and an action's position is what the report
/// calls it.
#[derive(Debug)]
enum Target<'a> {
    /// Every record, which is `sync`.
    Everything,
    /// The one record answering to this address.
    Action(&'a ItemAddress),
    /// Every record naming this group.
    Group(&'a ItemAddress),
}

impl Target<'_> {
    /// Whether this record is one of the ones asked for.
    fn wants(&self, action: &Action) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(id) => action.id().is_some_and(|declared| id.names(declared)),
            Self::Group(group) => action.group().is_some_and(|declared| group.names(declared)),
        }
    }

    /// How specifically this target names what it asked for, which is what
    /// decides the exclusions the run waives.
    fn named(&self) -> Named {
        match self {
            Self::Everything => Named::Nothing,
            Self::Action(_) => Named::Action,
            Self::Group(_) => Named::Group,
        }
    }

    /// What it means for this target to have matched no record at all, which
    /// only the arm that asked can say.
    ///
    /// Naming an action or a group that nothing in the manifest answers to is a
    /// command resolving nothing, and each gets its own failure. A group is
    /// nothing but the actions naming it, so an empty one and an absent one are
    /// the same failure. `sync` names nothing to resolve: an empty manifest is
    /// an ordinary successful run that did nothing.
    fn unresolved(&self, manifest: PathBuf) -> Option<Error> {
        match self {
            Self::Everything => None,
            Self::Action(id) => Some(Error::UnknownAction {
                path: manifest,
                id: (*id).clone(),
            }),
            Self::Group(group) => Some(Error::UnknownGroup {
                path: manifest,
                group: (*group).clone(),
            }),
        }
    }
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
    let target = Target::Everything;
    let selection = Selection::new(
        target.named(),
        skip_actions,
        skip_groups,
        env,
        disabled,
        reporter,
    );
    // An empty manifest, and one whose every action is disabled, are both
    // ordinary successful runs that did nothing, so the count is not consulted.
    run(&manifest, &target, &selection, roots, mode, reporter)?;
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
    let target = Target::Action(&id);
    // The command accepts neither run-only option, and naming one action waives
    // every exclusion either document holds.
    let selection = Selection::new(target.named(), &[], &[], env, disabled, reporter);
    // Nothing filters this command, so a record it named was carried out, and a
    // name no record carries has already failed as unresolved.
    run(&manifest, &target, &selection, roots, mode, reporter)?;
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
    let target = Target::Group(&group);
    // `--skip-group` is not accepted, so there is no group-shaped run-only list
    // to hand over; naming the group waives the one there would have been.
    let selection = Selection::new(target.named(), skip_actions, &[], env, disabled, reporter);
    let carried_out = run(&manifest, &target, &selection, roots, mode, reporter)?;

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
    target: &Target<'_>,
    selection: &Selection,
    roots: &Roots,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<usize, Error> {
    // A complaint about the invocation, so it comes before any of the work —
    // including the context, whose own failure would otherwise swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    let context = RunContext::new(roots, mode, reporter)?;
    // Counted rather than pre-collected, so the loop keeps iterating the
    // manifest's own list and a heading keeps naming a record by its position
    // in it (`guidance.md`, "Seams the late slices need").
    let mut wanted = 0;
    let mut carried_out = 0;

    for (index, entry) in manifest.actions.iter().enumerate() {
        if !target.wants(entry) {
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
        && let Some(error) = target.unresolved(roots.manifest())
    {
        return Err(error);
    }
    Ok(carried_out)
}
