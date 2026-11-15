//! The pipeline `sync`, `clone`'s bootstrap, and the apply commands share:
//! bootstrap adoption, selection, opening inclusions, assembling the run's list,
//! and executing it through the [`RunContext`] actions read, under the run lock.

mod assemble;
pub(crate) mod bootstrap;
pub(crate) mod clone_list;
mod compose;
pub(crate) mod context;
pub(crate) mod inclusion;
pub(crate) mod lock;
pub(crate) mod record;
pub(crate) mod selection;

use std::rc::Rc;

use self::assemble::assemble;
use self::record::{RunList, RunRecord, TopLevelRecord};
use crate::action::{self, PreparedAction};
use crate::env::Environment;
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::actions::Action;
use crate::manifest::condition::{Bindings, Exclusion, HostNamespaces};
use crate::manifest::disabled::DisabledItems;
use crate::manifest::item::{ItemAddress, ItemId};
use crate::manifest::remote::Tree;
use crate::output::Reporter;
use crate::run::bootstrap::BootstrapDecisions;
use crate::run::clone_list::PreparedList;
use crate::run::context::{DestinationOptions, RunContext};
use crate::run::selection::{Disposition, Selection, Target};
use crate::source::remotes;
use crate::vars::dynamic::{CachePolicy, DynamicVarResolver};
use crate::vars::name::VarName;
use crate::vars::set::VarSet;

/// What a command hands the pipeline: resolved roots, mode, `--var` values, and destination
/// options, with the session's environment and reporter. Nothing in it depends on the manifest;
/// the pipeline derives a [`RunContext`] from it after loading one.
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

/// `sync`, and `clone`'s synchronization: bring the home directory to the state the whole
/// manifest describes. `refresh_remotes` fetches every file and archive remote again, current or
/// not. `bootstrap` first adopts the leaf's bootstrap policy, saving it unless the run is a dry
/// run, which plans with it instead.
pub(crate) fn sync(
    invocation: &Invocation<'_>,
    skip_actions: &[String],
    skip_groups: &[String],
    refresh_remotes: bool,
    bootstrap: Option<&BootstrapDecisions>,
) -> Result<(), Error> {
    run_pipeline(
        Target::Everything,
        Skips {
            actions: skip_actions,
            groups: skip_groups,
        },
        RunKind::Sync {
            refresh_remotes,
            bootstrap,
        },
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
    /// `sync` and `clone`: adopts the bootstrap policy where there is one, then
    /// materializes, fetching every file and archive remote again where
    /// `refresh_remotes` says to.
    Sync {
        refresh_remotes: bool,
        bootstrap: Option<&'a BootstrapDecisions>,
    },
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
                refresh_remotes: true,
                ..
            }
        )
    }

    /// The bootstrap policy to adopt before reading disabled state, if any.
    fn bootstrap(&self) -> Option<&BootstrapDecisions> {
        match self {
            Self::Sync { bootstrap, .. } => *bootstrap,
            Self::Apply => None,
        }
    }
}

/// `apply-action`: carry out the one record named by `id`, whatever the
/// machine-local lists say about it.
pub(crate) fn apply_action(invocation: &Invocation<'_>, id: &str) -> Result<(), Error> {
    let id = ItemAddress::try_from(id.to_owned())?;
    // Direct application still honors inclusion and remote conditions.
    let processed_action_count = run_pipeline(
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
    let processed_action_count = run_pipeline(
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
    Run(&'a RunRecord, PreparedAction<'a>),
}

/// One record of the leaf manifest, prepared.
enum PlannedRecord<'a> {
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

/// Prepare records in declaration order, reading each allowed clone list once and deciding its
/// entries against `selection`. Missing or malformed lists fail before any action writes.
/// Unrequested and excluded lists are not read.
///
/// Bootstrap adoption and remote materialization have already run; their writes are retained on
/// failure. Files that an earlier action would create are not available yet.
fn prepare<'a>(
    run_list: &'a RunList,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
    scope: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<Vec<PlannedRecord<'a>>, Error> {
    run_list
        .top_level
        .iter()
        .map(|top| match top {
            TopLevelRecord::Inclusion {
                record,
                inclusion,
                opened,
            } if record.disposition.is_allowed() => {
                let remote = inclusion.remote();
                let steps = opened
                    .iter()
                    .flat_map(|opened| {
                        opened.records.iter().map(|included| {
                            step(
                                included,
                                Tree::Remote(remote),
                                selection,
                                &opened.scope,
                                context,
                                host,
                            )
                        })
                    })
                    .collect::<Result<_, _>>()?;
                Ok(PlannedRecord::Inclusion {
                    heading: &record.heading,
                    remote,
                    steps,
                })
            }
            TopLevelRecord::Action(record) | TopLevelRecord::Inclusion { record, .. } => Ok(
                PlannedRecord::Step(step(record, Tree::Leaf, selection, scope, context, host)?),
            ),
        })
        .collect()
}

/// The step for one record, read from `tree` and decided in `scope`.
fn step<'a>(
    record: &'a RunRecord,
    tree: Tree<&ItemId>,
    selection: &Selection<'_>,
    scope: &Rc<VarSet>,
    context: &RunContext<'_>,
    host: &HostNamespaces,
) -> Result<Step<'a>, Error> {
    Ok(match &record.disposition {
        Disposition::NotRequested => Step::NotRequested,
        Disposition::Excluded(exclusion) => Step::ReportExclusion(record, exclusion),
        Disposition::Allowed | Disposition::AllowedInPart => Step::Run(
            record,
            prepare_action(record, tree, selection, scope, context, host)?,
        ),
    })
}

