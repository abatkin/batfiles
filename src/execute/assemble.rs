//! Build the run's list from the leaf's actions, expanding each inclusion the
//! run reaches, and select every record in the same pass.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::inclusion::{IncludedRecord, InclusionContents, compose, not_selected};
use super::record::{LeafEntry, OpenedInclusion, RunList, RunRecord};
use crate::action::RunContext;
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::dynamic::{DynamicVarResolver, ManifestSource};
use crate::error::Error;
use crate::inclusion::{self, Inclusion};
use crate::manifest::action::Action;
use crate::output::Reporter;
use crate::selection::{Disposition, Selection};
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
            decide(&mut record, target_names_record, None, selection, &bindings);
            run_list.entries.push(LeafEntry::Action(record));
            continue;
        };
        let inclusion = Inclusion::at(declaration, index + 1);
        // A qualified target opens its inclusion without bypassing the inclusion's exclusions.
        let should_open_inclusion =
            target_names_record || selection.reaches_into(record.address.as_ref());
        if should_open_inclusion {
            let remote_exclusion = context.excluded_remote(inclusion.remote());
            let exclusion =
                inclusion.exclusion(record.subject(), selection, &bindings, remote_exclusion);
            let disposition = Disposition::from_exclusion(exclusion);
            record.disposition = if target_names_record {
                disposition
            } else {
                disposition.in_part()
            };
        }
        let included = if record.disposition.is_allowed() {
            let included = inclusion.manifest(context.repository().path())?;
            if included.is_none() {
                inclusion::warn_not_materialized(
                    inclusion.remote(),
                    &context.materialization(inclusion.remote()),
                    context.reporter(),
                );
            }
            included
        } else {
            None
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
            let filtered_out = (!included_by_filter).then(|| not_selected(&inclusion));
            let inclusion_allowed_whole = matches!(record.disposition, Disposition::Allowed);
            decide(
                &mut contributed,
                inclusion_allowed_whole || target_names_contributed,
                filtered_out,
                selection,
                &scoped,
            );
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

/// Decide a record that is not an inclusion. A `requested` record takes its first exclusion
/// under the target's waivers. A clone list the target only reaches into, for one of its
/// entries, is decided with nothing waived and allowed only in part. Anything else stays not
/// requested.
///
/// `filtered_out` is an inclusion's filters leaving the record out, which nothing waives and
/// which comes ahead of every other exclusion, so the record's condition is not evaluated.
fn decide(
    record: &mut RunRecord,
    requested: bool,
    filtered_out: Option<Exclusion>,
    selection: &Selection<'_>,
    bindings: &Bindings<'_>,
) {
    let reached = matches!(record.action, Action::GitCloneList(_))
        && selection.reaches_into_list(record.address.as_ref());
    if !requested && !reached {
        return;
    }
    record.disposition = match filtered_out {
        Some(exclusion) => Disposition::Excluded(exclusion),
        None if requested => {
            Disposition::from_exclusion(selection.exclusion(record.subject(), bindings))
        }
        None => Disposition::from_exclusion(
            selection.exclusion_without_waivers(record.subject(), bindings),
        )
        .in_part(),
    };
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
