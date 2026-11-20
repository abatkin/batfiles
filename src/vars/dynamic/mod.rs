//! Resolve dynamic variables by running commands, looking up releases, or reading
//! `dynamic-vars.toml`. Commands may have side effects; both kinds capture during dry runs too.
//! See [command execution](../../../docs/environment.md#how-dynamic-commands-are-run).

mod cache;
mod command;
mod lookup;
mod resolve;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use jiff::Timestamp;

pub(crate) use self::cache::DynamicVarCache;
pub(crate) use self::resolve::{
    CachePolicy, DynamicValueState, DynamicVarKey, ResolvedDynamicVar, age,
};
use self::resolve::{Capture, PendingDeclaration, resolve};
use crate::error::Error;
use crate::location::MachineRoots;
use crate::manifest::check::ValueRule;
use crate::manifest::condition::{Bindings, HostNamespaces};
use crate::manifest::item::ItemId;
use crate::manifest::remote::Remote;
use crate::manifest::remote::Tree;
use crate::manifest::template::EnvAccess;
use crate::manifest::vars::{DynamicSource, GithubReleaseSource, VarSpec};
use crate::output::{Reporter, quoted_value};
use crate::vars::name::VarName;
use crate::vars::scope::{VarScope, VarValue};

/// `github-url`'s default.
const DEFAULT_GITHUB_URL: &str = "https://github.com";

/// Resolve each dynamic declaration at most once per invocation, sharing remote results across
/// inclusions. Load the cache on demand and save only captured values.
pub(crate) struct DynamicVarResolver<'a> {
    path: PathBuf,
    policy: CachePolicy,
    /// Declarations run under [`CachePolicy::Force`] whatever `policy` says;
    /// `None` uses `policy` for every declaration.
    forced: Option<RefreshTargets>,
    /// Whether to skip declarations shadowed by machine-local values.
    skip_shadowed: bool,
    /// The remotes whose command declarations may run.
    allowed: BTreeSet<ItemId>,
    /// The remotes whose `github-url` templates may read `env`.
    env_templates: BTreeSet<ItemId>,
    /// Remotes already reported as not allowed to run their commands.
    refused: BTreeSet<ItemId>,
    /// The `env` and `facts` a `github-url` reads.
    host: &'a HostNamespaces,
    reporter: &'a Reporter,
    cache: Option<DynamicVarCache>,
    resolved: BTreeMap<DynamicVarKey, ResolvedDynamicVar>,
    changed: bool,
}

/// Which declarations `vars refresh` forces, regardless of cache freshness.
#[derive(Clone)]
pub(crate) enum RefreshTargets {
    All,
    /// At least one key, as constructed by [`Self::from_keys`].
    Named(BTreeSet<DynamicVarKey>),
}

