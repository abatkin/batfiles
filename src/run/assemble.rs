//! Build the run's list from the leaf's actions, expanding each inclusion the
//! run reaches, and select every record in the same pass.

use std::collections::BTreeMap;
use std::rc::Rc;

use super::compose::{IncludedRecord, InclusionContents, compose, not_selected};
use super::record::{OpenedInclusion, RunList, RunRecord, TopLevelRecord};
use crate::error::Error;
use crate::manifest::actions::Action;
use crate::manifest::condition::{Bindings, Exclusion, HostNamespaces};
use crate::manifest::item::ItemId;
use crate::manifest::remote::{Remote, Tree};
use crate::manifest::template::TemplateString;
use crate::output::Reporter;
use crate::run::context::RunContext;
use crate::run::inclusion::{self, Inclusion};
use crate::run::selection::{Disposition, Selection};
use crate::vars::dynamic::{DynamicVarResolver, ManifestSource};
use crate::vars::name::VarName;
use crate::vars::scope::{VarScope, VarValue};

/// Assemble and select run records, expanding only admitted inclusions. Invalid targets,
/// unreadable included manifests, and cache read failures return errors before any action
/// executes.
///
/// `context` must contain this command's materializations. Leaf records use `run_scope`;
/// included records use a scope derived from it. `host` supplies captured host inputs, and
/// `dynamic` resolves each opened manifest's dynamic variables. `remotes`, the leaf's
/// declarations, decide which checkouts are verified before they are opened.
pub(super) fn assemble(
    actions: Vec<Action>,
    remotes: &BTreeMap<ItemId, Remote>,
    selection: &Selection<'_>,
    context: &RunContext<'_>,
    run_scope: &Rc<VarScope>,
    host: &HostNamespaces,
    dynamic: &mut DynamicVarResolver<'_>,
) -> Result<RunList, Error> {
    let bindings = Bindings::new(run_scope, host);
    let mut run_list = RunList {
        top_level: Vec::with_capacity(actions.len()),
        target_found: false,
    };

    for (index, action) in actions.into_iter().enumerate() {
        let mut record = RunRecord::leaf(action, index + 1);

        let target_names_record = match_and_record_target(&mut run_list, &record, selection);
        if target_names_record && let Some(error) = selection.refusal(&record.action) {
            return Err(error);
        }

        let Action::IncludeRemote(declaration) = &record.action else {
            decide(&mut record, target_names_record, None, selection, &bindings)?;
            run_list.top_level.push(TopLevelRecord::Action(record));
            continue;
        };
        let inclusion = Inclusion::at(declaration, index + 1);
        // A qualified target opens its inclusion without bypassing the inclusion's exclusions.
        let should_open_inclusion =
            target_names_record || selection.reaches_into(record.address.as_ref());
        if should_open_inclusion {
            let remote_exclusion = context.excluded_remote(inclusion.remote());
            let exclusion =
                inclusion.exclusion(record.subject(), selection, &bindings, remote_exclusion)?;
            let disposition = Disposition::from_exclusion(exclusion);
            record.disposition = if target_names_record {
                disposition
            } else {
                disposition.in_part()
            };
        }
        let included = if record.disposition.is_allowed() {
            let included = inclusion.manifest(context.repository().path(), remotes, &bindings)?;
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
            run_list.top_level.push(TopLevelRecord::Inclusion {
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
        let materialized = context.materialization(inclusion.remote());
        let source = ManifestSource {
            tree: Tree::Remote(inclusion.remote()),
            root: &materialized,
        };
        let remote_vars = dynamic.layer(&remote_vars, &source, &BTreeMap::new())?;
        let remote_env = remotes
            .get(inclusion.remote())
            .filter(|remote| !remote.allows_env_templates())
            .map(|_| inclusion.remote());
        let scope = inclusion_scope(
            &remote_vars,
            remote_env,
            &declaration.vars,
            inclusion.label(),
            run_scope,
            host,
            context.reporter(),
        )?;
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
            let commands_refused = match &contributed.action {
                Action::Run(run) if included_by_filter => {
                    if run.trigger.keeps_state() && !inclusion.is_named() {
                        return Err(Error::StatefulRunInUnnamedInclusion {
                            record: contributed.heading.clone(),
                            inclusion: inclusion.label().to_owned(),
                        });
                    }
                    !remotes
                        .get(inclusion.remote())
                        .is_some_and(Remote::allows_commands)
                }
                _ => false,
            };
            // `apply-action` naming the record does not waive the permission.
            if commands_refused
                && contributed
                    .address
                    .as_ref()
                    .is_some_and(|address| selection.names(address))
            {
                return Err(Error::CommandsNotAllowed {
                    record: contributed.heading.clone(),
                    remote: inclusion.remote().clone(),
                });
            }
            let filtered_out = if !included_by_filter {
                Some(not_selected(&inclusion))
            } else if commands_refused {
                Some(commands_not_allowed(inclusion.remote()))
            } else {
                None
            };
            let inclusion_allowed_whole = matches!(record.disposition, Disposition::Allowed);
            decide(
                &mut contributed,
                inclusion_allowed_whole || target_names_contributed,
                filtered_out,
                selection,
                &scoped,
            )?;
            records.push(contributed);
        }
        run_list.top_level.push(TopLevelRecord::Inclusion {
            record,
            inclusion,
            opened: Some(OpenedInclusion { scope, records }),
        });
    }

    Ok(run_list)
}

/// The exclusion for a contributed `run` action whose remote does not set `allow-commands`.
fn commands_not_allowed(remote: &ItemId) -> Exclusion {
    Exclusion::NotPermitted(format!(
        "remote `{remote}` does not set `allow-commands = true`, so the `run` actions it \
         contributes do not run"
    ))
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
///
/// Fails if the condition reads variables that refer to each other in a cycle.
fn decide(
    record: &mut RunRecord,
    requested: bool,
    filtered_out: Option<Exclusion>,
    selection: &Selection<'_>,
    bindings: &Bindings<'_>,
) -> Result<(), Error> {
    let reached = matches!(record.action, Action::GitCloneList(_))
        && selection.reaches_into_list(record.address.as_ref());
    if !requested && !reached {
        return Ok(());
    }
    record.disposition = match filtered_out {
        Some(exclusion) => Disposition::Excluded(exclusion),
        None if requested => {
            Disposition::from_exclusion(selection.exclusion(record.subject(), bindings)?)
        }
        None => Disposition::from_exclusion(
            selection.exclusion_without_waivers(record.subject(), bindings)?,
        )
        .in_part(),
    };
    Ok(())
}

/// The scope for one opened inclusion's records: the run's set with the
/// remote's `[vars]` below the leaf's and the inclusion's `vars` above them,
/// evaluated in the run's scope against `host`. `remote_env` is the remote
/// when its templates may not read `env`.
///
/// Returns the run's set when both layers are empty; otherwise reports the
/// derived scope at `-vv`. Fails on a cycle among the variables an override, or
/// the report, reads.
fn inclusion_scope(
    remote: &BTreeMap<VarName, VarValue>,
    remote_env: Option<&ItemId>,
    overrides: &BTreeMap<VarName, TemplateString>,
    label: &str,
    run: &Rc<VarScope>,
    host: &HostNamespaces,
    reporter: &Reporter,
) -> Result<Rc<VarScope>, Error> {
    if remote.is_empty() && overrides.is_empty() {
        return Ok(Rc::clone(run));
    }
    let scope = Rc::new(run.with_inclusion(remote, remote_env, overrides, label, host)?);
    scope.report_inclusion(host, reporter)?;
    Ok(scope)
}
