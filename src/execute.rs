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

/// Resolved roots, mode, CLI variables, environment, and reporter for a command.
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
    run(
        &mut manifest,
        &selection,
        RemotePolicy::Materialize,
        invocation,
    )?;
    Ok(())
}

/// Whether execution materializes declared remotes or uses existing trees.
enum RemotePolicy {
    /// `sync`: clone what is missing and update what is there, before the first
    /// action.
    Materialize,
    /// The apply commands: whatever is materialized already, and nothing
    /// fetched.
    UseExisting,
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(invocation: &Invocation<'_>, id: &str) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    let (mut manifest, disabled) = load(invocation.roots)?;
    // Naming an action waives its exclusions, but not remote conditions.
    let selection = Selection::new(
        Target::Action(&id),
        &[],
        &[],
        invocation.env,
        disabled,
        invocation.reporter,
    );
    run(
        &mut manifest,
        &selection,
        RemotePolicy::UseExisting,
        invocation,
    )?;
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
    let executed_count = run(
        &mut manifest,
        &selection,
        RemotePolicy::UseExisting,
        invocation,
    )?;

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

/// Read selected, non-excluded clone lists and evaluate their entry conditions.
/// Attach entries to each manifest record once per run, retaining excluded entries
/// for reporting. Unread lists keep `None`; a validated empty list holds `Some([])`.
/// Missing or malformed lists fail before any action executes.
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

/// Evaluate an entry's condition, returning `None` when it may run.
/// The caller supplies the `not cloning` prefix when reporting an exclusion.
fn entry_exclusion(entry: &clone_list::Entry, bindings: &Bindings<'_>) -> Option<Exclusion> {
    entry.gate()?.exclusion(bindings, None)
}

/// Capture selection, apply the remote policy, prepare clone lists, and execute
/// actions in manifest order. Dry runs use the same selection and preparation.
/// Unknown targets, materialization failures, and action failures are errors.
/// Returns the number of handlers that completed successfully, including no-ops.
fn run(
    manifest: &mut Manifest,
    selection: &Selection<'_>,
    remotes: RemotePolicy,
    invocation: &Invocation<'_>,
) -> Result<usize, Error> {
    let reporter = invocation.reporter;
    // A complaint about the invocation, so it comes before any of the work —
    // including the context, whose own failure would otherwise swallow it.
    selection.warn_unmatched(&manifest.actions, reporter);
    // Resolve variables even when no action declares a condition.
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
    // Settled before the context, which carries it: an action resolving a path
    // into a remote asks what this run made of that remote's condition.
    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    let context = RunContext::new(
        invocation.roots,
        invocation.mode,
        excluded_remotes,
        reporter,
    )?;

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
    // are what a materialization is for: an action's source may be inside one,
    // and the list it reads may be that source.
    if let RemotePolicy::Materialize = remotes {
        remotes::materialize(&manifest.remotes, &context)?;
    }

    prepare_clone_lists(manifest, &selected_actions, &context, &bindings)?;
    let mut executed_count = 0;

    for selected in &selected_actions {
        let heading = &selected.heading;
        match &selected.exclusion {
            Some(exclusion) => exclusion.report_heading(reporter, heading),
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