impl RefreshTargets {
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
            .filter(move |key| key.tree.as_ref() == Tree::Remote(remote))
    }

    /// Whether any remote's declarations can be refreshed: always for
    /// [`All`](Self::All), and otherwise only when a key names one.
    pub fn reaches_remotes(&self) -> bool {
        match self {
            Self::All => true,
            Self::Named(keys) => keys.iter().any(|key| matches!(key.tree, Tree::Remote(_))),
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
    /// The tree that declared them.
    pub tree: Tree<&'a ItemId>,
    /// That repository's root, where its commands run.
    pub root: &'a Path,
}

impl<'a> DynamicVarResolver<'a> {
    /// Create a resolver for action execution, including declarations shadowed by machine-local
    /// values. The leaf's `remotes` determine which remotes may run dynamic commands and which
    /// may read `env` in a `github-url`; `host` holds what a `github-url` reads.
    pub fn for_run(
        state: &MachineRoots,
        policy: CachePolicy,
        remotes: &BTreeMap<ItemId, Remote>,
        host: &'a HostNamespaces,
        reporter: &'a Reporter,
    ) -> Self {
        let having = |permission: fn(&Remote) -> bool| {
            remotes
                .iter()
                .filter(|(_, remote)| permission(remote))
                .map(|(id, _)| id.clone())
                .collect()
        };
        Self {
            allowed: having(Remote::allows_dynamic_vars),
            env_templates: having(Remote::allows_env_templates),
            ..Self::new(state, policy, false, host, reporter)
        }
    }

    /// Create a resolver for `vars list`, skipping declarations shadowed by machine-local
    /// values and excluding remote declarations.
    pub fn for_listing(
        state: &MachineRoots,
        policy: CachePolicy,
        host: &'a HostNamespaces,
        reporter: &'a Reporter,
    ) -> Self {
        Self::new(state, policy, true, host, reporter)
    }

    /// Create a resolver that forces `targets` and uses the run policy for other declarations
    /// needed by [`layer`](Self::layer).
    pub fn for_refresh(
        state: &MachineRoots,
        targets: RefreshTargets,
        remotes: &BTreeMap<ItemId, Remote>,
        host: &'a HostNamespaces,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            forced: Some(targets),
            ..Self::for_run(state, CachePolicy::Auto, remotes, host, reporter)
        }
    }

    fn new(
        state: &MachineRoots,
        policy: CachePolicy,
        skip_shadowed: bool,
        host: &'a HostNamespaces,
        reporter: &'a Reporter,
    ) -> Self {
        Self {
            path: state.dynamic_vars_cache_path(),
            policy,
            forced: None,
            skip_shadowed,
            allowed: BTreeSet::new(),
            env_templates: BTreeSet::new(),
            refused: BTreeSet::new(),
            host,
            reporter,
            cache: None,
            resolved: BTreeMap::new(),
            changed: false,
        }
    }

    /// Build a variable layer from one manifest's `[vars]`, resolving dynamic declarations
    /// according to policy. Listing resolvers use `machine` to skip shadowed declarations.
    ///
    /// A remote without permission to run commands does not declare its command declarations,
    /// which are reported at `-v`. Cache read failures return an error.
    pub fn layer(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        source: &ManifestSource<'_>,
        machine: &BTreeMap<VarName, String>,
    ) -> Result<BTreeMap<VarName, VarValue>, Error> {
        self.resolve(vars, source, machine, false)?;

        Ok(vars
            .iter()
            .filter_map(|(name, spec)| {
                let value = match spec {
                    VarSpec::Static(value) => VarValue::Static(value.clone()),
                    // Only a command its remote may not run is left unresolved, and it
                    // declares nothing.
                    VarSpec::Dynamic(_) => {
                        VarValue::Dynamic(self.resolved.get(&identity(source, name))?.clone())
                    }
                };
                Some((name.clone(), value))
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
        self.resolve(vars, source, &BTreeMap::new(), true)
    }

    /// The forced declarations resolved so far, in identity order: the leaf's
    /// by name, then each remote's.
    pub fn forced(&self) -> impl Iterator<Item = (&DynamicVarKey, &ResolvedDynamicVar)> {
        self.resolved
            .iter()
            .filter(|(identity, _)| self.policy_of(identity) == CachePolicy::Force)
    }

    /// Resolve every unresolved dynamic declaration in `vars`, or only the forced ones when
    /// `only_forced` is set. Skip, and report, command declarations of a remote not allowed to
    /// run them.
    fn resolve(
        &mut self,
        vars: &BTreeMap<VarName, VarSpec>,
        source: &ManifestSource<'_>,
        machine: &BTreeMap<VarName, String>,
        only_forced: bool,
    ) -> Result<(), Error> {
        let refused = match source.tree {
            Tree::Remote(remote) if !self.allowed.contains(remote) => Some(remote),
            _ => None,
        };
        let mut unrun = Vec::new();
        let mut pending = Vec::new();
        for (name, spec) in vars {
            let VarSpec::Dynamic(spec) = spec else {
                continue;
            };
            let identity = identity(source, name);
            let policy = self.policy_of(&identity);
            let capture = match &spec.source {
                DynamicSource::Command(_) if refused.is_some() => {
                    unrun.push(format!("`{name}`"));
                    continue;
                }
                _ if self.resolved.contains_key(&identity)
                    || (only_forced && policy != CachePolicy::Force) =>
                {
                    continue;
                }
                DynamicSource::Command(command) => Capture::Command {
                    spec: command,
                    cwd: source.root,
                },
                DynamicSource::GithubRelease(release) => Capture::Lookup {
                    spec: release,
                    host: self.github_url(release, source.tree),
                },
            };
            pending.push(PendingDeclaration {
                identity,
                capture,
                cache: spec.cache,
                policy,
                shadowed: self.skip_shadowed && machine.contains_key(name),
            });
        }
        if let Some(remote) = refused {
            self.refuse(remote, &unrun);
        }

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
            .is_some_and(|targets| targets.contains(identity))
        {
            CachePolicy::Force
        } else {
            self.policy
        }
    }

    /// Report at `-v`, once per remote, the command declarations `unrun` that `remote` does not
    /// declare because it may not run commands.
    fn refuse(&mut self, remote: &ItemId, unrun: &[String]) {
        if !unrun.is_empty() && self.refused.insert(remote.clone()) {
            self.reporter.detail(
                1,
                &format!(
                    "remote `{remote}` is not allowed to run dynamic-variable commands, so it \
                     does not declare {}; set `allow-dynamic-vars = true` on it to run them",
                    unrun.join(", ")
                ),
            );
        }
    }

    /// The host a release declaration in `tree` asks: its `github-url`, evaluated against `env`
    /// and `facts` alone, with one trailing `/` removed. A failure names the field and its
    /// template, never the value.
    fn github_url(
        &self,
        release: &GithubReleaseSource,
        tree: Tree<&ItemId>,
    ) -> Result<String, String> {
        let Some(url) = &release.github_url else {
            return Ok(DEFAULT_GITHUB_URL.to_owned());
        };
        let env = match tree {
            Tree::Remote(remote) if !self.env_templates.contains(remote) => {
                EnvAccess::DeniedBy(remote)
            }
            _ => EnvAccess::Allowed,
        };
        // Manifest validation refuses a `github-url` reading any variable, so none are bound.
        let no_variables = Rc::new(VarScope::stack(
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeMap::new(),
            &[],
        ));
        let bindings = Bindings::new(&no_variables, self.host);
        match url.evaluate(ValueRule::HttpUrl, &bindings, env) {
            Ok(resolved) => Ok(resolved.strip_suffix('/').unwrap_or(&resolved).to_owned()),
            Err(error) => Err(format!(
                "`github-url` {} {error}",
                quoted_value(url.source())
            )),
        }
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
        tree: source.tree.cloned(),
        name: name.clone(),
    }
}
