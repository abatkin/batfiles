//! Select, prepare, and execute manifest actions for sync and apply commands.

use std::collections::BTreeMap;
use std::rc::Rc;

use crate::action::{
    self, Contribution, Executable, IncludedAction, Inclusion, InclusionContents, RunContext,
};
use crate::bootstrap::Bootstrap;
use crate::clone_list::PreparedList;
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::{ItemAddress, ItemId};
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::{Action, Contributor};
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
    // An empty manifest, and one whose every action is disabled, are both
    // ordinary successful runs that did nothing, so the count is not consulted.
    run(
        Target::Everything,
        Skips {
            actions: skip_actions,
            groups: skip_groups,
        },
        Kind::Sync,
        invocation,
    )?;
    Ok(())
}

/// `clone`'s synchronization: the same run, with the machine's starting point
/// decided from the manifest it just brought down.
pub(crate) fn bootstrap(
    invocation: &Invocation<'_>,
    bootstrap: &Bootstrap,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    run(
        Target::Everything,
        Skips {
            actions: skip_actions,
            groups: skip_groups,
        },
        Kind::Bootstrap(bootstrap),
        invocation,
    )?;
    Ok(())
}

/// The run-only skip lists a command hands over, or the empty ones a command
/// that waives them does.
struct Skips<'a> {
    actions: &'a [String],
    groups: &'a [String],
}

/// Which command the run serves, and the two things that follow from it:
/// whether the declared remotes are materialized before the first action, and
/// whether a bootstrap decides the machine-local lists before they are read.
enum Kind<'a> {
    /// `sync`: materialize, and adopt nothing.
    Sync,
    /// `clone`: materialize, having first settled what this machine starts with
    /// switched off.
    Bootstrap(&'a Bootstrap),
    /// The apply commands: whatever is materialized already, and nothing
    /// fetched.
    Apply,
}

