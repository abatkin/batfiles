//! `vars refresh`: run dynamic variables' commands whatever the cache holds,
//! for the leaf and for every remote in play on this machine. See
//! [`docs/cmdline.md`](../../docs/cmdline.md#vars-refresh).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use thiserror::Error as ThisError;

use super::{DynamicVarResolver, ManifestSource, RefreshOutcome, RefreshSelection, VarIdentity};
use crate::condition::{Bindings, Exclusion, HostNamespaces};
use crate::disabled::Disabled;
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

/// Refresh the named keys, or every declaration in play when none are named,
/// and write what was captured to the cache.
///
/// Every key is checked before any command it names runs, and each one that
/// cannot be refreshed is reported together. A remote key needs the leaf's
/// declarations resolved first, since the gates deciding which remotes are in
/// play read them; those run as a run would run them, and what they captured is
/// saved even when a key then fails. Fails after saving when a forced command
/// failed.
pub(crate) fn run(
    keys: &[VarIdentity],
    roots: &Roots,
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    let selection = RefreshSelection::from_keys(keys);
    refuse(
        selection
            .keys()
            .filter_map(|key| {
                declared_in_leaf(key, &manifest).map(|reason| Refusal::of(key, reason))
            })
            .collect(),
    )?;

    let mut dynamic =
        DynamicVarResolver::refresh(&roots.state, selection.clone(), &manifest.remotes, reporter);
    if selection.reaches_remotes() {
        refresh_with_remotes(manifest, &selection, roots, env, &mut dynamic, reporter)?;
    } else {
        let leaf = ManifestSource {
            remote: None,
            root: &roots.batfiles_dir,
        };
        dynamic.force(&manifest.vars, &leaf)?;
    }
    dynamic.save();
    report(&dynamic, reporter)
}

