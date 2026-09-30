//! `vars refresh`: run dynamic variables' commands whatever the cache holds,
//! for the leaf and for every remote in play on this machine. See
//! [`docs/cmdline.md`](../../docs/cmdline.md#vars-refresh).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use thiserror::Error as ThisError;

use super::{
    DynamicValueState, DynamicVarKey, DynamicVarResolver, ManifestSource, RefreshSelection,
};
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::DisabledItems;
use crate::env::Environment;
use crate::error::Error;
use crate::inclusion::{self, Inclusion};
use crate::item::{ItemAddress, ItemId};
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::Action;
use crate::manifest::vars::VarSpec;
use crate::output::Reporter;
use crate::paths;
use crate::remotes;
use crate::selection::{Selection, Subject};
use crate::var_set::VarSet;

/// Refresh the named dynamic variables, or all eligible declarations if `keys` is empty, and
/// save captured values.
///
/// Validate requested keys before running their commands. Remote validation may first run leaf
/// declarations to evaluate conditions; save those captures even if validation fails. Report
/// all invalid keys together. Forced command failures return an error after saving successful
/// captures.
pub(crate) fn run(
    keys: &[DynamicVarKey],
    roots: &Roots,
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest_path())?;
    let selection = RefreshSelection::from_keys(keys);
    refuse(
        selection
            .keys()
            .filter_map(|key| {
                declared_in_leaf(key, &manifest).map(|reason| UnrefreshableKey::of(key, reason))
            })
            .collect(),
    )?;

    let mut dynamic = DynamicVarResolver::for_refresh(
        &roots.state,
        selection.clone(),
        &manifest.remotes,
        reporter,
    );
    if selection.reaches_remotes() {
        refresh_with_remotes(manifest, &selection, roots, env, &mut dynamic, reporter)?;
    } else {
        let leaf = ManifestSource {
            remote: None,
            root: &roots.batfiles_repo,
        };
        dynamic.force(&manifest.vars, &leaf)?;
    }
    dynamic.save();
    report(&dynamic, reporter)
}

/// Resolve leaf variables, validate requested remote keys against admitted inclusions, and
/// refresh selected remote declarations.
fn refresh_with_remotes(
    manifest: Manifest,
    asked: &RefreshSelection,
    roots: &Roots,
    env: &Environment,
    dynamic: &mut DynamicVarResolver<'_>,
    reporter: &Reporter,
) -> Result<(), Error> {
    let variables = Rc::new(VarSet::resolve(
        &manifest.vars,
        &roots.batfiles_repo,
        &roots.state,
        env,
        &[],
        dynamic,
        reporter,
    )?);
    // Save leaf captures even if later reachability or key validation fails.
    dynamic.save();
    let host = HostNamespaces::capture(env);
    let bindings = Bindings::new(&variables, &host);
    let selection =
        Selection::without_run_skips(DisabledItems::load(&roots.state.disabled_path())?);
    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    let repository = paths::anchor(&roots.batfiles_repo)?;
    let reach = reachability(&manifest.actions, &selection, &bindings, &excluded_remotes);

    let wanted: BTreeSet<&ItemId> = match asked {
        RefreshSelection::All => reach.keys().collect(),
        RefreshSelection::Named(_) => asked.keys().filter_map(|key| key.remote.as_ref()).collect(),
    };

    let mut refusals = Vec::new();
    let mut opened = Vec::new();
    for remote in wanted {
        let reason = match reach.get(remote) {
            None => UnrefreshableReason::NotIncluded {
                remote: remote.clone(),
            },
            Some(Reach::Excluded(exclusions)) => {
                for (label, exclusion) in exclusions {
                    exclusion.report_heading(reporter, label);
                }
                UnrefreshableReason::NotInPlay {
                    remote: remote.clone(),
                    reasons: exclusions
                        .iter()
                        .map(|(label, exclusion)| format!("{label}: {}", exclusion.reason()))
                        .collect(),
                }
            }
            Some(Reach::InPlay(inclusion)) => match inclusion.manifest(&repository)? {
                None if matches!(asked, RefreshSelection::All) => {
                    let path = remotes::materialization(&repository, remote);
                    inclusion::warn_not_materialized(remote, &path, reporter);
                    continue;
                }
                None => UnrefreshableReason::NotMaterialized {
                    remote: remote.clone(),
                    path: remotes::materialization(&repository, remote),
                },
                Some(included) => {
                    for key in asked.keys_for(remote) {
                        if let Some(reason) = declared(included.vars.get(&key.name), Some(remote)) {
                            refusals.push(UnrefreshableKey::of(key, reason));
                        }
                    }
                    opened.push((remote, included.vars));
                    continue;
                }
            },
        };
        for key in asked.keys_for(remote) {
            refusals.push(UnrefreshableKey::of(key, reason.clone()));
        }
    }
    refusals.sort_by(|a, b| a.key.cmp(&b.key));
    refuse(refusals)?;

    for (remote, vars) in &opened {
        let tree = remotes::materialization(&repository, remote);
        let source = ManifestSource {
            remote: Some(remote),
            root: &tree,
        };
        dynamic.force(vars, &source)?;
    }
    Ok(())
}