impl Kind<'_> {
    /// Whether this run brings the declared remotes up to date first. An apply
    /// command is aimed at one record, and fetching the whole declared set is
    /// the whole-repository job `sync` is for.
    fn materializes(&self) -> bool {
        !matches!(self, Self::Apply)
    }
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(invocation: &Invocation<'_>, id: &str) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    // Naming an action waives its exclusions, but not remote conditions.
    let processed_action_count = run(
        Target::Action(&id),
        Skips {
            actions: &[],
            groups: &[],
        },
        Kind::Apply,
        invocation,
    )?;

    if processed_action_count == 0 {
        // A target that matched nothing is the error `run` already raised, so
        // the record was found and passed over -- and naming it waived every
        // reason for that but one: the inclusion that contributed it did not
        // select it. The record's own line, with the reason on it, is at `-v`.
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
    // `--skip-group` is not accepted, so there is no group-shaped run-only list
    // to hand over; naming the group waives the one there would have been.
    let processed_action_count = run(
        Target::Group(&group),
        Skips {
            actions: skip_actions,
            groups: &[],
        },
        Kind::Apply,
        invocation,
    )?;

    if processed_action_count == 0 {
        invocation.reporter.info(
            "nothing to apply: every action in the group is disabled, skipped, \
             excluded by its own condition, or was not contributed",
        );
    }
    Ok(())
}

/// One entry in the run's list: a manifest action, the addresses it answers to,
/// how a report names it, and what this run does with it.
pub(crate) struct RunRecord {
    pub action: Action,
    /// The inclusion that contributed it, or `None` for a record the leaf
    /// repository declared. Also what says which of the two a record is.
    ///
    /// Shared with every other record of the same inclusion, so a record's
    /// remote, scope, addressing, and heading are one inclusion's answers.
    from: Option<Rc<Contribution>>,
    /// The address the record's `id` answers to, or `None` where it answers to
    /// none: a record written without one, or one contributed by an inclusion
    /// written without one. An unqualified address never reaches a contributed
    /// record, which is what keeps `zshrc` meaning the leaf's own.
    pub address: Option<ItemAddress>,
    /// The same, for the group the record names.
    pub group_address: Option<ItemAddress>,
    /// How a report names it, settled as the list is built so that the record
    /// and the line naming it cannot come apart.
    pub heading: String,
    pub disposition: Disposition,
}

impl RunRecord {
    /// A record the leaf manifest declared, at its one-based position in it.
    pub fn leaf(action: Action, number: usize) -> Self {
        Self::new(action, number, None)
    }

    /// A record an inclusion contributed, at its one-based position in the
    /// manifest that declared it.
    ///
    /// `from` is the whole of what follows from having been contributed: how a
    /// line names the record, what qualifies its addresses, the tree its paths
    /// are read from, and the scope its conditions are decided in.
    pub fn contributed(action: Action, number: usize, from: &Rc<Contribution>) -> Self {
        Self::new(action, number, Some(Rc::clone(from)))
    }

    fn new(action: Action, number: usize, from: Option<Rc<Contribution>>) -> Self {
        let by = from
            .as_ref()
            .map_or(Contributor::Leaf, |it| it.contributor());
        // A record an inclusion written without an `id` contributed answers to
        // no address at all: the qualified spelling has no first segment to
        // match, and the unqualified one means the leaf's own record of that ID.
        // The heading names such a record's inclusion in words instead. Its
        // qualifier is the one the addresses are built with, so a name in the
        // list and the same name in a line about it cannot come apart.
        let qualifier = by.qualifier();
        let addressed = qualifier.is_some() || from.is_none();
        let qualify = |id: Option<&ItemId>| {
            id.filter(|_| addressed)
                .map(|id| ItemAddress::qualified(qualifier, id))
        };
        let (address, group_address) = (qualify(action.id()), qualify(action.group()));
        let heading = action.describe(number, by);
        Self {
            heading,
            action,
            from,
            address,
            group_address,
            disposition: Disposition::Unwanted,
        }
    }

    /// The remote this record's repository paths are read from, or `None` for
    /// one the leaf repository declared.
    pub fn remote(&self) -> Option<&ItemId> {
        self.from.as_ref().map(|it| it.remote())
    }

    /// The inclusion's variable scope, or `run` for a leaf record.
    fn scope<'a>(&'a self, run: &'a Rc<VarSet>) -> &'a Rc<VarSet> {
        self.from.as_ref().map_or(run, |it| it.scope())
    }

    /// What carrying this record out works from, reading whatever it needs.
    ///
    /// A clone list is the one record that needs anything read: its list comes
    /// from the tree this record came from, as its other paths do, and each
    /// entry's condition is decided against the same [scope](Self::scope) the
    /// record's own was. Every other record is carried out from its declaration,
    /// so there is nothing to read and nothing that can fail.
    fn executable<'a>(
        &'a self,
        context: &RunContext<'_>,
        variables: &Rc<VarSet>,
        host: &HostNamespaces,
    ) -> Result<Executable<'a>, Error> {
        let Action::GitCloneList(list) = &self.action else {
            return Ok(Executable::Declared(&self.action));
        };
        let path = context.source(self.remote(), &list.source)?;
        let bindings = Bindings::new(self.scope(variables), host);
        Ok(Executable::CloneList(PreparedList::prepare(
            list, &path, &bindings,
        )?))
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

/// One step of the run: what the action loop does with one record of the list,
/// with whatever doing it needs already read.
///
/// [`prepare`] makes one for every record, in declaration order, and nothing
/// else makes any. A clone list becomes a [`Run`](Self::Run) only by being read,
/// so no step carries out a list nobody opened.
enum Step<'a> {
    /// Nothing, for a record the command did not ask for.
    Skip,
    /// Say, under the heading naming the record, why this run passes it over.
    Report(&'a RunRecord, &'a Exclusion),
    /// Carry the record out, from this.
    Run(&'a RunRecord, Executable<'a>),
}

/// Settle every record of the run's list into the step the action loop walks,
/// reading whatever carrying one out needs.
///
/// Reading a clone list is the whole of that, and it happens here for each
/// selected, unexcluded list once per run: a missing or malformed one fails the
/// run before anything writes, even where its record comes last, and a list an
/// earlier action would have produced is not yet there to read. Lists this run
/// passes over are not opened at all.
fn prepare<'a>(
    records: &'a [RunRecord],
    context: &RunContext<'_>,
    variables: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<Vec<Step<'a>>, Error> {
    records
        .iter()
        .map(|record| match &record.disposition {
            Disposition::Unwanted => Ok(Step::Skip),
            Disposition::Excluded(exclusion) => Ok(Step::Report(record, exclusion)),
            Disposition::Run => Ok(Step::Run(
                record,
                record.executable(context, variables, host)?,
            )),
        })
        .collect()
}