/// Resolve the leaf, decide which remotes are in play, check the remote keys
/// `asked` names against them, and force what is wanted in each.
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
        &roots.batfiles_dir,
        &roots.state,
        env,
        &[],
        dynamic,
        reporter,
    )?);
    // Before anything that can fail, so the leaf's captures outlive it.
    dynamic.save();
    let host = HostNamespaces::capture(env);
    let bindings = Bindings::new(&variables, &host);
    let selection = Selection::persistent(Disabled::load(&roots.state.disabled())?);
    let excluded_remotes = remotes::excluded(&manifest.remotes, &bindings);
    // Anchored as a run anchors it, so a materialization, and the commands run
    // in it, are where a run finds them.
    let repository = paths::anchor(&roots.batfiles_dir)?;
    let reach = reachability(&manifest.actions, &selection, &bindings, &excluded_remotes);

    let wanted: BTreeSet<&ItemId> = match asked {
        RefreshSelection::All => reach.keys().collect(),
        RefreshSelection::Named(_) => asked.keys().filter_map(|key| key.remote.as_ref()).collect(),
    };

    let mut refusals = Vec::new();
    let mut opened = Vec::new();
    for remote in wanted {
        let reason = match reach.get(remote) {
            None => Reason::NotIncluded {
                remote: remote.clone(),
            },
            Some(Reach::Excluded(exclusions)) => {
                for (label, exclusion) in exclusions {
                    exclusion.report_heading(reporter, label);
                }
                Reason::NotInPlay {
                    remote: remote.clone(),
                    reasons: exclusions
                        .iter()
                        .map(|(label, exclusion)| format!("{label}: {}", exclusion.reason()))
                        .collect(),
                }
            }
            // Admitted by `reachability`, so its tree may be read.
            Some(Reach::InPlay(inclusion)) => match inclusion.manifest(&repository)? {
                None if matches!(asked, RefreshSelection::All) => {
                    let path = remotes::materialization(&repository, remote);
                    inclusion::warn_not_materialized(remote, &path, reporter);
                    continue;
                }
                None => Reason::NotMaterialized {
                    remote: remote.clone(),
                    path: remotes::materialization(&repository, remote),
                },
                Some(included) => {
                    for key in asked.keys_for(remote) {
                        if let Some(reason) = declared(included.vars.get(&key.name), Some(remote)) {
                            refusals.push(Refusal::of(key, reason));
                        }
                    }
                    opened.push((remote, included.vars));
                    continue;
                }
            },
        };
        for key in asked.keys_for(remote) {
            refusals.push(Refusal::of(key, reason.clone()));
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

/// Whether a remote is in play: named by an inclusion a run on this machine
/// would open.
enum Reach {
    /// The first inclusion naming it that a run would open.
    InPlay(Inclusion),
    /// Every inclusion naming it is excluded; each one's label and exclusion.
    Excluded(Vec<(String, Exclusion)>),
}

/// Decide, for every remote an `include-remote` names, whether it is in play,
/// by asking each inclusion what a run of `selection` would, with the remote
/// conditions `excluded_remotes` decided. A remote no inclusion names is absent
/// from the result.
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
        // A leaf record's addresses are unqualified.
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
fn declared_in_leaf(key: &VarIdentity, manifest: &Manifest) -> Option<Reason> {
    let Some(remote) = &key.remote else {
        return declared(manifest.vars.get(&key.name), None);
    };
    match manifest.remotes.get(remote) {
        None => Some(Reason::UnknownRemote {
            remote: remote.clone(),
        }),
        Some(declaration) if !declaration.allows_dynamic_vars() => Some(Reason::NotAllowed {
            remote: remote.clone(),
        }),
        Some(_) => None,
    }
}

/// Why `spec`, looked up in the leaf's or `remote`'s `[vars]`, is not a
/// dynamic declaration.
fn declared(spec: Option<&VarSpec>, remote: Option<&ItemId>) -> Option<Reason> {
    match spec {
        None => Some(Reason::NotDeclared {
            remote: remote.cloned(),
        }),
        Some(VarSpec::Static(_)) => Some(Reason::Static),
        Some(VarSpec::Dynamic(_)) => None,
    }
}

/// Fail naming every refusal, if there are any.
fn refuse(refusals: Vec<Refusal>) -> Result<(), Error> {
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(Failure::Unrefreshable(refusals).into())
    }
}

/// Say what was refreshed, and fail if a forced command did not capture.
fn report(dynamic: &DynamicVarResolver<'_>, reporter: &Reporter) -> Result<(), Error> {
    let mut forced = 0;
    let mut failed = 0;
    for (identity, resolved) in dynamic.forced() {
        forced += 1;
        match resolved.refresh {
            RefreshOutcome::Refreshed => reporter.info(&format!("refreshed `{identity}`")),
            // Each already warned, saying what the run fell back on.
            _ => failed += 1,
        }
    }
    if forced == 0 {
        reporter.info("nothing to refresh");
    }
    if failed == 0 {
        Ok(())
    } else {
        Err(Failure::NotRefreshed { count: failed }.into())
    }
}

/// `vars refresh`'s own failures.
#[derive(Debug, ThisError)]
pub(crate) enum Failure {
    /// Keys naming something the command cannot refresh, in key order.
    #[error("{}", render(.0))]
    Unrefreshable(Vec<Refusal>),

    /// Forced commands that ran and captured nothing; each was warned about.
    #[error(
        "{count} dynamic {} could not be refreshed",
        if *.count == 1 { "variable" } else { "variables" }
    )]
    NotRefreshed { count: usize },
}

/// One key, and why it cannot be refreshed.
#[derive(Debug)]
pub(crate) struct Refusal {
    key: VarIdentity,
    reason: Reason,
}

impl Refusal {
    fn of(key: &VarIdentity, reason: Reason) -> Self {
        Self {
            key: key.clone(),
            reason,
        }
    }
}

/// Why a key cannot be refreshed.
#[derive(Debug, Clone, ThisError)]
pub(crate) enum Reason {
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
fn render(refusals: &[Refusal]) -> String {
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
