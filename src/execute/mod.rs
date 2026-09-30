//! Select, prepare, and execute manifest actions for sync and apply commands.

mod assemble;
mod inclusion;
pub(crate) mod record;

use std::rc::Rc;

use self::assemble::assemble;
use self::record::{Disposition, LeafEntry, RunList, RunRecord};
use crate::action::{self, DestinationOptions, Executable, RunContext};
use crate::bootstrap::BootstrapDecisions;
use crate::clone_list::PreparedList;
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::DisabledItems;
use crate::dynamic::{CachePolicy, DynamicVarResolver};
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
    /// Whether to refresh dynamic variables regardless of cache freshness.
    pub refresh_vars: bool,
    /// Content refresh and destination conflict settings.
    pub destination_options: DestinationOptions,
    pub env: &'a Environment,
    pub reporter: &'a Reporter,
}

/// `sync`: bring the home directory to the state the whole manifest describes.
/// `refresh_remotes` fetches every file and archive remote again, current or
/// not.
pub(crate) fn sync(
    invocation: &Invocation<'_>,
    skip_actions: &[String],
    skip_groups: &[String],
    refresh_remotes: bool,
) -> Result<(), Error> {
    run(
        Target::Everything,
        Skips {
            actions: skip_actions,
            groups: skip_groups,
        },
        RunKind::Sync { refresh_remotes },
        invocation,
    )?;
    Ok(())
}

/// `clone`'s synchronization: a `sync` that first adopts the cloned manifest's
/// bootstrap policy.
pub(crate) fn bootstrap(
    invocation: &Invocation<'_>,
    bootstrap: &BootstrapDecisions,
    skip_actions: &[String],
    skip_groups: &[String],
) -> Result<(), Error> {
    run(
        Target::Everything,
        Skips {
            actions: skip_actions,
            groups: skip_groups,
        },
        RunKind::Clone(bootstrap),
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
enum RunKind<'a> {
    /// `sync`: materializes, fetching every file and archive remote again
    /// where `refresh_remotes` says to; adopts nothing.
    Sync { refresh_remotes: bool },
    /// `clone`: adopts the bootstrap policy, then materializes.
    Clone(&'a BootstrapDecisions),
    /// `apply-action` and `apply-group`: use existing materializations and fetch
    /// nothing.
    Apply,
}

impl RunKind<'_> {
    /// Whether the command materializes declared remotes before assembly.
    fn materializes(&self) -> bool {
        !matches!(self, Self::Apply)
    }

    /// Whether materializing fetches file and archive remotes that are current.
    fn refreshes_remotes(&self) -> bool {
        matches!(
            self,
            Self::Sync {
                refresh_remotes: true
            }
        )
    }
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(invocation: &Invocation<'_>, id: &str) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    // Direct application still honors inclusion and remote conditions.
    let processed_action_count = run(
        Target::Action(&id),
        Skips {
            actions: &[],
            groups: &[],
        },
        RunKind::Apply,
        invocation,
    )?;

    if processed_action_count == 0 {
        // The target exists; only an inclusion filter can prevent its dispatch here.
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
    let processed_action_count = run(
        Target::Group(&group),
        Skips {
            actions: skip_actions,
            groups: &[],
        },
        RunKind::Apply,
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

/// A prepared execution step. Clone lists in `Run` have already been read and validated.
enum Step<'a> {
    /// Nothing: the record was not requested.
    NotRequested,
    /// Report the exclusion under the record's heading.
    ReportExclusion(&'a RunRecord, &'a Exclusion),
    /// Execute the record.
    Run(&'a RunRecord, Executable<'a>),
}

/// One record of the leaf manifest, prepared.
enum PreparedLeafEntry<'a> {
    /// Any record but an admitted inclusion.
    Step(Step<'a>),
    /// An admitted inclusion's heading and ordered steps, with sources resolved against
    /// `remote`. Steps are empty if its manifest was unread or contributed no records.
    Inclusion {
        heading: &'a str,
        remote: &'a ItemId,
        steps: Vec<Step<'a>>,
    },
}

/// Prepare records in declaration order, reading selected, allowed clone lists once. Missing or
/// malformed lists fail before any action writes. Unrequested and excluded lists are not read.
///
/// Bootstrap adoption and remote materialization have already run; their writes are retained on
/// failure. Files that an earlier action would create are not available yet.
fn prepare<'a>(
    run_list: &'a RunList,
    context: &RunContext<'_>,
    scope: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<Vec<PreparedLeafEntry<'a>>, Error> {
    run_list
        .entries
        .iter()
        .map(|entry| match entry {
            LeafEntry::Inclusion {
                record,
                inclusion,
                opened,
            } if matches!(record.disposition, Disposition::Allowed) => {
                let remote = inclusion.remote();
                let steps = opened
                    .iter()
                    .flat_map(|opened| {
                        opened
                            .records
                            .iter()
                            .map(|it| step(it, Some(remote), &opened.scope, context, host))
                    })
                    .collect::<Result<_, _>>()?;
                Ok(PreparedLeafEntry::Inclusion {
                    heading: &record.heading,
                    remote,
                    steps,
                })
            }
            LeafEntry::Action(record) | LeafEntry::Inclusion { record, .. } => Ok(
                PreparedLeafEntry::Step(step(record, None, scope, context, host)?),
            ),
        })
        .collect()
}

/// The step for one record, from `remote`'s tree (`None`: the leaf's) and
/// decided in `scope`.
fn step<'a>(
    record: &'a RunRecord,
    remote: Option<&ItemId>,
    scope: &Rc<VarSet>,
    context: &RunContext<'_>,
    host: &HostNamespaces,
) -> Result<Step<'a>, Error> {
    Ok(match &record.disposition {
        Disposition::NotRequested => Step::NotRequested,
        Disposition::Excluded(exclusion) => Step::ReportExclusion(record, exclusion),
        Disposition::Allowed => {
            Step::Run(record, executable(record, remote, scope, context, host)?)
        }
    })
}

/// Prepare execution inputs for `record`. Clone lists are read from `remote`'s tree (`None` for
/// the leaf), with conditions evaluated in `scope`; other actions use their declaration
/// directly.
fn executable<'a>(
    record: &'a RunRecord,
    remote: Option<&ItemId>,
    scope: &Rc<VarSet>,
    context: &RunContext<'_>,
    host: &HostNamespaces,
) -> Result<Executable<'a>, Error> {
    let Action::GitCloneList(list) = &record.action else {
        return Ok(Executable::Declared(&record.action));
    };
    let path = context.source(remote, &list.source)?;
    let bindings = Bindings::new(scope, host);
    Ok(Executable::CloneList(PreparedList::prepare(
        list, &path, &bindings,
    )?))
}