/// Prepare execution inputs for `record`. Clone lists are read from `tree`, with their entries
/// decided by `selection` in `scope`; other actions use their declaration directly.
fn prepare_action<'a>(
    record: &'a RunRecord,
    tree: Tree<&ItemId>,
    selection: &Selection<'_>,
    scope: &Rc<VarSet>,
    context: &RunContext<'_>,
    host: &HostNamespaces,
) -> Result<PreparedAction<'a>, Error> {
    let Action::GitCloneList(list) = &record.action else {
        return Ok(PreparedAction::Declared(&record.action));
    };
    let path = context.source(tree, &list.source)?;
    let bindings = Bindings::new(scope, host);
    Ok(PreparedAction::CloneList(PreparedList::prepare(
        list,
        &path,
        record.address.as_ref(),
        |entry| selection.entry_disposition(entry, &record.disposition, &bindings),
    )?))
}

/// The address of every addressable entry in the clone lists `plan` prepared.
fn entry_addresses<'a>(plan: &'a [PlannedRecord<'_>]) -> Vec<&'a ItemAddress> {
    plan.iter()
        .flat_map(|planned| match planned {
            PlannedRecord::Step(step) => std::slice::from_ref(step),
            PlannedRecord::Inclusion { steps, .. } => steps.as_slice(),
        })
        .filter_map(|step| match step {
            Step::Run(_, PreparedAction::CloneList(list)) => Some(list.entries()),
            _ => None,
        })
        .flatten()
        .filter_map(|entry| entry.address.as_ref())
        .collect()
}

/// Carry out one step, whose record is read from `tree`. Returns whether the record was
/// dispatched.
fn execute(step: &Step<'_>, tree: Tree<&ItemId>, context: &RunContext<'_>) -> Result<bool, Error> {
    match step {
        Step::NotRequested => Ok(false),
        Step::ReportExclusion(record, exclusion) => {
            exclusion.report_heading(context.reporter(), &record.heading);
            Ok(false)
        }
        Step::Run(record, prepared) => {
            context.reporter().detail(1, &record.heading);
            action::dispatch(prepared, tree, context)?;
            Ok(true)
        }
    }
}

/// Load inputs, adopt bootstrap state, materialize remotes, assemble and select records,
/// prepare clone lists, check the target, and execute actions in declaration order. Return the number of
/// dispatched actions, including unchanged actions and dry-run dispatches, but excluding
/// inclusions.
///
/// Dynamic-variable captures and bootstrap state may be saved before selection or preparation
/// fails. Leaf variables resolve before bootstrap adoption; allowed remote variables resolve
/// when their inclusions open.
fn run_pipeline(
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
    let disabled = match kind.bootstrap() {
        Some(bootstrap) => bootstrap.adopt(
            &manifest.default_disabled,
            &bindings,
            &invocation.roots.state,
            invocation.mode,
            reporter,
        )?,
        None => DisabledItems::load(&invocation.roots.state.disabled_path())?,
    };

    let selection = Selection::new(
        target,
        skips.actions,
        skips.groups,
        invocation.env,
        disabled,
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
        context.materialize(&manifest.remotes, kind.refreshes_remotes())?;
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

    let plan = prepare(&run_list, &selection, &context, &scope, &host)?;
    // Included addresses are known only after assembly, and entry addresses after preparation.
    let list_entries = entry_addresses(&plan);
    run_list.warn_unmatched(&selection, &list_entries, reporter);
    let entry_found = list_entries.iter().any(|address| selection.names(address));
    if let Some(error) =
        run_list.unmatched_target_error(&selection, invocation.roots.manifest_path(), entry_found)
    {
        return Err(error);
    }

    let mut processed_action_count = 0;

    for prepared in &plan {
        match prepared {
            PlannedRecord::Step(step) => {
                processed_action_count += usize::from(execute(step, Tree::Leaf, &context)?);
            }
            PlannedRecord::Inclusion {
                heading,
                remote,
                steps,
            } => {
                reporter.detail(1, heading);
                for step in steps {
                    processed_action_count +=
                        usize::from(execute(step, Tree::Remote(remote), &context)?);
                }
            }
        }
    }
    Ok(processed_action_count)
}
