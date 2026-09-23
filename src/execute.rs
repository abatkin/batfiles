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
    // A sync that dispatches nothing is an ordinary success.
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

/// `clone`'s synchronization: a `sync` that first adopts the cloned manifest's
/// bootstrap policy.
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

/// The `--skip-action` and `--skip-group` values for this run; empty where the
/// command waives them.
struct Skips<'a> {
    actions: &'a [String],
    groups: &'a [String],
}

/// The command a run serves. Decides whether remotes are materialized before
/// assembly and whether a bootstrap writes `disabled.toml` before it is read.
enum Kind<'a> {
    /// `sync`: materializes; adopts nothing.
    Sync,
    /// `clone`: adopts the bootstrap policy, then materializes.
    Bootstrap(&'a Bootstrap),
    /// `apply-action` and `apply-group`: use existing materializations and fetch
    /// nothing.
    Apply,
}

impl Kind<'_> {
    /// Whether the run materializes the declared remotes. Apply commands target
    /// one record or group; updating every remote is `sync`'s job.
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
        // `run` fails on a target that matched nothing, so the record was found
        // and not run. Naming it waived every exclusion except its inclusion's
        // filters. `-v` shows the record's own line.
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
    // No `--skip-group`: naming the group waives group skips.
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

/// One entry in the run's list: an action, its addresses, its report heading,
/// and what this run does with it.
pub(crate) struct RunRecord {
    pub action: Action,
    /// The [`Contribution`] of the inclusion that contributed this record, shared
    /// by all of that inclusion's records; `None` for a leaf record.
    from: Option<Rc<Contribution>>,
    /// The address the record's `id` answers to. `None` for a record without an
    /// `id` or one from an inclusion without an `id`. A contributed record's
    /// address is always qualified, so an unqualified `zshrc` reaches only the
    /// leaf's record.
    pub address: Option<ItemAddress>,
    /// The address of the record's `group`, qualified the same way.
    pub group_address: Option<ItemAddress>,
    /// How reports name the record, fixed when the record is built.
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
    pub fn contributed(action: Action, number: usize, from: &Rc<Contribution>) -> Self {
        Self::new(action, number, Some(Rc::clone(from)))
    }

    fn new(action: Action, number: usize, from: Option<Rc<Contribution>>) -> Self {
        let by = from
            .as_ref()
            .map_or(Contributor::Leaf, |it| it.contributor());
        // A record from an inclusion without an `id` has no address: there is no
        // qualifier to match, and an unqualified address means the leaf's
        // record. Its heading names the inclusion instead. Addresses and heading
        // use the same qualifier.
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

    /// Prepare this record for execution.
    ///
    /// Only a clone list reads anything: its list, from the same tree as the
    /// record's other paths, with entry conditions decided in the record's
    /// [scope](Self::scope). Every other record executes from its declaration
    /// and cannot fail here.
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
    /// Not requested by the command. Kept in the list so a skip naming it is not
    /// reported as matching nothing.
    Unwanted,
    /// Requested, and excluded for this reason.
    Excluded(Exclusion),
    /// Requested, and executed.
    Run,
}

/// What the action loop does with one record, with its inputs already read.
///
/// Only [`prepare`] builds steps, one per record in declaration order. A clone
/// list becomes a [`Run`](Self::Run) only once it has been read.
enum Step<'a> {
    /// Nothing: the record was not requested.
    Skip,
    /// Report the exclusion under the record's heading.
    Report(&'a RunRecord, &'a Exclusion),
    /// Execute the record.
    Run(&'a RunRecord, Executable<'a>),
}

/// Turn each record into a [`Step`], reading every selected, unexcluded clone
/// list once.
///
/// Runs after bootstrap adoption and remote materialization, and before the
/// first action. A missing or malformed list fails the run before any action
/// writes, even if its record comes last; what adoption and materialization
/// already wrote remains. A list an earlier action would produce is not yet
/// available. Unwanted and excluded lists are not opened.
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

