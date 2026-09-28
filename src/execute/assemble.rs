//! Build the run's list from the leaf's actions, expanding each inclusion the
//! run reaches, and select every record in the same pass.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::inclusion::{IncludedAction, Inclusion, InclusionContents};
use super::record::{Disposition, Node, Opened, RunList, RunRecord};
use crate::action::RunContext;
use crate::condition::{Bindings, HostNamespaces};
use crate::dynamic::{DynamicVarResolver, ManifestSource};
use crate::error::Error;
use crate::manifest::action::Action;
use crate::output::Reporter;
use crate::selection::Selection;
use crate::var::VarName;
use crate::var_set::{VarSet, VarValue};

/// Build the run's list from the leaf's actions, expanding each inclusion the
/// run reaches, and set every record's disposition.
///
/// Expansion and selection share one pass: an inclusion is opened only if the
/// selection admits it, and the records it contributes are then selected like
/// any other. An unreadable inclusion manifest, or a target naming a record the
/// command cannot run, fails here, before any action runs.
///
/// `context` must hold this command's materializations. `variables` is the
/// run's set, used for every record except those an opened inclusion
/// contributed, which are decided in its [scope](Opened::scope); `host` backs
/// each scope's bindings. `dynamic` resolves the declarations of each manifest
/// an inclusion opens, in that remote's materialization; an unreadable cache
/// fails here.
pub(super) fn assemble(
    actions: Vec<Action>,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
    variables: &Rc<VarSet>,
    host: &HostNamespaces,
    dynamic: &mut DynamicVarResolver<'_>,
) -> Result<RunList, Error> {
    let bindings = Bindings::new(variables, host);
    let mut run_list = RunList {
        nodes: Vec::with_capacity(actions.len()),
        target_found: false,
    };

    for (index, action) in actions.into_iter().enumerate() {
        let mut record = RunRecord::leaf(action, index + 1);

        let target_names_record = match_and_record_target(&mut run_list, &record, selection);
        // A target naming a record the command cannot run is an invocation
        // error, raised before anything is prepared or run.
        if target_names_record && let Some(error) = selection.refusal(&record.action) {
            return Err(error);
        }

        let Action::IncludeRemote(declaration) = &record.action else {
            if target_names_record {
                record.disposition = disposition(&record, selection, &bindings);
            }
            run_list.nodes.push(Node::Action(record));
            continue;
        };
        let inclusion = Inclusion::at(declaration, index + 1);
        // Whether the run intends to read the inclusion's manifest; an exclusion
        // or a missing materialization can still leave it unread. A target
        // inside it, like `apply-action --id corp.zshrc`, opens it without
        // naming the inclusion, and so without waiving its exclusions.
        let should_open_inclusion = target_names_record || selection.reaches_into(inclusion.id());
        if should_open_inclusion {
            record.disposition = match inclusion.exclusion(&record, selection, &bindings, context) {
                Some(exclusion) => Disposition::Excluded(exclusion),
                None => Disposition::Run,
            };
        }
        // Unread when not requested, excluded, or not materialized.
        let contents = match record.disposition {
            Disposition::Run => inclusion.read(context)?,
            _ => None,
        };
        let Some(InclusionContents {
            vars: remote_vars,
            actions: included_actions,
        }) = contents
        else {
            run_list.nodes.push(Node::Inclusion {
                record,
                inclusion,
                opened: None,
            });
            continue;
        };
        // Resolved only for an opened inclusion, once its gates have been
        // decided in the leaf's scope.
        let tree = context.materialization(inclusion.remote());
        let source = ManifestSource {
            remote: Some(inclusion.remote()),
            root: &tree,
        };
        let remote_vars = dynamic.layer(&remote_vars, &source, &BTreeMap::new())?;
        // Derived, and reported, only for an opened inclusion.
        let scope = inclusion_scope(
            &remote_vars,
            &declaration.vars,
            inclusion.label(),
            variables,
            context.reporter(),
        );
        let scoped = Bindings::new(&scope, host);

        let mut records = Vec::with_capacity(included_actions.len());
        for IncludedAction {
            number,
            action,
            included_by_filter,
        } in included_actions
        {
            let mut contributed = RunRecord::contributed(action, number, inclusion.contributor());
            // Targeting the inclusion targets every record it contributed; a
            // qualified address targets one.
            let target_names_contributed =
                match_and_record_target(&mut run_list, &contributed, selection);
            if target_names_record || target_names_contributed {
                // The inclusion's filters come first and bypass the selection,
                // so no command waives them. A filtered-out record's condition
                // is never evaluated.
                contributed.disposition = if included_by_filter {
                    disposition(&contributed, selection, &scoped)
                } else {
                    Disposition::Excluded(inclusion.not_selected())
                };
            }
            records.push(contributed);
        }
        run_list.nodes.push(Node::Inclusion {
            record,
            inclusion,
            opened: Some(Opened { scope, records }),
        });
    }

    Ok(run_list)
}

/// Whether the target names `record`, noting in `run_list` that the target was
/// found.
fn match_and_record_target(
    run_list: &mut RunList,
    record: &RunRecord,
    selection: &Selection<'_>,
) -> bool {
    let named = selection.wants(record);
    run_list.target_found |= named;
    named
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

/// The scope for one opened inclusion's records: the run's set with the
/// remote's `[vars]` below the leaf's and the inclusion's `vars` above them.
///
/// Returns the run's set when both layers are empty; otherwise reports the
/// derived scope at `-vv`.
fn inclusion_scope(
    remote: &BTreeMap<VarName, VarValue>,
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
