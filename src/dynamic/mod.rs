//! Dynamic variables: running their commands, and resolving them against the
//! disposable `dynamic-vars.toml` cache.
//!
//! Commands run in both run modes, and are arbitrary programs with side effects
//! batfiles does not control. See [how dynamic commands are
//! run](../../docs/environment.md#how-dynamic-commands-are-run).

mod cache;
mod resolve;
mod run;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::Timestamp;

pub(crate) use self::cache::DynamicVarCache;
pub(crate) use self::resolve::{CachePolicy, ResolvedDynamicVar};
use self::resolve::{ScopedDeclaration, VarIdentity, resolve};
use crate::error::Error;
use crate::item::ItemId;
use crate::location::StateRoots;
use crate::manifest::remote::Remote;
use crate::manifest::vars::VarSpec;
use crate::output::Reporter;
use crate::var::VarName;
use crate::var_set::VarValue;

/// One command's dynamic declarations, each resolved once.
///
/// Loads the cache only when a declaration needs it, and writes it only when
/// something was captured. Every inclusion of one remote shares its
/// declarations' results, so each command runs at most once per run.
pub(crate) struct DynamicVarResolver<'a> {
    path: PathBuf,
    policy: CachePolicy,
    /// Whether a declaration a machine-local value shadows goes unrun.
    lazy: bool,
    /// The remotes whose declarations may run.
    allowed: BTreeSet<ItemId>,
    /// Remotes already reported as not allowed to run their declarations.
    refused: BTreeSet<ItemId>,
    reporter: &'a Reporter,
    cache: Option<DynamicVarCache>,
    resolved: BTreeMap<VarIdentity, ResolvedDynamicVar>,
    changed: bool,
}

/// Where a manifest's `[vars]` came from.
pub(crate) struct ManifestSource<'a> {
    /// The declaring remote, or `None` for the leaf repository.
    pub remote: Option<&'a ItemId>,
    /// That repository's root, where its commands run.
    pub root: &'a Path,
}

impl<'a> DynamicVarResolver<'a> {
    /// Evaluate every declaration handed over, shadowed or not: the resolver
    /// for commands that execute actions. `remotes` are the leaf's, whose
    /// `allow-dynamic-vars` decides which remotes may run commands.
    pub fn eager(
        state: &StateRoots,
        policy: CachePolicy,
        remotes: &BTreeMap<ItemId, Remote>,
        reporter: &'a Reporter,
    ) -> Self {
        let allowed = remotes
            .iter()
            .filter(|(_, remote)| remote.allows_dynamic_vars())
            .map(|(id, _)| id.clone())
            .collect();
        Self::new(state, policy, false, allowed, reporter)
    }

    /// Leave unrun a declaration a machine-local value shadows: the resolver
    /// for `vars list`, which reads no remote.
    pub fn lazy(state: &StateRoots, policy: CachePolicy, reporter: &'a Reporter) -> Self {
        Self::new(state, policy, true, BTreeSet::new(), reporter)
    }

    fn new(
        state: &StateRoots,
        policy: CachePolicy,
        lazy: bool,
        allowed: BTreeSet<ItemId>,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            path: state.dynamic_vars(),
            policy,
            lazy,
            allowed,
            refused: BTreeSet::new(),
            reporter,
            cache: None,
            resolved: BTreeMap::new(),
            changed: false,
        }
    }

    /// One manifest's `[vars]` as a variable layer, running what the policy
    /// calls for. `machine` is this machine's `vars.toml`, which a lazy
    /// resolver consults.
    ///
    /// A remote not allowed to run commands contributes its static values
    /// alone, and says so at `-v`. Fails only when the cache cannot be read.
    pub fn layer(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        source: &ManifestSource<'_>,
        machine: &BTreeMap<VarName, String>,
    ) -> Result<BTreeMap<VarName, VarValue>, Error> {
        if let Some(remote) = source.remote
            && !self.allowed.contains(remote)
        {
            return Ok(self.refuse(vars, remote));
        }

        let pending: Vec<ScopedDeclaration<'_>> = vars
            .iter()
            .filter_map(|(name, spec)| match spec {
                VarSpec::Static(_) => None,
                VarSpec::Dynamic(spec) => Some(ScopedDeclaration {
                    identity: identity(source, name),
                    spec,
                    cwd: source.root,
                    shadowed: self.lazy && machine.contains_key(name),
                }),
            })
            .filter(|declaration| !self.resolved.contains_key(&declaration.identity))
            .collect();

        if !pending.is_empty() {
            let cache = match self.cache.take() {
                Some(cache) => cache,
                None => DynamicVarCache::load(&self.path)?,
            };
            let cache = self.cache.insert(cache);
            let resolution = resolve(&pending, self.policy, cache, Timestamp::now, self.reporter);
            self.changed |= resolution.changed;
            self.resolved.extend(resolution.vars);
        }

        Ok(vars
            .iter()
            .map(|(name, spec)| {
                let value = match spec {
                    VarSpec::Static(value) => VarValue::Static(value.clone()),
                    VarSpec::Dynamic(_) => VarValue::Dynamic(
                        self.resolved
                            .get(&identity(source, name))
                            .cloned()
                            .expect("every declaration was resolved above"),
                    ),
                };
                (name.clone(), value)
            })
            .collect())
    }

    /// The static half of a remote's `[vars]`: a declaration it may not run
    /// declares nothing. Reports the ones left out at `-v`, once per remote.
    fn refuse(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        remote: &ItemId,
    ) -> BTreeMap<VarName, VarValue> {
        let mut layer = BTreeMap::new();
        let mut unrun = Vec::new();
        for (name, spec) in vars {
            match spec {
                VarSpec::Static(value) => {
                    layer.insert(name.clone(), VarValue::Static(value.clone()));
                }
                VarSpec::Dynamic(_) => unrun.push(format!("`{name}`")),
            }
        }
        if !unrun.is_empty() && self.refused.insert(remote.clone()) {
            self.reporter.detail(
                1,
                &format!(
                    "remote `{remote}` is not allowed to run dynamic variables, so it does not \
                     declare {}; set `allow-dynamic-vars = true` on it to run them",
                    unrun.join(", ")
                ),
            );
        }
        layer
    }

    /// Write the cache if anything was captured since the last save. A cache
    /// that cannot be written warns rather than fails: the captured values
    /// still serve this run.
    pub fn save(&mut self) {
        let Some(cache) = self.cache.as_ref().filter(|_| self.changed) else {
            return;
        };
        self.changed = false;
        if let Err(error) = cache.save(&self.path) {
            self.reporter.warn(&format!(
                "{error}; dynamic variables captured by this run are not cached"
            ));
        }
    }
}

fn identity(source: &ManifestSource<'_>, name: &VarName) -> VarIdentity {
    VarIdentity {
        remote: source.remote.cloned(),
        name: name.clone(),
    }
}
