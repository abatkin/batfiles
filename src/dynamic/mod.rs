//! Resolve dynamic variables by running commands or reading `dynamic-vars.toml`. Commands may
//! have side effects and run during dry runs too. See [command
//! execution](../../docs/environment.md#how-dynamic-commands-are-run).

mod cache;
pub(crate) mod refresh;
mod resolve;
mod run;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use jiff::Timestamp;

pub(crate) use self::cache::DynamicVarCache;
pub(crate) use self::resolve::{CachePolicy, DynamicValueState, DynamicVarKey, ResolvedDynamicVar};
use self::resolve::{PendingDeclaration, resolve};
use crate::error::Error;
use crate::item::ItemId;
use crate::location::StateRoots;
use crate::manifest::remote::Remote;
use crate::manifest::vars::VarSpec;
use crate::output::Reporter;
use crate::var::VarName;
use crate::var_set::VarValue;

/// Resolve each dynamic declaration at most once per invocation, sharing remote results across
/// inclusions. Load the cache on demand and save only captured values.
pub(crate) struct DynamicVarResolver<'a> {
    path: PathBuf,
    policy: CachePolicy,
    /// Declarations run under [`CachePolicy::Force`] whatever `policy` says;
    /// `None` uses `policy` for every declaration.
    forced: Option<RefreshSelection>,
    /// Whether to skip declarations shadowed by machine-local values.
    skip_shadowed: bool,
    /// The remotes whose declarations may run.
    allowed: BTreeSet<ItemId>,
    /// Remotes already reported as not allowed to run their declarations.
    refused: BTreeSet<ItemId>,
    reporter: &'a Reporter,
    cache: Option<DynamicVarCache>,
    resolved: BTreeMap<DynamicVarKey, ResolvedDynamicVar>,
    changed: bool,
}

/// Which declarations `vars refresh` forces, regardless of cache freshness.
#[derive(Clone)]
pub(crate) enum RefreshSelection {
    All,
    /// At least one key, as constructed by [`Self::from_keys`].
    Named(BTreeSet<DynamicVarKey>),
}

impl RefreshSelection {
    /// Select all declarations if `keys` is empty; otherwise select the distinct named keys.
    pub fn from_keys(keys: &[DynamicVarKey]) -> Self {
        if keys.is_empty() {
            Self::All
        } else {
            Self::Named(keys.iter().cloned().collect())
        }
    }

    /// The keys named, in key order; none for [`All`](Self::All).
    pub fn keys(&self) -> impl Iterator<Item = &DynamicVarKey> {
        match self {
            Self::All => None,
            Self::Named(keys) => Some(keys),
        }
        .into_iter()
        .flatten()
    }

    /// The keys naming one of `remote`'s declarations.
    pub fn keys_for<'a>(&'a self, remote: &'a ItemId) -> impl Iterator<Item = &'a DynamicVarKey> {
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

    fn contains(&self, identity: &DynamicVarKey) -> bool {
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
    /// Create a resolver for action execution, including declarations shadowed by machine-local
    /// values. The leaf's `remotes` determine which remotes may run dynamic commands.
    pub fn for_run(
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

    /// Create a resolver for `vars list`, skipping declarations shadowed by machine-local
    /// values and excluding remote declarations.
    pub fn for_listing(state: &StateRoots, policy: CachePolicy, reporter: &'a Reporter) -> Self {
        Self::new(state, policy, true, BTreeSet::new(), reporter)
    }

    /// Create a resolver that forces `selection` and uses the run policy for other declarations
    /// needed by [`layer`](Self::layer).
    pub fn for_refresh(
        state: &StateRoots,
        selection: RefreshSelection,
        remotes: &BTreeMap<ItemId, Remote>,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            forced: Some(selection),
            ..Self::for_run(state, CachePolicy::Auto, remotes, reporter)
        }
    }

    fn new(
        state: &StateRoots,
        policy: CachePolicy,
        skip_shadowed: bool,
        allowed: BTreeSet<ItemId>,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            path: state.dynamic_vars_cache_path(),
            policy,
            forced: None,
            skip_shadowed,
            allowed,
            refused: BTreeSet::new(),
            reporter,
            cache: None,
            resolved: BTreeMap::new(),
            changed: false,
        }
    }

    /// Build a variable layer from one manifest's `[vars]`, resolving commands according to
    /// policy. Listing resolvers use `machine` to skip shadowed declarations.
    ///
    /// Remotes without permission to run commands contribute only static values and are
    /// reported at `-v`. Cache read failures return an error.
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

    /// Resolve only forced declarations from `vars`, without building a variable layer.
    /// Disallowed remote commands are skipped. Cache read failures return an error.
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
    pub fn forced(&self) -> impl Iterator<Item = (&DynamicVarKey, &ResolvedDynamicVar)> {
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
        let pending: Vec<PendingDeclaration<'_>> = vars
            .iter()
            .filter_map(|(name, spec)| match spec {
                VarSpec::Static(_) => None,
                VarSpec::Dynamic(spec) => {
                    let identity = identity(source, name);
                    Some(PendingDeclaration {
                        policy: self.policy_of(&identity),
                        identity,
                        spec,
                        cwd: source.root,
                        shadowed: self.skip_shadowed && machine.contains_key(name),
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

    fn policy_of(&self, identity: &DynamicVarKey) -> CachePolicy {
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

    /// Return a remote's static values, omitting disallowed dynamic declarations. Report
    /// omissions at `-v`, once per remote.
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

fn identity(source: &ManifestSource<'_>, name: &VarName) -> DynamicVarKey {
    DynamicVarKey {
        remote: source.remote.cloned(),
        name: name.clone(),
    }
}
