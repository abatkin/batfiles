//! Dynamic variables: running their commands, and resolving them against the
//! disposable `dynamic-vars.toml` cache.
//!
//! Commands run in both run modes, and are arbitrary programs with side effects
//! batfiles does not control. See [how dynamic commands are
//! run](../../docs/environment.md#how-dynamic-commands-are-run).

mod cache;
pub(crate) mod refresh;
mod resolve;
mod run;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::Timestamp;

pub(crate) use self::cache::DynamicVarCache;
pub(crate) use self::resolve::{CachePolicy, RefreshOutcome, ResolvedDynamicVar, VarIdentity};
use self::resolve::{ScopedDeclaration, resolve};
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
    /// Declarations run under [`CachePolicy::Force`] whatever `policy` says;
    /// `None` uses `policy` for every declaration.
    forced: Option<RefreshSelection>,
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

/// Which declarations `vars refresh` forces, regardless of cache freshness.
#[derive(Clone)]
pub(crate) enum RefreshSelection {
    All,
    /// At least one key, as constructed by [`Self::from_keys`].
    Named(BTreeSet<VarIdentity>),
}

impl RefreshSelection {
    /// The selection a command line's keys ask for: [`All`](Self::All) when
    /// there are none, and each key once however often it was written.
    pub fn from_keys(keys: &[VarIdentity]) -> Self {
        if keys.is_empty() {
            Self::All
        } else {
            Self::Named(keys.iter().cloned().collect())
        }
    }

    /// The keys named, in key order; none for [`All`](Self::All).
    pub fn keys(&self) -> impl Iterator<Item = &VarIdentity> {
        match self {
            Self::All => None,
            Self::Named(keys) => Some(keys),
        }
        .into_iter()
        .flatten()
    }

    /// The keys naming one of `remote`'s declarations.
    pub fn keys_for<'a>(&'a self, remote: &'a ItemId) -> impl Iterator<Item = &'a VarIdentity> {
        self.keys()
            .filter(move |key| key.remote.as_ref() == Some(remote))
    }

    /// Whether any remote's declarations can be refreshed: always for
    /// [`All`](Self::All), and otherwise only when a key names one.
    pub fn reaches_remotes(&self) -> bool {
        match self {
            Self::All => true,
            Self::Named(keys) => keys.iter().any(|key| key.remote.is_some()),
        }
    }

    fn contains(&self, identity: &VarIdentity) -> bool {
        match self {
            Self::All => true,
            Self::Named(keys) => keys.contains(identity),
        }
    }
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

    /// Force `selection` and resolve any other declaration a
    /// [`layer`](Self::layer) needs as a run would: the resolver for
    /// `vars refresh`. Otherwise [`eager`](Self::eager).
    pub fn refresh(
        state: &StateRoots,
        selection: RefreshSelection,
        remotes: &BTreeMap<ItemId, Remote>,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            forced: Some(selection),
            ..Self::eager(state, CachePolicy::Auto, remotes, reporter)
        }
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
            forced: None,
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
        self.resolve(vars, source, machine, false)?;

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

    /// Resolve only the declarations in one manifest's `[vars]` that this
    /// resolver forces, building no layer. A remote not allowed to run
    /// commands runs none, as with [`layer`](Self::layer). Fails only when the
    /// cache cannot be read.
    pub fn force(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        source: &ManifestSource<'_>,
    ) -> Result<(), Error> {
        if let Some(remote) = source.remote
            && !self.allowed.contains(remote)
        {
            self.refuse(vars, remote);
            return Ok(());
        }
        self.resolve(vars, source, &BTreeMap::new(), true)
    }

    /// The forced declarations resolved so far, in identity order: the leaf's
    /// by name, then each remote's.
    pub fn forced(&self) -> impl Iterator<Item = (&VarIdentity, &ResolvedDynamicVar)> {
        self.resolved
            .iter()
            .filter(|(identity, _)| self.policy_of(identity) == CachePolicy::Force)
    }

    /// Resolve every unresolved dynamic declaration in `vars`, or only the
    /// forced ones when `only_forced` is set.
    fn resolve(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        source: &ManifestSource<'_>,
        machine: &BTreeMap<VarName, String>,
        only_forced: bool,
    ) -> Result<(), Error> {
        let pending: Vec<ScopedDeclaration<'_>> = vars
            .iter()
            .filter_map(|(name, spec)| match spec {
                VarSpec::Static(_) => None,
                VarSpec::Dynamic(spec) => {
                    let identity = identity(source, name);
                    Some(ScopedDeclaration {
                        policy: self.policy_of(&identity),
                        identity,
                        spec,
                        cwd: source.root,
                        shadowed: self.lazy && machine.contains_key(name),
                    })
                }
            })
            .filter(|declaration| {
                !self.resolved.contains_key(&declaration.identity)
                    && (!only_forced || declaration.policy == CachePolicy::Force)
            })
            .collect();

        if !pending.is_empty() {
            let cache = match self.cache.take() {
                Some(cache) => cache,
                None => DynamicVarCache::load(&self.path)?,
            };
            let cache = self.cache.insert(cache);
            let resolution = resolve(&pending, cache, Timestamp::now, self.reporter);
            self.changed |= resolution.changed;
            self.resolved.extend(resolution.vars);
        }
        Ok(())
    }

    fn policy_of(&self, identity: &VarIdentity) -> CachePolicy {
        if self
            .forced
            .as_ref()
            .is_some_and(|selection| selection.contains(identity))
        {
            CachePolicy::Force
        } else {
            self.policy
        }
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
