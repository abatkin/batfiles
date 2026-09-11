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
use crate::selection::{Selection, Target};
use crate::var::VarName;
use crate::var_set::VarSet;

/// `sync`: bring the home directory to the state the whole manifest describes.
pub(crate) fn sync(
    roots: &Roots,
    mode: RunMode,
    skip_actions: &[String],
    skip_groups: &[String],
    vars: &[(VarName, String)],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let (mut manifest, disabled) = load(roots)?;
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
    run(&mut manifest, &selection, roots, mode, vars, env, reporter)?;
    Ok(())
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(
    roots: &Roots,
    mode: RunMode,
    id: &str,
    vars: &[(VarName, String)],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    let (mut manifest, disabled) = load(roots)?;
    // The command accepts neither run-only option, and naming one action waives
    // every exclusion either document holds.
    let selection = Selection::new(Target::Action(&id), &[], &[], env, disabled, reporter);
    run(&mut manifest, &selection, roots, mode, vars, env, reporter)?;
    Ok(())
}

/// `apply-group`: carry out the records naming `group` that are not themselves
/// disabled or skipped.
pub(crate) fn apply_group(
    roots: &Roots,
    mode: RunMode,
    group: &str,
    skip_actions: &[String],
    vars: &[(VarName, String)],
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let group = ItemAddress::try_from(group.to_owned())?;
    let (mut manifest, disabled) = load(roots)?;
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
    let executed_count = run(&mut manifest, &selection, roots, mode, vars, env, reporter)?;

    if executed_count == 0 {
        reporter.info(
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

/// Capture selection once, prepare executable clone lists, and execute in
/// manifest order. Unknown targets and action failures are errors. Dry runs use
/// the same selection and preparation.
///
/// The count returned is of action handlers this run invoked and that returned
/// successfully, which is what tells a caller whether the run reached any of the
/// records it was asked for. It is not a count of changes: an action finding its
/// seed already in place, a clone list with no entries, and a list whose every
/// entry warned each count once, the same as one that wrote something.
fn run(
    manifest: &mut Manifest,
    selection: &Selection<'_>,
    roots: &Roots,
    mode: RunMode,
    vars: &[(VarName, String)],
    env: &Environment,
    reporter: &Reporter,
) -> Result<usize, Error> {
    // A complaint about the invocation, so it comes before any of the work —
    // including the context, whose own failure would otherwise swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    // Resolved for every run, in both modes: it describes the run rather than
    // changing the home directory. Shared rather than copied, because the
    // `vars` namespace answers from these layers rather than from a flattened
    // copy of them.
    let variables = Rc::new(VarSet::resolve(
        &manifest.vars,
        &roots.state,
        env,
        vars,
        reporter,
    )?);
    variables.report(reporter);
    // The host is read once for the whole run, and every condition in it is
    // decided against these bindings.
    let host = HostNamespaces::capture(env);
    let bindings = Bindings::new(&variables, &host);
    let context = RunContext::new(roots, mode, reporter)?;

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
        && let Some(error) = selection.unresolved(roots.manifest())
    {
        return Err(error);
    }
    Ok(executed_count)
}
