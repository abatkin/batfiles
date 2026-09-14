//! Select, prepare, and execute manifest actions for sync and apply commands.

use std::rc::Rc;

use crate::action::{self, RunContext};
use crate::clone_list;
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::ItemAddress;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::Action;
use crate::mode::RunMode;
use crate::output::Reporter;
use crate::remotes;
use crate::selection::{Selection, Target};
use crate::var::VarName;
use crate::var_set::VarSet;

/// What an invocation settled before any of these commands got to work: where
/// it runs, whether it writes, and the two sources of a variable that the
/// command line and the environment supply.
///
/// One borrowed group rather than five parameters, because every command here
/// takes all five and hands all five on unchanged; what distinguishes them is
/// the record they were pointed at, which each takes for itself.
pub(crate) struct Invocation<'a> {
    pub roots: &'a Roots,
    pub mode: RunMode,
    /// `--var` values, in the order they were written.
    pub vars: &'a [(VarName, String)],
    pub env: &'a Environment,
    pub reporter: &'a Reporter,
}

/// `sync`: bring the home directory to the state the whole manifest describes.
pub(crate) fn sync(
    invocation: &Invocation<'_>,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    let (mut manifest, disabled) = load(invocation.roots)?;
    let selection = Selection::new(
        Target::Everything,
        skip_actions,
        skip_groups,
        invocation.env,
        disabled,
        invocation.reporter,
    );
    // An empty manifest, and one whose every action is disabled, are both
    // ordinary successful runs that did nothing, so the count is not consulted.
    run(&mut manifest, &selection, Remotes::Materialize, invocation)?;
    Ok(())
}

/// What a run does about the repositories the manifest declares.
///
/// The distinction is the command's, not the manifest's: bringing every
/// declared remote up to date is `sync`'s whole-repository job, and an apply
/// command carrying it out would put the network between someone and the one
/// record they named.
enum Remotes {
    /// `sync`: clone what is missing and update what is there, before the first
    /// action.
    Materialize,
    /// The apply commands: whatever is materialized already, and nothing
    /// fetched.
    AsFound,
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(invocation: &Invocation<'_>, id: &str) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    let (mut manifest, disabled) = load(invocation.roots)?;
    // The command accepts neither run-only option, and naming one action waives
    // every exclusion either document holds.
    let selection = Selection::new(
        Target::Action(&id),
        &[],
        &[],
        invocation.env,
        disabled,
        invocation.reporter,
    );
    run(&mut manifest, &selection, Remotes::AsFound, invocation)?;
    Ok(())
}

/// `apply-group`: carry out the records naming `group` that are not themselves
/// disabled or skipped.
pub(crate) fn apply_group(
    invocation: &Invocation<'_>,
    group: &str,
    skip_actions: &[String],
) -> Result<(), Error> {
    let group = ItemAddress::try_from(group.to_owned())?;
    let (mut manifest, disabled) = load(invocation.roots)?;
    // `--skip-group` is not accepted, so there is no group-shaped run-only list
    // to hand over; naming the group waives the one there would have been.
    let selection = Selection::new(
        Target::Group(&group),
        skip_actions,
        &[],
        invocation.env,
        disabled,
        invocation.reporter,
    );
    let executed_count = run(&mut manifest, &selection, Remotes::AsFound, invocation)?;

    if executed_count == 0 {
        invocation.reporter.info(
            "nothing to apply: every action in the group is disabled, skipped, \
             or excluded by its own condition",
        );
    }
    Ok(())
}

/// Read the two documents every one of these commands works from.
fn load(roots: &Roots) -> Result<(Manifest, Disabled), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let disabled = Disabled::load(&roots.state.disabled())?;
    Ok((manifest, disabled))
}

/// An action's position in the manifest, how a report names it, and its
/// run-specific exclusion, if any.
struct SelectedAction {
    index: usize,
    heading: String,
    exclusion: Option<Exclusion>,
}

/// Read every executable clone list from the captured selection before writes,
/// and settle each entry's own condition against this run.
///
/// The contract the action relies on: every list this run may execute leaves
/// here read, and the ones passed over are exactly the ones an exclusion has
/// already closed, which are not executed. An excluded list is never opened, so
/// its record keeps the unread `None` that says nothing looked.
///
/// Entries are attached to the manifest record, which therefore holds both what
/// a repository declared and what this run made of it. That is sound while one
/// record is prepared once per run; an inclusion that splices the same remote
/// twice must splice a copy per inclusion, since the two can differ in `vars`.
///
/// An entry a condition closes stays on its list, marked, so that the action
/// reports it where the rest of the list is reported.
fn prepare_clone_lists(
    manifest: &mut Manifest,
    selected_actions: &[SelectedAction],
    context: &RunContext<'_>,
    bindings: &Bindings<'_>,
) -> Result<(), Error> {
    for selected in selected_actions {
        if selected.exclusion.is_some() {
            continue;
        }
        if let Action::GitCloneList(list) = &mut manifest.actions[selected.index] {
            let mut entries = clone_list::read(&context.source(&list.source)?)?;
            for entry in &mut entries {
                entry.exclusion = entry_exclusion(entry, bindings);
            }
            list.entries = Some(entries);
        }
    }
    Ok(())
}