/// Carry out one step, whose record is read from `remote`'s tree (`None`: the
/// leaf's). Returns whether the record was dispatched.
fn execute(
    step: &Step<'_>,
    remote: Option<&ItemId>,
    context: &RunContext<'_>,
) -> Result<bool, Error> {
    match step {
        Step::NotRequested => Ok(false),
        Step::ReportExclusion(record, exclusion) => {
            exclusion.report_heading(context.reporter(), &record.heading);
            Ok(false)
        }
        Step::Run(record, executable) => {
            context.reporter().detail(1, &record.heading);
            action::run(executable, remote, context)?;
            Ok(true)
        }
    }
}

/// Load inputs, adopt bootstrap state, materialize remotes, assemble and select records,
/// prepare clone lists, and execute actions in declaration order. Return the number of
/// dispatched actions, including unchanged actions and dry-run dispatches, but excluding
/// inclusions.
///
/// Dynamic-variable captures and bootstrap state may be saved before selection or preparation
/// fails. Leaf variables resolve before bootstrap adoption; allowed remote variables resolve
/// when their inclusions open.
fn run(
    target: Target<'_>,
    skips: Skips<'_>,
    kind: RunKind<'_>,
    invocation: &Invocation<'_>,
) -> Result<usize, Error> {
    let reporter = invocation.reporter;
    let manifest = Manifest::load(&invocation.roots.manifest_path())?;
    let policy = if invocation.refresh_vars {
        CachePolicy::Force
    } else {
        CachePolicy::Auto
    };
    let mut dynamic =
        DynamicVarResolver::for_run(&invocation.roots.state, policy, &manifest.remotes, reporter);
    let scope = Rc::new(VarSet::resolve(
        &manifest.vars,
        &invocation.roots.batfiles_repo,
        &invocation.roots.state,
        invocation.env,
        invocation.vars,
        &mut dynamic,
        reporter,
    )?);
    // Save leaf captures even if a later phase fails.
    dynamic.save();
    scope.report(reporter);
    let host = HostNamespaces::capture(invocation.env);
    let bindings = Bindings::new(&scope, &host);

    // Bootstrap defaults come only from the leaf manifest.
    if let RunKind::Clone(bootstrap) = kind {
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
        DisabledItems::load(&invocation.roots.state.disabled_path())?,
        reporter,
    );

    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    let context = RunContext::new(
        invocation.roots,
        invocation.mode,
        excluded_remotes,
        invocation.destination_options,
        reporter,
    )?;

    // Materializations must exist before opening inclusions and preparing clone lists.
    if kind.materializes() {
        remotes::materialize(&manifest.remotes, &context, kind.refreshes_remotes())?;
    }

    let run_list = assemble(
        manifest.actions,
        &selection,
        &context,
        &scope,
        &host,
        &mut dynamic,
    );
    // Save earlier inclusions' captures even if a later inclusion failed.
    dynamic.save();
    let run_list = run_list?;
    // Included addresses are known only after assembly.
    run_list.warn_unmatched(&selection, reporter);

    let plan = prepare(&run_list, &context, &scope, &host)?;
    let mut processed_action_count = 0;

    for prepared in &plan {
        match prepared {
            PreparedLeafEntry::Step(step) => {
                processed_action_count += usize::from(execute(step, None, &context)?);
            }
            PreparedLeafEntry::Inclusion {
                heading,
                remote,
                steps,
            } => {
                reporter.detail(1, heading);
                for step in steps {
                    processed_action_count += usize::from(execute(step, Some(remote), &context)?);
                }
            }
        }
    }

    if let Some(error) =
        run_list.unmatched_target_error(&selection, invocation.roots.manifest_path())
    {
        return Err(error);
    }
    Ok(processed_action_count)
}