/// Read the documents, settle what the run carries out, apply the remote
/// policy, prepare clone lists, and execute in declaration order. Dry runs use
/// the same list and preparation. Unknown targets, materialization failures,
/// unreadable inclusions, and action failures are errors.
///
/// The sequence at the top is the one thing here that is not free to move. The
/// variables are resolved before a bootstrap adopts, because a candidate's
/// condition is decided against them; a bootstrap writes `disabled.toml` before
/// it is read, because the selection built from it is what passes over what the
/// bootstrap just switched off.
///
/// Returns how many records this run dispatched: every record it carried out
/// but an inclusion, which counts through what it brought in rather than for
/// itself. Records dispatched and not changes made — an action that found its
/// destination already correct is one, and so is every action of a dry run. A
/// zero says the run reached no record to dispatch at all.
fn run(
    target: Target<'_>,
    skips: Skips<'_>,
    kind: Kind<'_>,
    invocation: &Invocation<'_>,
) -> Result<usize, Error> {
    let reporter = invocation.reporter;
    let manifest = Manifest::load(&invocation.roots.manifest())?;
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

    // Only the leaf's candidates are offered: bootstrap policy belongs to the
    // repository this machine was pointed at, and an included remote's own
    // section is ignored like its `[remotes]`.
    if let Kind::Bootstrap(bootstrap) = kind {
        bootstrap.adopt(
            &manifest.default_disabled,
            &bindings,
            &invocation.roots.state,
            reporter,
        )?;
    }

    let selection = Selection::new(
        target,
        skips.actions,
        skips.groups,
        invocation.env,
        Disabled::load(&invocation.roots.state.disabled())?,
        reporter,
    );

    // Settled before the context, which carries it: an action resolving a path
    // into a remote asks what this run made of that remote's condition.
    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    let context = RunContext::new(
        invocation.roots,
        invocation.mode,
        excluded_remotes,
        reporter,
    )?;

    // Before the list is expanded, before the clone lists are read, and before
    // the first action runs: an inclusion reads a manifest out of a
    // materialization, and an action's source -- including a clone list -- may
    // be inside one.
    if kind.materializes() {
        remotes::materialize(&manifest.remotes, &context)?;
    }

    let run_list = assemble(manifest.actions, &selection, &context, &variables, &host)?;
    // Waits for the list to be assembled: until then there is no telling a name
    // that matched nothing from one an inclusion answers.
    selection.warn_unmatched(&run_list, reporter);

    let RunList {
        records,
        target_found,
        ..
    } = run_list;
    let steps = prepare(&records, &context, &variables, &host)?;
    let mut processed_action_count = 0;

    for step in &steps {
        match step {
            Step::Skip => {}
            Step::Report(record, exclusion) => {
                exclusion.report_heading(reporter, &record.heading);
            }
            Step::Run(record, executable) => {
                reporter.detail(1, &record.heading);
                action::run(executable, record.remote(), &context)?;
                // An inclusion carries nothing out: what it contributed is in
                // this list and counts for itself. Counting the record too would
                // let one the run opened only to reach a group stand in for work
                // that never happened.
                if !matches!(record.action, Action::IncludeRemote(_)) {
                    processed_action_count += 1;
                }
            }
        }
    }

    if !target_found && let Some(error) = selection.unresolved(invocation.roots.manifest()) {
        return Err(error);
    }
    Ok(processed_action_count)
}

/// The run's list, and the two things about assembling it that a report needs.
pub(crate) struct RunList {
    pub records: Vec<RunRecord>,
    /// Whether the command's target named one of these records. An inclusion
    /// opened only to look inside does not count: a target that found nothing
    /// within it found nothing.
    target_found: bool,
    /// The inclusions whose manifest this run did not read, by `id`: the ones it
    /// was not asked to open, the ones an exclusion closed, and the ones there
    /// was nothing to read. A qualified skip naming one of them is not a skip
    /// that matched nothing — the run never read the list that would have
    /// answered it. Inclusion IDs and never action IDs: what went unread is a
    /// manifest, not a record in one.
    pub unread_inclusions: Vec<ItemId>,
}

impl RunList {
    /// Whether the command's target names this record, remembering that the run
    /// reached one it named.
    fn match_and_record_target(&mut self, record: &RunRecord, selection: &Selection<'_>) -> bool {
        let named = selection.wants(record);
        self.target_found |= named;
        named
    }
}

