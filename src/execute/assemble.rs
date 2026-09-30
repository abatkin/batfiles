//! Build the run's list from the leaf's actions, expanding each inclusion the
//! run reaches, and select every record in the same pass.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::inclusion::{IncludedRecord, InclusionContents, compose, not_selected};
use super::record::{Disposition, LeafEntry, OpenedInclusion, RunList, RunRecord};
use crate::action::RunContext;
use crate::condition::{Bindings, HostNamespaces};
use crate::dynamic::{DynamicVarResolver, ManifestSource};
use crate::error::Error;
use crate::inclusion::{self, Inclusion};
use crate::manifest::action::Action;
use crate::output::Reporter;
use crate::selection::Selection;
use crate::var::VarName;
use crate::var_set::{VarSet, VarValue};

/// Assemble and select run records, expanding only admitted inclusions. Invalid targets,
/// unreadable included manifests, and cache read failures return errors before any action
/// executes.
///
/// `context` must contain this command's materializations. Leaf records use `run_scope`;
/// included records use a scope derived from it. `host` supplies captured host inputs, and
/// `dynamic` resolves each opened manifest's dynamic variables.
pub(super) fn assemble(
    actions: Vec<Action>,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
    run_scope: &Rc<VarSet>,
    host: &HostNamespaces,
    dynamic: &mut DynamicVarResolver<'_>,
) -> Result<RunList, Error> {
    let bindings = Bindings::new(run_scope, host);
    let mut run_list = RunList {
        entries: Vec::with_capacity(actions.len()),
        target_found: false,
    };

    for (index, action) in actions.into_iter().enumerate() {
        let mut record = RunRecord::leaf(action, index + 1);

        let target_names_record = match_and_record_target(&mut run_list, &record, selection);
        if target_names_record && let Some(error) = selection.refusal(&record.action) {
            return Err(error);
        }

        let Action::IncludeRemote(declaration) = &record.action else {
            if target_names_record {
                record.disposition = disposition(&record, selection, &bindings);
            }
            run_list.entries.push(LeafEntry::Action(record));
            continue;
        };
        let inclusion = Inclusion::at(declaration, index + 1);
        // A qualified target opens its inclusion without bypassing the inclusion's exclusions.
        let should_open_inclusion = target_names_record || selection.reaches_into(inclusion.id());
        if should_open_inclusion {
            let remote_exclusion = context.excluded_remote(inclusion.remote());
            let exclusion =
                inclusion.exclusion(record.subject(), selection, &bindings, remote_exclusion);
            record.disposition = match exclusion {
                Some(exclusion) => Disposition::Excluded(exclusion),
                None => Disposition::Allowed,
            };
        }
        let included = match record.disposition {
            Disposition::Allowed => {
                let included = inclusion.manifest(context.repository().path())?;
                if included.is_none() {
                    inclusion::warn_not_materialized(
                        inclusion.remote(),
                        &context.materialization(inclusion.remote()),
                        context.reporter(),
                    );
                }
                included
            }
            _ => None,
        };
        let Some(included) = included else {
            run_list.entries.push(LeafEntry::Inclusion {
                record,
                inclusion,
                opened: None,
            });
            continue;
        };
        let InclusionContents {
            vars: remote_vars,
            records: included_records,
        } = compose(&inclusion, declaration, included, context.reporter());
        // Resolve remote variables only after the inclusion's gate passes in the leaf scope.
        let tree = context.materialization(inclusion.remote());
        let source = ManifestSource {
            remote: Some(inclusion.remote()),
            root: &tree,
        };
        let remote_vars = dynamic.layer(&remote_vars, &source, &BTreeMap::new())?;
        let scope = inclusion_scope(
            &remote_vars,
            &declaration.vars,
            inclusion.label(),
            run_scope,
            context.reporter(),
        );
        let scoped = Bindings::new(&scope, host);

        let mut records = Vec::with_capacity(included_records.len());
        for IncludedRecord {
            number,
            action,
            included_by_filter,
        } in included_records
        {
            let mut contributed = RunRecord::contributed(action, number, inclusion.contributor());
            let target_names_contributed =
                match_and_record_target(&mut run_list, &contributed, selection);
            if target_names_record || target_names_contributed {
                // Inclusion filters cannot be waived and must suppress condition evaluation for
                // rejected records.
                contributed.disposition = if included_by_filter {
                    disposition(&contributed, selection, &scoped)
                } else {
                    Disposition::Excluded(not_selected(&inclusion))
                };
            }
            records.push(contributed);
        }
        run_list.entries.push(LeafEntry::Inclusion {
            record,
            inclusion,
            opened: Some(OpenedInclusion { scope, records }),
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
    let named = selection.wants(record.subject());
    run_list.target_found |= named;
    named
}

/// The disposition of a requested record: its first exclusion, or `Allowed`.
fn disposition(
    record: &RunRecord,
    selection: &Selection<'_>,
    bindings: &Bindings<'_>,
) -> Disposition {
    match selection.exclusion(record.subject(), bindings) {
        Some(exclusion) => Disposition::Excluded(exclusion),
        None => Disposition::Allowed,
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
    if remote.is_empty() && overrides.is_empty() {
        return Rc::clone(run);
    }
    let scope = Rc::new(run.with_inclusion(remote, overrides, label));
    scope.report_inclusion(reporter);
    scope
}