/// Whether an admitted inclusion makes a remote eligible for refresh.
enum Reach {
    /// The first inclusion naming it that a run would open.
    InPlay(Inclusion),
    /// Every inclusion naming it is excluded; each one's label and exclusion.
    Excluded(Vec<(String, Exclusion)>),
}

/// Evaluate inclusion admission for each remote named by an `include-remote`. Apply `selection`
/// and `excluded_remotes`; omit remotes with no inclusion.
fn reachability(
    actions: &[Action],
    selection: &Selection<'_>,
    bindings: &Bindings<'_>,
    excluded_remotes: &BTreeMap<ItemId, Exclusion>,
) -> BTreeMap<ItemId, Reach> {
    let mut reach = BTreeMap::new();
    for (index, action) in actions.iter().enumerate() {
        let Action::IncludeRemote(declaration) = action else {
            continue;
        };
        let remote = declaration.remote.clone();
        let inclusion = Inclusion::at(declaration, index + 1);
        let leaf = |id: Option<&ItemId>| id.map(|id| ItemAddress::qualified(None, id));
        let (address, group_address) = (leaf(action.id()), leaf(action.group()));
        let record = Subject {
            address: address.as_ref(),
            group_address: group_address.as_ref(),
            gate: action.gate(),
        };
        let exclusion = inclusion.exclusion(
            record,
            selection,
            bindings,
            excluded_remotes.get(inclusion.remote()),
        );
        match (
            exclusion,
            reach
                .entry(remote)
                .or_insert_with(|| Reach::Excluded(Vec::new())),
        ) {
            (_, Reach::InPlay(_)) => {}
            (None, entry) => *entry = Reach::InPlay(inclusion),
            (Some(exclusion), Reach::Excluded(exclusions)) => {
                exclusions.push((inclusion.label().to_owned(), exclusion));
            }
        }
    }
    reach
}

/// Why a key cannot be refreshed, as far as the leaf manifest alone can tell.
fn declared_in_leaf(key: &DynamicVarKey, manifest: &Manifest) -> Option<UnrefreshableReason> {
    let Some(remote) = &key.remote else {
        return declared(manifest.vars.get(&key.name), None);
    };
    match manifest.remotes.get(remote) {
        None => Some(UnrefreshableReason::UnknownRemote {
            remote: remote.clone(),
        }),
        Some(declaration) if !declaration.allows_dynamic_vars() => {
            Some(UnrefreshableReason::NotAllowed {
                remote: remote.clone(),
            })
        }
        Some(_) => None,
    }
}

/// Why `spec`, looked up in the leaf's or `remote`'s `[vars]`, is not a
/// dynamic declaration.
fn declared(spec: Option<&VarSpec>, remote: Option<&ItemId>) -> Option<UnrefreshableReason> {
    match spec {
        None => Some(UnrefreshableReason::NotDeclared {
            remote: remote.cloned(),
        }),
        Some(VarSpec::Static(_)) => Some(UnrefreshableReason::Static),
        Some(VarSpec::Dynamic(_)) => None,
    }
}