/// What this run does with a record it asked for: the first exclusion that
/// applies to it, or carrying it out.
fn disposition(
    record: &RunRecord,
    selection: &Selection<'_>,
    bindings: &Bindings<'_>,
) -> Disposition {
    match selection.exclusion(record, bindings) {
        Some(exclusion) => Disposition::Excluded(exclusion),
        None => Disposition::Run,
    }
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
/// every record is decided against except one an inclusion put in a
/// [scope](RunRecord::scope) of its own; `host` is what a scope's bindings are
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
        unread_inclusions: Vec::new(),
    };

    for (index, action) in actions.into_iter().enumerate() {
        let mut record = RunRecord::leaf(action, index + 1);

        let target_names_record = run_list.match_and_record_target(&record, selection);
        // Before anything is prepared or run: a target naming a record it cannot
        // carry out is a complaint about the invocation.
        if target_names_record && let Some(error) = selection.refusal(&record.action) {
            return Err(error);
        }

        // Whether this run means to read the inclusion's manifest, which is not
        // the same as having read one: an exclusion below still closes it, and
        // the read itself can come back with nothing. Naming something inside it
        // is the second reason to open one, since `apply-action --id corp.zshrc`
        // reaches past the record without naming it.
        let should_open_inclusion = match &record.action {
            Action::IncludeRemote(inclusion) => {
                target_names_record || selection.reaches_into(inclusion.id.as_ref())
            }
            _ => false,
        };

        if target_names_record || should_open_inclusion {
            // The record's own condition, and a remote's, are the leaf's to
            // decide: an inclusion's `vars` are what it hands to the records it
            // contributes, so they do not decide whether it contributes them.
            record.disposition = disposition(&record, selection, &bindings);
        }

        let Action::IncludeRemote(declaration) = &record.action else {
            run_list.records.push(record);
            continue;
        };
        // Identified before the list is borrowed to push on, and owned from here
        // so that it outlives the record. `index + 1` is the position in this
        // manifest, which nothing below can see.
        let inclusion = Inclusion::at(declaration, index + 1);
        // Either the run was not asked to look inside this one, or an exclusion
        // closed it, or the remote's own condition did. All three leave its
        // contents unread, which is what a qualified skip has to be told apart
        // from — and the last is an exclusion on the record like any other.
        let contents = match context.excluded_remote(inclusion.remote()) {
            Some(exclusion) if matches!(record.disposition, Disposition::Run) => {
                record.disposition = Disposition::Excluded(inclusion.closed_by_remote(exclusion));
                None
            }
            _ if !should_open_inclusion || !matches!(record.disposition, Disposition::Run) => None,
            _ => inclusion.read(context)?,
        };
        let Some(InclusionContents {
            vars: remote_vars,
            actions: included_actions,
        }) = contents
        else {
            run_list.unread_inclusions.extend(inclusion.id().cloned());
            run_list.records.push(record);
            continue;
        };
        // Derived only now that the inclusion has been opened: one the run
        // passed over hands nothing to anything.
        let scope = inclusion_scope(
            &remote_vars,
            &declaration.vars,
            inclusion.label(),
            variables,
            context.reporter(),
        );
        // The one thing every record below is given.
        let from = Rc::new(inclusion.with_scope(scope));
        let scoped = Bindings::new(from.scope(), host);
        run_list.records.push(record);

        for IncludedAction {
            number,
            action,
            included_by_filter,
        } in included_actions
        {
            let mut contributed = RunRecord::contributed(action, number, &from);
            // Asking for the inclusion asks for everything it brought in; the
            // record's own qualified name is the finer way to reach one.
            let target_names_contributed =
                run_list.match_and_record_target(&contributed, selection);
            if target_names_record || target_names_contributed {
                // The inclusion's own filters first, and not through the
                // selection: they say what the leaf composed rather than what
                // this machine leaves out, so no command waives them. Deciding
                // them here also keeps the rule that a record something already
                // excludes never has its own condition evaluated.
                contributed.disposition = if included_by_filter {
                    disposition(&contributed, selection, &scoped)
                } else {
                    Disposition::Excluded(from.not_selected())
                };
            }
            run_list.records.push(contributed);
        }
    }

    Ok(run_list)
}

/// The scope the records one inclusion contributes are decided against: the
/// run's own set with the included remote's `[vars]` beneath it and the
/// inclusion's `vars` overrides above the leaf's, or the run's own where neither
/// declared anything.
///
/// Called once per opened inclusion, and reports the scope at `-vv` as it is
/// derived.
fn inclusion_scope(
    remote: &BTreeMap<VarName, String>,
    overrides: &BTreeMap<VarName, String>,
    label: &str,
    run: &Rc<VarSet>,
    reporter: &Reporter,
) -> Rc<VarSet> {
    // Two empty layers decide every name the way the run's set already does, so
    // there is nothing to derive and nothing to report.
    if remote.is_empty() && overrides.is_empty() {
        return Rc::clone(run);
    }
    let scope = Rc::new(run.with_inclusion(remote, overrides, label));
    scope.report_inclusion(reporter);
    scope
}