/// The verdict of one entry's condition, or `None` where this run clones it —
/// including where it declares no condition at all.
///
/// A condition that cannot be decided closes the entry rather than stopping the
/// run, so the entries around it are cloned as they would have been. No
/// consequence clause: the line the caller writes it into opens with `not
/// cloning`.
// CARRY(6.5): `Selection::exclusion` maps an evaluation the same way, and a
// failure must close the gate in both. Remote conditions are the third caller;
// share the mapping then.
fn entry_exclusion(entry: &clone_list::Entry, bindings: &Bindings<'_>) -> Option<Exclusion> {
    let gate = entry.gate()?;
    match gate.admits(bindings) {
        Ok(true) => None,
        Ok(false) => Some(Exclusion::Expected(gate.exclusion_reason())),
        Err(error) => Some(Exclusion::EvaluationFailed(gate.unevaluable(None, &error))),
    }
}

/// Capture selection once, materialize the declared remotes where `remotes`
/// asks for it, prepare executable clone lists, and execute in manifest order.
/// Unknown targets, materialization failures, and action failures are errors.
/// Dry runs use the same selection and preparation.
///
/// The count returned is of action handlers this run invoked and that returned
/// successfully, which is what tells a caller whether the run reached any of the
/// records it was asked for. It is not a count of changes: an action finding its
/// seed already in place, a clone list with no entries, and a list whose every
/// entry warned each count once, the same as one that wrote something.
fn run(
    manifest: &mut Manifest,
    selection: &Selection<'_>,
    remotes: Remotes,
    invocation: &Invocation<'_>,
) -> Result<usize, Error> {
    let reporter = invocation.reporter;
    // A complaint about the invocation, so it comes before any of the work —
    // including the context, whose own failure would otherwise swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    // Resolved for every run, in both modes: it describes the run rather than
    // changing the home directory. Shared rather than copied, because the
    // `vars` namespace answers from these layers rather than from a flattened
    // copy of them.
    let variables = Rc::new(VarSet::resolve(
        &manifest.vars,
        &invocation.roots.state,
        invocation.env,
        invocation.vars,
        reporter,
    )?);
    variables.report(reporter);
    // The host is read once for the whole run, and every condition in it is
    // decided against these bindings.
    let host = HostNamespaces::capture(invocation.env);
    let bindings = Bindings::new(&variables, &host);
    let context = RunContext::new(invocation.roots, invocation.mode, reporter)?;

    let mut selected_actions: Vec<SelectedAction> = Vec::new();
    for (index, action) in manifest.actions.iter().enumerate() {
        if !selection.wants(action) {
            continue;
        }
        // Settled here, and reported below, so that a condition batfiles cannot
        // decide warns under the record's own heading and in manifest order
        // rather than ahead of the run.
        selected_actions.push(SelectedAction {
            index,
            heading: action.describe(index + 1),
            exclusion: selection.exclusion(action, &bindings),
        });
    }

    // Before the lists are read and before the first action runs, because both
    // are what a materialization is for: 6.3 gives an action a source inside
    // one, and the list it reads may be that source.
    if let Remotes::Materialize = remotes {
        remotes::materialize(&manifest.remotes, &context)?;
    }

    prepare_clone_lists(manifest, &selected_actions, &context, &bindings)?;
    let mut executed_count = 0;

    for selected in &selected_actions {
        let heading = &selected.heading;
        match &selected.exclusion {
            Some(Exclusion::Expected(why)) => {
                reporter.detail(1, &format!("{heading} - skipped: {why}"))
            }
            // Without the "skipped" frame the other reasons take: the reason
            // says what is not happening, and this one is printed whether or
            // not the run asked for detail.
            Some(Exclusion::EvaluationFailed(why)) => reporter.warn(&format!("{heading}: {why}")),
            None => {
                reporter.detail(1, heading);
                action::run(&manifest.actions[selected.index], &context)?;
                executed_count += 1;
            }
        }
    }

    if selected_actions.is_empty()
        && let Some(error) = selection.unresolved(invocation.roots.manifest())
    {
        return Err(error);
    }
    Ok(executed_count)
}