/// Load the manifest and variables, adopt any bootstrap policy, materialize
/// remotes, assemble and select the run's list, prepare clone lists, and
/// execute in declaration order. Dry runs use the same list and preparation.
/// Unknown targets, materialization failures, unreadable inclusions, and action
/// failures are errors.
///
/// Variables resolve before bootstrap adoption, which decides candidate
/// conditions against them. Adoption writes `disabled.toml` before the
/// selection reads it.
///
/// Returns the number of records dispatched, not counting inclusions. A record
/// counts whether or not it changed anything, including in a dry run.
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
    // Captured once; every condition in the run is decided against it.
    let host = HostNamespaces::capture(invocation.env);
    let bindings = Bindings::new(&variables, &host);

    // Only the leaf's `[default-disabled]` is adopted; an included manifest's
    // is ignored, like its `[remotes]`.
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

    // Decided before the context, which carries it: resolving a path into a
    // remote checks that remote's condition.
    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    let context = RunContext::new(
        invocation.roots,
        invocation.mode,
        excluded_remotes,
        reporter,
    )?;

    // Before assembly, preparation, and execution: an inclusion reads its
    // manifest from a materialization, and an action's source, including a
    // clone list, may be inside one.
    if kind.materializes() {
        remotes::materialize(&manifest.remotes, &context)?;
    }

    let run_list = assemble(manifest.actions, &selection, &context, &variables, &host)?;
    // After assembly: only then can a name that matched nothing be told from
    // one an inclusion answers.
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
                // An inclusion executes nothing; its contributed records count
                // for themselves. Counting it would hide an `apply-group` that
                // opened an inclusion and ran none of its records.
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

/// The run's list, and what reports need to know about its assembly.
pub(crate) struct RunList {
    pub records: Vec<RunRecord>,
    /// Whether the target named a record in the list. An inclusion opened only
    /// to reach inside it does not count.
    target_found: bool,
    /// IDs of inclusions whose manifest this run did not read: not requested,
    /// excluded, or not materialized. A qualified skip into one of them is not
    /// reported as matching nothing.
    pub unread_inclusions: Vec<ItemId>,
}

impl RunList {
    /// Whether the target names `record`, noting that the target was found.
    fn match_and_record_target(&mut self, record: &RunRecord, selection: &Selection<'_>) -> bool {
        let named = selection.wants(record);
        self.target_found |= named;
        named
    }
}

/// The disposition of a requested record: its first exclusion, or `Run`.
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

/// Build the run's list from the leaf's actions, expanding each inclusion the
/// run reaches, and set every record's disposition.
///
/// Expansion and selection share one pass: an inclusion is opened only if the
/// selection admits it, and its records are then selected like any other. An
/// unreadable inclusion manifest, or a target naming a record the command
/// cannot run, fails here, before any action runs.
///
/// `context` must hold this command's materializations. `variables` is the
/// run's set, used for every record except those whose inclusion derived a
/// [scope](RunRecord::scope) of its own; `host` backs each scope's bindings.
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
        // A target naming a record the command cannot run is an invocation
        // error, raised before anything is prepared or run.
        if target_names_record && let Some(error) = selection.refusal(&record.action) {
            return Err(error);
        }

        // Whether the run intends to read the inclusion's manifest; an exclusion
        // or a missing materialization can still leave it unread. A target
        // inside it, like `apply-action --id corp.zshrc`, opens it without
        // naming the inclusion.
        let should_open_inclusion = match &record.action {
            Action::IncludeRemote(inclusion) => {
                target_names_record || selection.reaches_into(inclusion.id.as_ref())
            }
            _ => false,
        };

        if target_names_record || should_open_inclusion {
            // Decided in the leaf's scope: an inclusion's `vars` apply only to the
            // records it contributes, not to its own condition.
            record.disposition = disposition(&record, selection, &bindings);
        }

        let Action::IncludeRemote(declaration) = &record.action else {
            run_list.records.push(record);
            continue;
        };
        // Owns copies of the declaration's fields, since `record` moves into the
        // list below.
        let inclusion = Inclusion::at(declaration, index + 1);
        // Unread when not requested, excluded, closed by the remote's condition
        // (which becomes the record's exclusion), or not materialized.
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
        // Derived, and reported, only for an opened inclusion.
        let scope = inclusion_scope(
            &remote_vars,
            &declaration.vars,
            inclusion.label(),
            variables,
            context.reporter(),
        );
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
            // Targeting the inclusion targets every record it contributed; a
            // qualified address targets one.
            let target_names_contributed =
                run_list.match_and_record_target(&contributed, selection);
            if target_names_record || target_names_contributed {
                // The inclusion's filters come first and bypass the selection,
                // so no command waives them. A filtered-out record's condition
                // is never evaluated.
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

/// The scope for one opened inclusion's records: the run's set with the
/// remote's `[vars]` below the leaf's and the inclusion's `vars` above them.
///
/// Returns the run's set when both layers are empty; otherwise reports the
/// derived scope at `-vv`.
fn inclusion_scope(
    remote: &BTreeMap<VarName, String>,
    overrides: &BTreeMap<VarName, String>,
    label: &str,
    run: &Rc<VarSet>,
    reporter: &Reporter,
) -> Rc<VarSet> {
    // Empty layers change nothing.
    if remote.is_empty() && overrides.is_empty() {
        return Rc::clone(run);
    }
    let scope = Rc::new(run.with_inclusion(remote, overrides, label));
    scope.report_inclusion(reporter);
    scope
}
