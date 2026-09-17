//! Select, prepare, and execute manifest actions for sync and apply commands.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::action::{self, IncludedAction, RunContext};
use crate::clone_list;
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::{ItemAddress, ItemId};
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
    let (manifest, disabled) = load(invocation.roots)?;
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
    run(manifest, &selection, RemotePolicy::Materialize, invocation)?;
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
    let (manifest, disabled) = load(invocation.roots)?;
    // Naming an action waives its exclusions, but not remote conditions.
    let selection = Selection::new(
        Target::Action(&id),
        &[],
        &[],
        invocation.env,
        disabled,
        invocation.reporter,
    );
    let executed_count = run(manifest, &selection, RemotePolicy::UseExisting, invocation)?;

    if executed_count == 0 {
        // A target that matched nothing is the error `run` already raised, so
        // reaching here means the record was found and passed over. One thing
        // does that to a record this command named: the inclusion that
        // contributed it did not select it, which is the leaf's own description
        // of what it took and not something naming the record waives. The
        // record's own line, with the reason on it, is at `-v`.
        invocation
            .reporter
            .info("nothing to apply: the inclusion that contributed the action did not select it");
    }
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
    let (manifest, disabled) = load(invocation.roots)?;
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
    let executed_count = run(manifest, &selection, RemotePolicy::UseExisting, invocation)?;

    if executed_count == 0 {
        // The last clause is what an inclusion added to this list: a group may
        // now hold one, and a group whose only member brought nothing in has no
        // action that was disabled, skipped, or excluded.
        invocation.reporter.info(
            "nothing to apply: every action in the group is disabled, skipped, \
             excluded by its own condition, or was not contributed",
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

/// One record in the run's list: the action, the addresses it answers to, how a
/// report names it, and what this run does with it.
///
/// A leaf manifest's record and one an [inclusion](assemble) contributed differ
/// in exactly three ways, and all three are fields here rather than a kind of
/// their own: a contributed record is addressed under the inclusion that
/// brought it in, its repository paths are read from that remote's tree, and its
/// conditions may be decided against a variable set that inclusion's `vars`
/// derived.
pub(crate) struct Record {
    pub action: Action,
    /// The remote whose materialization this record's paths are read from, or
    /// `None` for one the leaf repository declared. Set for every contributed
    /// record and for no other, so it is also what says where one came from.
    pub remote: Option<ItemId>,
    /// The address the record's `id` answers to, or `None` where it answers to
    /// none: a record written without one, or one contributed by an inclusion
    /// written without one. An unqualified address never reaches a contributed
    /// record, which is what keeps `zshrc` meaning the leaf's own.
    pub name: Option<ItemAddress>,
    /// The same, for the group the record names.
    pub group: Option<ItemAddress>,
    /// How a report names it, settled as the list is built so that the record
    /// and the line naming it cannot come apart.
    pub heading: String,
    /// The variable set this record's conditions are decided against, where an
    /// inclusion's `vars` derived one for it. `None` is every other record,
    /// which is decided against the run's own set: see [`Self::scope`].
    vars: Option<Rc<VarSet>>,
    pub disposition: Disposition,
}

impl Record {
    /// A record the leaf manifest declared, at its one-based position in it.
    pub fn leaf(action: Action, number: usize) -> Self {
        Self::new(action, number, None, None)
    }

    /// A record an inclusion contributed, at its one-based position in the
    /// manifest that declared it. `inclusion` is the `id` of the record that
    /// brought it in, absent where that record was written without one, and
    /// `remote` the tree its paths are read from.
    pub fn contributed(
        action: Action,
        number: usize,
        inclusion: Option<&ItemId>,
        remote: ItemId,
    ) -> Self {
        Self::new(action, number, inclusion, Some(remote))
    }

    fn new(
        action: Action,
        number: usize,
        inclusion: Option<&ItemId>,
        remote: Option<ItemId>,
    ) -> Self {
        // A record an inclusion written without an `id` contributed answers to
        // no address at all: the qualified spelling has no first segment to
        // match, and the unqualified one means the leaf's own record of that ID.
        // A contributed record is the one with a remote, which is what tells it
        // from a leaf record whose own `id` is missing.
        let addressed = inclusion.is_some() || remote.is_none();
        let address = |id: Option<&ItemId>| {
            id.filter(|_| addressed)
                .map(|id| ItemAddress::qualified(inclusion, id))
        };
        let (name, group) = (address(action.id()), address(action.group()));
        Self {
            heading: action.describe(number, inclusion),
            action,
            remote,
            name,
            group,
            vars: None,
            disposition: Disposition::Unwanted,
        }
    }

    /// Put the record in the scope an inclusion's `vars` derived, which is the
    /// set its own condition and its clone list's entry conditions are then
    /// decided against.
    fn in_scope(mut self, vars: &Rc<VarSet>) -> Self {
        self.vars = Some(Rc::clone(vars));
        self
    }

    /// The variable set this record's conditions are decided against: the one
    /// an inclusion derived for it, or `run`'s own where no inclusion did.
    ///
    /// One accessor rather than two call sites choosing, so that the set a
    /// record was selected against is the set its clone list is read against.
    fn scope<'a>(&'a self, run: &'a Rc<VarSet>) -> &'a Rc<VarSet> {
        self.vars.as_ref().unwrap_or(run)
    }
}

/// What this run does with one record.
pub(crate) enum Disposition {
    /// The command did not ask for it. Kept in the list all the same, so that a
    /// skip naming it is not reported as naming nothing.
    Unwanted,
    /// Asked for, and passed over for this reason.
    Excluded(Exclusion),
    /// Asked for, and carried out.
    Run,
}

/// Read selected, non-excluded clone lists and evaluate their entry conditions.
/// Attach entries to each record of the run's list once per run, retaining
/// excluded entries for reporting. A list an inclusion contributed is read from
/// that remote's materialization, as its record's other paths are, and its
/// entry conditions are decided against the same [scope](Record::scope) the
/// record was.
/// Unread lists keep `None`; a validated empty list holds `Some([])`.
/// Missing or malformed lists fail before any action executes.
fn prepare_clone_lists(
    records: &mut [Record],
    context: &RunContext<'_>,
    variables: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<(), Error> {
    for record in records {
        if !matches!(record.disposition, Disposition::Run) {
            continue;
        }
        // Both taken before the record is borrowed to be written on: the
        // entries are settled on the record that holds the remote they are read
        // from and the variables they are decided against.
        let remote = record.remote.clone();
        let vars = Rc::clone(record.scope(variables));
        if let Action::GitCloneList(list) = &mut record.action {
            let bindings = Bindings::new(&vars, host);
            let mut entries = clone_list::read(&context.source(remote.as_ref(), &list.source)?)?;
            for entry in &mut entries {
                entry.exclusion = entry_exclusion(entry, &bindings);
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

/// Apply the remote policy, expand the run's list, prepare clone lists, and
/// execute in declaration order. Dry runs use the same list and preparation.
/// Unknown targets, materialization failures, unreadable inclusions, and action
/// failures are errors.
/// Returns the number of records that carried work out, which an inclusion
/// never does: what it contributed is in the same list and counts for itself.
fn run(
    manifest: Manifest,
    selection: &Selection<'_>,
    remotes: RemotePolicy,
    invocation: &Invocation<'_>,
) -> Result<usize, Error> {
    let reporter = invocation.reporter;
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

    // Before the list is expanded, before the lists are read, and before the
    // first action runs, because all three are what a materialization is for: an
    // inclusion reads a manifest out of one, an action's source may be inside
    // one, and the list it reads may be that source.
    if let RemotePolicy::Materialize = remotes {
        remotes::materialize(&manifest.remotes, &context)?;
    }

    let run_list = assemble(manifest.actions, selection, &context, &variables, &host)?;
    // A complaint about the invocation, and it waits for the list to be
    // assembled because until then there is no telling a name that matched
    // nothing from one an inclusion answers.
    selection.warn_unmatched(&run_list, reporter);

    let RunList {
        mut records,
        target_found,
        ..
    } = run_list;
    prepare_clone_lists(&mut records, &context, &variables, &host)?;
    let mut executed_count = 0;

    for record in &records {
        match &record.disposition {
            Disposition::Unwanted => {}
            Disposition::Excluded(exclusion) => {
                exclusion.report_heading(reporter, &record.heading);
            }
            Disposition::Run => {
                reporter.detail(1, &record.heading);
                action::run(&record.action, record.remote.as_ref(), &context)?;
                // An inclusion carries nothing out: what it contributed is in
                // this list and counts for itself. Counting the record too would
                // let an inclusion the run only opened to reach a group stand in
                // for work that never happened.
                if !matches!(record.action, Action::IncludeRemote(_)) {
                    executed_count += 1;
                }
            }
        }
    }

    if !target_found && let Some(error) = selection.unresolved(invocation.roots.manifest()) {
        return Err(error);
    }
    Ok(executed_count)
}

/// The run's list, and the two things about assembling it that a report needs.
pub(crate) struct RunList {
    pub records: Vec<Record>,
    /// Whether the command's target named one of these records. An inclusion
    /// opened only to look inside does not count: a target that found nothing
    /// within it found nothing.
    target_found: bool,
    /// The inclusions whose manifest this run did not read, by `id`: the ones it
    /// was not asked to open, the ones an exclusion closed, and the ones there
    /// was nothing to read. A qualified skip naming one of them is not a skip
    /// that matched nothing — the run never read the list that would have
    /// answered it.
    pub unread: Vec<ItemId>,
}

/// Build the run's list from the manifest's, expanding every inclusion the run
/// reaches, and decide what this run does with each record.
///
/// One pass, because neither half can precede the other: an inclusion is opened
/// only where this run's selection admits it, and what it brings in is then
/// selected like anything else. A manifest that cannot be read, and a target
/// naming a record it cannot carry out, both fail here — before the first action
/// runs, as a malformed clone list does.
///
/// `context` must already hold whatever materializations this command provides,
/// since an inclusion reads one. `variables` is the run's own set, which is what
/// every record is decided against except one an inclusion writing `vars` put in
/// a [scope](Record::scope) of its own; `host` is what a scope's bindings are
/// built over.
fn assemble(
    actions: Vec<Action>,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
    variables: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<RunList, Error> {
    let bindings = Bindings::new(variables, host);
    let mut run_list = RunList {
        records: Vec::with_capacity(actions.len()),
        target_found: false,
        unread: Vec::new(),
    };

    for (index, action) in actions.into_iter().enumerate() {
        let mut record = Record::leaf(action, index + 1);

        let wanted = selection.wants(&record);
        if wanted {
            // Before anything is prepared or run: a target naming a record it
            // cannot carry out is a complaint about the invocation.
            if let Some(error) = selection.refusal(&record.action) {
                return Err(error);
            }
            run_list.target_found = true;
        }

        // An inclusion is opened where the command asked for it, and also where
        // the command named something inside it: `apply-action --id corp.zshrc`
        // reaches past the record without naming it.
        let opened = match &record.action {
            Action::IncludeRemote(inclusion) => {
                wanted || selection.reaches_into(inclusion.id.as_ref())
            }
            _ => false,
        };

        if wanted || opened {
            // The record's own condition, and a remote's, are the leaf's to
            // decide: an inclusion's `vars` are what it hands to the records it
            // contributes, so they do not decide whether it contributes them.
            record.disposition = match selection.exclusion(&record, &bindings) {
                Some(exclusion) => Disposition::Excluded(exclusion),
                None => Disposition::Run,
            };
        }

        let Action::IncludeRemote(inclusion) = &record.action else {
            run_list.records.push(record);
            continue;
        };
        let (remote, qualifier) = (inclusion.remote.clone(), inclusion.id.clone());
        // How a report names this inclusion, and what the run says about a
        // record its filters left out. Both taken while the record that wrote
        // them is still to hand and before the list is borrowed to push on.
        let label = action::inclusion_label(inclusion);
        let not_selected = format!("not selected by {label}");
        // Either the run was not asked to look inside this one, or an exclusion
        // closed it, or the remote's own condition did. All three leave its
        // contents unread, which is what a qualified skip has to be told apart
        // from — and the last is an exclusion on the record like any other.
        let included_actions = match context.excluded_remote(&remote) {
            Some(exclusion) if matches!(record.disposition, Disposition::Run) => {
                record.disposition = Disposition::Excluded(closed_by_remote(exclusion, &remote));
                None
            }
            _ if !opened || !matches!(record.disposition, Disposition::Run) => None,
            _ => action::read_inclusion(inclusion, context)?,
        };
        let Some(included_actions) = included_actions else {
            run_list.unread.extend(qualifier);
            run_list.records.push(record);
            continue;
        };
        // Derived once for the inclusion, and only now that it has been opened:
        // an inclusion the run passed over hands its overrides to nothing and
        // has nothing to report about them.
        let scope = inclusion_scope(&inclusion.vars, &label, variables, context.reporter());
        let scoped = Bindings::new(&scope, host);
        run_list.records.push(record);

        for IncludedAction {
            number,
            action,
            selected,
        } in included_actions
        {
            let mut contributed =
                Record::contributed(action, number, qualifier.as_ref(), remote.clone())
                    .in_scope(&scope);
            // Asking for the inclusion asks for everything it brought in; the
            // record's own qualified name is the finer way to reach one.
            let named = selection.wants(&contributed);
            if named {
                run_list.target_found = true;
            }
            if wanted || named {
                // The inclusion's own filters first, and not through the
                // selection: they are the leaf saying what it composes rather
                // than what this machine leaves out, so `apply-action` naming
                // one record does not waive them, as it does not waive a
                // remote's condition. Deciding them here also keeps the rule
                // that a record something already excludes never has its own
                // condition evaluated.
                contributed.disposition = if selected {
                    match selection.exclusion(&contributed, &scoped) {
                        Some(exclusion) => Disposition::Excluded(exclusion),
                        None => Disposition::Run,
                    }
                } else {
                    Disposition::Excluded(Exclusion::Expected(not_selected.clone()))
                };
            }
            run_list.records.push(contributed);
        }
    }

    Ok(run_list)
}

/// The variable set the records one inclusion contributes are decided against:
/// the run's own where the inclusion wrote no `vars`, and the run's own with
/// those overrides above the leaf's `[vars]` where it did.
///
/// Derived once per opened inclusion and reported at `-vv` as it is derived, so
/// that what a contributed record's condition read is visible beside the run's
/// own variables rather than only in the record's outcome.
fn inclusion_scope(
    overrides: &BTreeMap<VarName, String>,
    label: &str,
    run: &Rc<VarSet>,
    reporter: &Reporter,
) -> Rc<VarSet> {
    if overrides.is_empty() {
        return Rc::clone(run);
    }
    let scope = Rc::new(run.with_inclusion(overrides, label));
    scope.report_inclusion(reporter);
    scope
}

/// A remote's exclusion, said about the inclusion that depended on it. The
/// severity is the remote's: a condition batfiles could not decide is a warning
/// wherever it is reported.
fn closed_by_remote(exclusion: &Exclusion, remote: &ItemId) -> Exclusion {
    let reason = format!("remote `{remote}` is excluded here: {}", exclusion.reason());
    match exclusion {
        Exclusion::Expected(_) => Exclusion::Expected(reason),
        Exclusion::EvaluationFailed(_) => Exclusion::EvaluationFailed(reason),
    }
}