/// Fail naming every refusal, if there are any.
fn refuse(refusals: Vec<UnrefreshableKey>) -> Result<(), Error> {
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(RefreshError::Unrefreshable(refusals).into())
    }
}

/// Say what was refreshed, and fail if a forced command did not capture.
fn report(dynamic: &DynamicVarResolver<'_>, reporter: &Reporter) -> Result<(), Error> {
    let mut forced = 0;
    let mut failed = 0;
    for (identity, resolved) in dynamic.forced() {
        forced += 1;
        match resolved.state {
            DynamicValueState::Refreshed => reporter.info(&format!("refreshed `{identity}`")),
            // Resolution already reported each failure and its fallback.
            _ => failed += 1,
        }
    }
    if forced == 0 {
        reporter.info("nothing to refresh");
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(RefreshError::NotRefreshed { count: failed }.into())
    }
}

/// `vars refresh`'s own failures.
#[derive(Debug, ThisError)]
pub(crate) enum RefreshError {
    /// Keys naming something the command cannot refresh, in key order.
    #[error("{}", render(.0))]
    Unrefreshable(Vec<UnrefreshableKey>),

    /// Forced commands that ran and captured nothing; each was warned about.
    #[error(
        "{count} dynamic {} could not be refreshed",
        if *.count == 1 { "variable" } else { "variables" }
    )]
    NotRefreshed { count: usize },
}

/// One key, and why it cannot be refreshed.
#[derive(Debug)]
pub(crate) struct UnrefreshableKey {
    key: DynamicVarKey,
    reason: UnrefreshableReason,
}

impl UnrefreshableKey {
    fn of(key: &DynamicVarKey, reason: UnrefreshableReason) -> Self {
        Self {
            key: key.clone(),
            reason,
        }
    }
}

/// Why a key cannot be refreshed.
#[derive(Debug, Clone, ThisError)]
pub(crate) enum UnrefreshableReason {
    #[error("{} does not declare it", declarer(.remote.as_ref()))]
    NotDeclared { remote: Option<ItemId> },

    #[error("it is a static variable, with no command to run")]
    Static,

    #[error("batfiles.toml declares no remote `{remote}`")]
    UnknownRemote { remote: ItemId },

    #[error(
        "remote `{remote}` is not allowed to run dynamic variables; set \
         `allow-dynamic-vars = true` on it to run them"
    )]
    NotAllowed { remote: ItemId },

    #[error("no include-remote in batfiles.toml includes remote `{remote}`")]
    NotIncluded { remote: ItemId },

    /// Every inclusion naming the remote is excluded; one reason for each.
    #[error("remote `{remote}` is not in play on this machine: {}", .reasons.join("; "))]
    NotInPlay {
        remote: ItemId,
        reasons: Vec<String>,
    },

    #[error(
        "remote `{remote}` is not materialized at {}; run `batfiles sync` to bring it down",
        .path.display()
    )]
    NotMaterialized { remote: ItemId, path: PathBuf },
}

/// How a reason names the manifest a declaration was looked up in.
fn declarer(remote: Option<&ItemId>) -> String {
    match remote {
        Some(remote) => format!("batfiles.toml of remote `{remote}`"),
        None => "batfiles.toml".to_owned(),
    }
}

/// One refusal on one line, or several beneath a count.
fn render(refusals: &[UnrefreshableKey]) -> String {
    match refusals {
        [only] => format!("cannot refresh `{}`: {}", only.key, only.reason),
        _ => {
            let lines: Vec<String> = refusals
                .iter()
                .map(|refusal| format!("\n  `{}`: {}", refusal.key, refusal.reason))
                .collect();
            format!(
                "cannot refresh {} dynamic variables:{}",
                refusals.len(),
                lines.concat()
            )
        }
    }
}
