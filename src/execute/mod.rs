//! Select, prepare, and execute manifest actions for sync and apply commands.

mod assemble;
pub(crate) mod inclusion;
pub(crate) mod record;

use std::rc::Rc;

use self::assemble::assemble;
use self::record::{Disposition, RunList, RunRecord};
use crate::action::{self, Executable, RunContext};
use crate::bootstrap::Bootstrap;
use crate::clone_list::PreparedList;
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

/// What the action loop does with one record, with its inputs already read.
///
/// Only [`prepare`] builds steps, one per record in declaration order. A clone
/// list becomes a [`Run`](Self::Run) only once it has been read.
enum Step<'a> {
    /// Nothing: the record was not requested.
    Skip,
    /// Report the exclusion under the record's heading.
    Report(&'a RunRecord, &'a Exclusion),
    /// Report the record's heading and execute nothing: an admitted inclusion,
    /// whose records assembly already placed after it.
    Heading(&'a RunRecord),
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
            Disposition::Run if matches!(record.action, Action::IncludeRemote(_)) => {
                Ok(Step::Heading(record))
            }
            Disposition::Run => Ok(Step::Run(
                record,
                executable(record, context, variables, host)?,
            )),
        })
        .collect()
}

/// What executing `record` works from.
///
/// Only a clone list reads anything: its list, from the same tree as the
/// record's other paths, with entry conditions decided in the record's
/// [scope](RunRecord::scope). Every other record executes from its declaration
/// and cannot fail here.
fn executable<'a>(
    record: &'a RunRecord,
    context: &RunContext<'_>,
    variables: &Rc<VarSet>,
    host: &HostNamespaces,
) -> Result<Executable<'a>, Error> {
    let Action::GitCloneList(list) = &record.action else {
        return Ok(Executable::Declared(&record.action));
    };
    let path = context.source(record.remote(), &list.source)?;
    let bindings = Bindings::new(record.scope(variables), host);
    Ok(Executable::CloneList(PreparedList::prepare(
        list, &path, &bindings,
    )?))
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
/// Returns the number of records dispatched, which never includes an
/// inclusion. A record counts whether or not it changed anything, including in
/// a dry run.
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
            Step::Heading(record) => reporter.detail(1, &record.heading),
            Step::Run(record, executable) => {
                reporter.detail(1, &record.heading);
                action::run(executable, record.remote(), &context)?;
                processed_action_count += 1;
            }
        }
    }

    if !target_found && let Some(error) = selection.unresolved(invocation.roots.manifest()) {
        return Err(error);
    }
    Ok(processed_action_count)
}
