//! Effective reachability: which remotes and inclusions are actually in play,
//! and every variable the surviving layers declare.
//!
//! This is the pipeline `vars list`, `vars refresh`, and `sync` all run. It
//! lives beside [`scope`](crate::scope) and [`condition`](crate::condition)
//! rather than inside [`repo::load`](crate::repo::load) because it is the half
//! that is *not* read-only: it holds a [`Reporter`], captures the host, reads
//! the dynamic-variable cache, and runs commands. `load` keeps only the two
//! halves of its own walk.
//!
//! Six phases, in order, with the one difference between `vars` and `sync`
//! sitting between the third and the fifth:
//!
//! 1. [`load::structure`] — the walk. Nothing under `remotes/` is touched.
//! 2. The leaf's dynamic declarations resolve, and the leaf scope is built.
//! 3. The two condition layers evaluate against that scope, producing
//!    [`Effective`].
//! 4. ***`sync` only:*** materialize [`Effective::included`]. `vars` goes
//!    straight to 5 over whatever is already on disk.
//! 5. [`Structure::read`] — the surviving remotes' manifests.
//! 6. Their allowed declarations resolve, and one scope per surviving inclusion
//!    is built.
//!
//! The staging is what makes phase 4 a single insertion point rather than a
//! mode flag with nowhere to put it: [`Reach`] holds everything phases 1–3
//! produced, and [`Reach::read`] is the step after the seam.
//!
//! **An unevaluable gate closes, and warns.** An undeclared identifier, a
//! non-boolean result, or an arithmetic overflow in a `when` or an `unless`
//! excludes the record. Not "the condition is false": a false `unless` *opens* a
//! gate, so a typo'd `unless = "no_gui_"` would install the very thing it was
//! written to suppress. Closing in both spellings is the safe direction, and it
//! keeps one bad identifier in one third-party remote from costing the whole
//! `sync`. Nothing is silently ignored — the warning names the record and the
//! condition — which is what keeps this strict in `docs/goals.md`'s sense.
#![allow(
    dead_code,
    reason = "no command dispatches to the pipeline until `vars list`"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use jiff::Timestamp;

use crate::condition::{Bindings, EvalError, Host, gate};
use crate::config::{Environment, Roots};
use crate::dynamic::{CachePolicy, Declaration, Identity, Resolution, resolve};
use crate::item::ItemId;
use crate::output::Reporter;
use crate::repo::load::{self, LoadError, Structure};
use crate::repo::{Condition, Leaf, VarDecl};
use crate::scope::{Outcomes, Overlay, Scope};
use crate::state::{DynamicVarCache, MachineVars};
use crate::tomlfile;
use crate::var::VarName;

/// Everything captured once per invocation, and handed to the pipeline whole.
///
/// The [`Overlay`] stays the caller's rather than being built here: `vars list
/// --machine-only` must not build one at all, and `sync` passes its `--var`
/// slice where `vars list` passes `&[]`. Having it as a parameter makes that
/// short-circuit structural. The [`Host`], by contrast, is captured inside
/// [`Reach::start`], so paying `gethostname` exactly once stops being a caller
/// obligation.
pub(crate) struct Invocation<'a> {
    pub overlay: &'a Overlay,
    pub environment: &'a Environment,
    /// The policy **both** resolve passes run under.
    ///
    /// **`vars refresh <key>` must not pass [`CachePolicy::Force`] here.** The
    /// leaf pass is not optional — the leaf scope is what the gates read, and
    /// therefore what decides which remotes are refreshed at all — so a forced
    /// invocation re-runs every leaf declaration just to build that scope, and
    /// moves every `captured-at` with it. That is the opposite of what a
    /// selective refresh asks for.
    ///
    /// Whole-set refresh is the case this field already serves. **Selective
    /// refresh is step 10's, and it is an addition rather than a restructure**:
    /// a `Selection` field beside this one, read in `resolve_layer`, which
    /// splits a layer's declarations into the forced few and the rest at
    /// [`CachePolicy::Auto`]. It is deliberately not built here, because no
    /// caller exists and the key syntax it would key off is step 10's to
    /// specify.
    pub policy: CachePolicy,
    pub shadowing: Shadowing<'a>,
    pub reporter: &'a Reporter,
}

/// Whether a declaration a higher layer already binds is still run.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Shadowing<'a> {
    /// `docs/state.md`'s eager rule: everything reachable and allowed runs,
    /// including declarations a machine-local value overrides.
    Eager,
    /// `vars list` only: a name this map holds is not run.
    Lazy(&'a MachineVars),
}

impl Shadowing<'_> {
    /// Whether `name` is already bound by a higher layer.
    ///
    /// Called from *both* discovery passes, which is what makes "a name
    /// `vars.toml` holds is marked shadowed on every declaration of it" one line
    /// rather than an invariant to remember. [`Refresh::Shadowed`] is sticky in
    /// the merge, so marking only one layer would produce a row reading "not
    /// run" over a value that did run.
    ///
    /// [`Refresh::Shadowed`]: crate::dynamic::Refresh::Shadowed
    fn shadows(self, name: &VarName) -> bool {
        match self {
            Self::Eager => false,
            Self::Lazy(machine) => machine.values.contains_key(name),
        }
    }
}

/// Phases 1–3 done: the walk, the leaf's resolved declarations, and the leaf
/// scope the gates read.
///
/// `sync` inserts materialization between [`effective`](Self::effective) and
/// [`read`](Self::read), where the [`Structure`] this still owns has each
/// remote's declaration and target root. **No accessor for that is built here**,
/// because no caller exists yet; adding one later is a narrow accessor on a
/// crate-private struct rather than a restructure, precisely because the
/// [`Structure`] has not been given away at the seam.
pub(crate) struct Reach<'a> {
    invocation: Invocation<'a>,
    structure: Structure,
    host: Host,
    cache_path: PathBuf,
    leaf_resolution: Resolution,
    leaf_scope: Scope,
    /// `None` until a declaration set is non-empty. `docs/state.md` says the
    /// cache is not *loaded* when nothing is declared, and that forbids reading
    /// the file at all rather than merely not writing it — so the slot is filled
    /// on first need, in each of the two passes.
    cache: Option<DynamicVarCache>,
}

impl<'a> Reach<'a> {
    /// Phases 1 and 2: walk the leaf, resolve its declarations, build its scope.
    pub fn start(roots: &Roots, invocation: Invocation<'a>) -> Result<Self, Error> {
        let structure = load::structure(roots)?;
        let host = Host::capture(invocation.environment);
        let cache_path = roots.dynamic_vars();
        let mut cache = None;

        let declarations = leaf_declarations(&structure, invocation.shadowing);
        let leaf_resolution = resolve_layer(
            &declarations,
            &mut cache,
            &cache_path,
            invocation.policy,
            invocation.reporter,
        )?;

        let leaf_scope = Scope::leaf(
            &structure.repo,
            &Outcomes::index([&leaf_resolution]),
            invocation.overlay,
        );

        Ok(Self {
            invocation,
            structure,
            host,
            cache_path,
            leaf_resolution,
            leaf_scope,
            cache,
        })
    }

    /// The scope every gate below is evaluated against, and the one `sync`'s
    /// leaf actions will read.
    pub fn leaf_scope(&self) -> &Scope {
        &self.leaf_scope
    }

    /// Phase 3: the two condition layers, against the leaf scope.
    ///
    /// One [`Bindings`] for the whole phase — it clones every variable into the
    /// `vars` namespace on construction, so building one per condition would pay
    /// that per `when` in the manifest.
    pub fn effective(&self) -> Effective {
        let bindings = Bindings::new(&self.leaf_scope, &self.host);
        let reporter = self.invocation.reporter;
        let mut closed = Vec::new();

        // Once per *declared* remote rather than once per inclusion, so an
        // unevaluable `[remotes]` condition warns once however many inclusions
        // select it — the same reasoning that puts `Overlay::build` ahead of the
        // scopes.
        let mut open: BTreeSet<&ItemId> = BTreeSet::new();
        for id in self.structure.included.keys() {
            let (when, unless) = self.structure.repo.config.remotes[id].conditions();
            if opens(
                || Site::Remote(id.clone()),
                when,
                unless,
                &bindings,
                &mut closed,
                reporter,
            ) {
                open.insert(id);
            }
        }

        let mut included = BTreeSet::new();
        let mut inclusions = BTreeSet::new();
        for inclusion in &self.structure.inclusions {
            // An inclusion of a remote that already closed is skipped without
            // re-evaluating its own gate and without a second `Closed` entry:
            // the remote's entry already says why.
            if !open.contains(&inclusion.remote) {
                continue;
            }
            // `Action::conditions` is total across the variants, so reaching an
            // `include-remote`'s pair needs no match and no `expect`.
            let (when, unless) =
                self.structure.repo.config.actions[inclusion.position].conditions();
            if opens(
                || Site::Inclusion {
                    remote: inclusion.remote.clone(),
                    label: inclusion.label.clone(),
                },
                when,
                unless,
                &bindings,
                &mut closed,
                reporter,
            ) {
                included.insert(inclusion.remote.clone());
                inclusions.insert(inclusion.position);
            }
        }

        Effective {
            included,
            inclusions,
            closed,
        }
    }

    /// Phases 5 and 6: read the surviving manifests and resolve their allowed
    /// declarations.
    ///
    /// Consumes `self`, because [`Reached`] owns both resolutions and an
    /// inclusion scope indexes across them.
    pub fn read(self, effective: Effective) -> Result<Reached<'a>, Error> {
        // Destructured rather than reached through `self.`, because a method
        // call on `self` borrows all of it while a declaration set borrows the
        // model it points into. Disjoint field borrows are what make this
        // compile; see the discovery functions below.
        let Self {
            invocation,
            structure,
            host,
            cache_path,
            leaf_resolution,
            leaf_scope,
            mut cache,
        } = self;

        let leaf = structure.read(&effective)?;

        let declarations = remote_declarations(&leaf, invocation.shadowing);
        let remote_resolution = resolve_layer(
            &declarations,
            &mut cache,
            &cache_path,
            invocation.policy,
            invocation.reporter,
        )?;

        Ok(Reached {
            invocation,
            host,
            cache_path,
            leaf,
            effective,
            leaf_scope,
            leaf_resolution,
            remote_resolution,
            cache,
        })
    }
}

/// The whole pipeline done: the effective model, both resolutions, and the
/// scopes they make buildable.
pub(crate) struct Reached<'a> {
    invocation: Invocation<'a>,
    /// Carried forward even though nothing here evaluates a condition after
    /// phase 3: `sync`'s action conditions will, and recapturing would pay
    /// `gethostname` twice.
    host: Host,
    cache_path: PathBuf,
    leaf: Leaf,
    effective: Effective,
    leaf_scope: Scope,
    leaf_resolution: Resolution,
    remote_resolution: Resolution,
    cache: Option<DynamicVarCache>,
}

impl Reached<'_> {
    /// The effective model: `included` and `inclusions` carry only survivors.
    pub fn leaf(&self) -> &Leaf {
        &self.leaf
    }

    pub fn effective(&self) -> &Effective {
        &self.effective
    }

    pub fn leaf_scope(&self) -> &Scope {
        &self.leaf_scope
    }

    /// The invocation's [`Host`], for a later phase that evaluates conditions of
    /// its own.
    pub fn host(&self) -> &Host {
        &self.host
    }

    /// Phase 6's second half: one scope per surviving inclusion, in action
    /// order.
    ///
    /// The [`Outcomes`] index spans *both* resolutions, because an inclusion
    /// scope merges the leaf's declarations and the remote's. It is built here
    /// rather than stored on `self`, which would be self-referential.
    pub fn inclusion_scopes(&self) -> Vec<InclusionScope> {
        const INCLUDED: &str = "an inclusion's remote is in the model that produced it";

        let outcomes = Outcomes::index([&self.leaf_resolution, &self.remote_resolution]);
        self.leaf
            .inclusions
            .iter()
            .map(|inclusion| InclusionScope {
                label: inclusion.label.clone(),
                position: inclusion.position,
                scope: Scope::inclusion(
                    self.leaf.included.get(&inclusion.remote).expect(INCLUDED),
                    &self.leaf.repo,
                    inclusion,
                    &outcomes,
                    self.invocation.overlay,
                ),
            })
            .collect()
    }

    /// Write the cache, if and only if either pass wrote an entry.
    ///
    /// The pipeline's job rather than each command's, so `docs/state.md`'s "does
    /// not load, create, or rewrite the cache file or its directory" is one rule
    /// in one place. A filled slot is implied by a changed resolution; both are
    /// stated because the guarantee reads as one rule.
    pub fn commit(&self) -> Result<(), Error> {
        let changed = self.leaf_resolution.changed || self.remote_resolution.changed;
        match &self.cache {
            Some(cache) if changed => cache.save(&self.cache_path).map_err(Error::Cache),
            _ => Ok(()),
        }
    }
}

/// One surviving inclusion's scope, labeled as the model labels it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InclusionScope {
    /// [`Inclusion::label`](crate::repo::Inclusion::label): unique within the
    /// leaf, and what a listing's section header prints.
    pub label: String,
    /// [`Inclusion::position`](crate::repo::Inclusion::position): the action
    /// index, and an id-less inclusion's stable identity.
    pub position: usize,
    pub scope: Scope,
}

/// Which remotes and inclusions survived their gates, and what closed.
///
/// Lives here rather than on the model: [`RemoteState`] records facts about a
/// materialization, and exclusion is a *verdict* about an inclusion.
///
/// [`RemoteState`]: crate::repo::RemoteState
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Effective {
    /// Declared remotes at least one surviving inclusion selects: the set
    /// [`Structure::read`] reads, and the set `sync` materializes **in order to
    /// splice actions**.
    ///
    /// Named for inclusion rather than for fetching on purpose. This is **not**
    /// `sync`'s complete fetch set: `sync` also materializes file and archive
    /// remotes, and Git remotes reached through `@remote/path`, but which ones
    /// is a question this phase is not in a position to answer. `@remote/path`
    /// is available only to leaf actions, so that set falls out of action
    /// planning; part of it is not plan-time at all, since a `git-clone-list`
    /// materializes its manifest when the action executes; and gating every
    /// `[remotes]` entry here would evaluate conditions on remotes nothing
    /// references, so one broken `when` on an unused file remote would warn on
    /// every `vars list`. `sync` reuses the same two primitives — one
    /// [`Bindings`] over the leaf scope, and [`gate`] — at the phase where the
    /// action set is known.
    pub included: BTreeSet<ItemId>,
    /// The surviving inclusions, as
    /// [`Inclusion::position`](crate::repo::Inclusion::position) values — action
    /// positions, never indices into [`Structure::inclusions`]. Ordered, so
    /// iterating is action order.
    pub inclusions: BTreeSet<usize>,
    /// Every gate that closed, in evaluation order.
    ///
    /// One flat list rather than a verdict per remote, because exclusion happens
    /// to a *record*. A remote absent from `included` with no [`Site::Remote`]
    /// entry naming it was excluded by its inclusions, and the
    /// [`Site::Inclusion`] entries naming it say which — so the derived case
    /// needs no variant of its own.
    ///
    /// A gate that closed because it could not be *evaluated* is recorded as an
    /// ordinary closure: the user already received a warning naming the fault
    /// when it happened, so nothing downstream owes a second, differently worded
    /// mention of it.
    pub closed: Vec<Closed>,
}

impl Effective {
    /// The all-effective verdict: every declared remote and every inclusion,
    /// with nothing closed.
    ///
    /// What [`load::leaf`] evaluates, and the definition that keeps [`Leaf`] one
    /// thing — the model of whichever inclusion set it was asked to read.
    pub fn all(structure: &Structure) -> Self {
        Self {
            included: structure.included.keys().cloned().collect(),
            inclusions: structure
                .inclusions
                .iter()
                .map(|inclusion| inclusion.position)
                .collect(),
            closed: Vec::new(),
        }
    }
}

/// One record whose gate closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Closed {
    pub site: Site,
    pub gate: Gate,
    /// The condition's source text, as written in the manifest.
    pub condition: String,
}

/// Which record a gate belonged to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Site {
    /// A `[remotes]` entry's own gate.
    Remote(ItemId),
    /// An `include-remote`'s gate. Carries the remote it selects and the
    /// inclusion's label, so a diagnostic needs no second lookup.
    Inclusion { remote: ItemId, label: String },
}

impl fmt::Display for Site {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Remote(id) => write!(f, "remote `{id}`"),
            Self::Inclusion { remote, label } => {
                write!(f, "include-remote `{label}` of remote `{remote}`")
            }
        }
    }
}

/// Which of the two spellings a gate was written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Gate {
    When,
    Unless,
}

impl fmt::Display for Gate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::When => "when",
            Self::Unless => "unless",
        })
    }
}

/// Evaluate one record's gate, recording and reporting whatever closes it.
///
/// `site` is a closure so the identity — two clones — is built only when it is
/// needed, which is the uncommon case: most gates open, and most records have no
/// gate at all.
fn opens(
    site: impl FnOnce() -> Site,
    when: Option<&Condition>,
    unless: Option<&Condition>,
    bindings: &Bindings<'_>,
    closed: &mut Vec<Closed>,
    reporter: &Reporter,
) -> bool {
    // Mirrors `gate`'s own both-present preference, which is unreachable for a
    // validated manifest and documented as arbitrary there.
    let (spelling, condition) = match (when, unless) {
        (Some(when), _) => (Gate::When, when),
        (None, Some(unless)) => (Gate::Unless, unless),
        (None, None) => return true,
    };

    let site = match gate(when, unless, bindings) {
        Ok(true) => return true,
        Ok(false) => site(),
        Err(error) => {
            let site = site();
            reporter.warn(&unevaluable(&site, spelling, &error));
            site
        }
    };

    let entry = Closed {
        site,
        gate: spelling,
        condition: condition.source().to_owned(),
    };
    reporter.detail(1, &excluded(&entry));
    closed.push(entry);
    false
}

/// The warning an unevaluable gate prints before it closes.
///
/// Pure, following the rule step 5 set for the resolver's `warning`: both
/// [`Reporter::warn`] and [`Reporter::detail`] write straight to standard error
/// with no seam a unit test can reach, so the text is asserted through the
/// formatter and emission waits for the command-level tests.
///
/// No `condition` parameter: [`EvalError`] already carries the source text and
/// renders the fault, so the two arguments are the ones it cannot supply — which
/// record, and which of the two spellings the gate was written in.
fn unevaluable(site: &Site, gate: Gate, error: &EvalError) -> String {
    format!("excluding {site}, because its `{gate}` cannot be decided — {error}")
}

/// The `-v` line every closed gate prints.
///
/// The listing itself omits an excluded inclusion entirely — it shows the
/// effective configuration and nothing else — so this is what answers "why did
/// that remote vanish?" without making the user go hunting. It is a real
/// behavior on stderr, at the verbosity level the ladder already exists for,
/// and it is silent at normal verbosity.
fn excluded(closed: &Closed) -> String {
    format!(
        "{} is excluded by its `{}` condition `{}`",
        closed.site, closed.gate, closed.condition
    )
}

/// The leaf's dynamic declarations, marked for shadowing.
///
/// A free function over the pieces it needs rather than a method, because
/// `self.declarations()` would borrow all of `*self` while the resolver wants
/// `&mut self.cache` — disjoint *field* borrows are fine within one body, and
/// method calls on `self` are not. It also makes discovery testable without a
/// [`Reach`].
fn leaf_declarations<'a>(
    structure: &'a Structure,
    shadowing: Shadowing<'_>,
) -> Vec<Declaration<'a>> {
    declarations(
        &structure.repo.config.vars,
        None,
        &structure.repo.root,
        shadowing,
    )
}

/// The surviving remotes' dynamic declarations, filtered to those that may run.
///
/// Iterates `included` — per *declared* remote, not per inclusion — so "two
/// inclusions of one remote share a single capture" is true by construction
/// rather than by a dedupe pass. **The `allow-dynamic-vars` filter is here**,
/// which is why the resolver has no not-allowed outcome, and why it runs
/// *before* the cache slot is filled: a disallowed declaration is not
/// "declared" for the purpose of the do-not-load rule. A remote that is
/// `NoManifest` or `NotMaterialized` carries an empty config and contributes
/// nothing with no branch.
fn remote_declarations<'a>(leaf: &'a Leaf, shadowing: Shadowing<'_>) -> Vec<Declaration<'a>> {
    leaf.included
        .values()
        .filter(|remote| remote.allow_dynamic_vars)
        .flat_map(|remote| {
            declarations(
                &remote.repo.config.vars,
                Some(&remote.id),
                &remote.repo.root,
                shadowing,
            )
        })
        .collect()
}

/// One `[vars]` map's dynamic entries, as the resolver takes them.
fn declarations<'a>(
    vars: &'a BTreeMap<VarName, VarDecl>,
    remote: Option<&ItemId>,
    cwd: &'a Path,
    shadowing: Shadowing<'_>,
) -> Vec<Declaration<'a>> {
    vars.iter()
        .filter_map(|(name, decl)| {
            let VarDecl::Dynamic(dynamic) = decl else {
                return None;
            };
            Some(Declaration {
                identity: Identity {
                    remote: remote.cloned(),
                    name: name.clone(),
                },
                decl: dynamic,
                cwd,
                shadowed: shadowing.shadows(name),
            })
        })
        .collect()
}

/// Resolve one layer, filling the cache slot only if the layer declares
/// anything.
///
/// The empty short-circuit is the whole of `docs/state.md`'s "does not *load*":
/// an invocation with no applicable dynamic declarations must not so much as
/// open the file. Loading in [`Reach::start`] would break that outright, and
/// loading before the remote pass would break it whenever only a remote declares
/// one — so the slot is filled on first need, in each pass.
///
/// The clock is [`Timestamp::now`] rather than a parameter: the resolver already
/// owns the freshness tests through its own clock closure, and a closure here
/// would infect both stage types with a generic to test nothing new.
fn resolve_layer(
    declarations: &[Declaration<'_>],
    slot: &mut Option<DynamicVarCache>,
    path: &Path,
    policy: CachePolicy,
    reporter: &Reporter,
) -> Result<Resolution, Error> {
    if declarations.is_empty() {
        return Ok(Resolution::default());
    }

    if slot.is_none() {
        *slot = Some(DynamicVarCache::load(path)?);
    }
    let cache = slot.as_mut().expect("the slot was just filled");

    Ok(resolve(
        declarations,
        policy,
        cache,
        Timestamp::now,
        reporter,
    ))
}

/// Two ways the pipeline fails, and no more.
///
/// A condition failure is not among them — it warns and closes — and neither is
/// a run failure, which the resolver already turns into an outcome and a
/// warning. Both sources name their own file already: [`LoadError`] names the
/// manifest or the materialization root, and [`tomlfile::Error`] names the path
/// *and* which of read, parse, serialize, or write failed. So `Display`
/// delegates in both arms and adds no wrapper of its own.
#[derive(Debug)]
pub(crate) enum Error {
    /// The repository could not be walked or read.
    Load(LoadError),
    /// `dynamic-vars.toml` could not be read or replaced.
    Cache(tomlfile::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(error) => error.fmt(f),
            Self::Cache(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Load(error) => Some(error),
            Self::Cache(error) => Some(error),
        }
    }
}

impl From<LoadError> for Error {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

impl From<tomlfile::Error> for Error {
    fn from(error: tomlfile::Error) -> Self {
        Self::Cache(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::fs;
    use tempfile::TempDir;

    use crate::dynamic::Refresh;
    use crate::output::Verbosity;
    use crate::repo::{BatfilesConfig, RemoteState};
    use crate::scope::Source;
    use crate::state::CachedVar;

    /// A temporary tree with resolved roots pointing into it, plus the four
    /// per-invocation values the pipeline borrows.
    ///
    /// The cache directory is deliberately *not* created: several tests assert
    /// that the pipeline neither reads nor creates it.
    struct Fixture {
        dir: TempDir,
        roots: Roots,
        machine: MachineVars,
        overlay: Overlay,
        environment: Environment,
        reporter: Reporter,
        policy: CachePolicy,
    }

    impl Fixture {
        /// A leaf repository holding `manifest`, with no machine-local values.
        fn new(manifest: &str) -> Self {
            Self::with_machine(manifest, &[])
        }

        /// A leaf repository holding `manifest`, over a `vars.toml` of `pairs`.
        fn with_machine(manifest: &str, pairs: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            let base = dir.path();
            let roots = Roots {
                home: base.join("home"),
                batfiles_dir: base.join("dotfiles"),
                config_dir: base.join("config"),
                cache_dir: base.join("cache"),
            };
            fs::create_dir_all(&roots.batfiles_dir).expect("leaf root");
            fs::write(roots.batfiles_config(), manifest).expect("leaf manifest");

            let machine = MachineVars {
                values: pairs
                    .iter()
                    .map(|(key, value)| (name(key), (*value).to_owned()))
                    .collect(),
            };
            // `Quiet` so a test whose command fails does not spray the harness's
            // own standard error with the child's. Warnings still print: they
            // are not verbosity-gated, and there is no seam that could catch
            // them here.
            let reporter = Reporter::new(false, Verbosity::Quiet);
            let environment = Environment::from_pairs(std::iter::empty::<(String, String)>());
            let overlay = Overlay::build(&machine, &environment, &[], &reporter);

            Self {
                dir,
                roots,
                machine,
                overlay,
                environment,
                reporter,
                policy: CachePolicy::Auto,
            }
        }

        /// Materialize `id`, with a `batfiles.toml` when one is given.
        fn materialize(&self, id: &str, manifest: Option<&str>) -> PathBuf {
            let root = self.roots.remotes_dir().join(id);
            fs::create_dir_all(&root).expect("remote root");
            if let Some(manifest) = manifest {
                fs::write(root.join(BatfilesConfig::FILE_NAME), manifest).expect("remote manifest");
            }
            root
        }

        /// Put a document at the cache path that `DynamicVarCache::load` cannot
        /// parse.
        ///
        /// The instrument for "did not load": loading is fatal on a malformed
        /// document, so a pipeline that succeeds over one demonstrably never
        /// opened it — which is the only way to tell "did not load" from
        /// "loaded and wrote nothing".
        fn poison_cache(&self) {
            fs::create_dir_all(&self.roots.cache_dir).expect("cache dir");
            fs::write(self.roots.dynamic_vars(), "entries = \n").expect("malformed cache");
        }

        fn invocation<'a>(&'a self, shadowing: Shadowing<'a>) -> Invocation<'a> {
            Invocation {
                overlay: &self.overlay,
                environment: &self.environment,
                policy: self.policy,
                shadowing,
                reporter: &self.reporter,
            }
        }

        /// The whole pipeline, eagerly: start, gate, read.
        fn run(&self) -> Result<Reached<'_>, Error> {
            self.run_with(Shadowing::Eager)
        }

        fn run_with<'a>(&'a self, shadowing: Shadowing<'a>) -> Result<Reached<'a>, Error> {
            let reach = Reach::start(&self.roots, self.invocation(shadowing))?;
            let effective = reach.effective();
            reach.read(effective)
        }

        /// The whole pipeline over the `vars.toml` the fixture holds.
        fn run_lazily(&self) -> Result<Reached<'_>, Error> {
            self.run_with(Shadowing::Lazy(&self.machine))
        }

        fn reached(&self) -> Reached<'_> {
            match self.run() {
                Ok(reached) => reached,
                Err(error) => panic!("the fixture should reach: {error}"),
            }
        }

        /// The failure the pipeline reported. `Reached` is not `Debug` — it
        /// holds a `Host`, which owns expression values — so `expect_err` is
        /// not available.
        fn error(&self) -> Error {
            match self.run() {
                Ok(_) => panic!("the fixture should fail"),
                Err(error) => error,
            }
        }

        /// Whether the command declaring `name` recorded having run, in `repo`.
        fn ran(&self, repo: &Path, name: &str) -> bool {
            repo.join(format!("ran-{name}")).exists()
        }

        fn leaf_root(&self) -> PathBuf {
            self.roots.batfiles_dir.clone()
        }

        fn remote_root(&self, id: &str) -> PathBuf {
            self.roots.remotes_dir().join(id)
        }

        /// The cache as it is on disk, which is empty when the file is absent.
        fn cache(&self) -> DynamicVarCache {
            DynamicVarCache::load(&self.roots.dynamic_vars()).expect("the cache should load")
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }
    }

    fn name(text: &str) -> VarName {
        VarName::new(text).expect("valid name")
    }

    fn id(text: &str) -> ItemId {
        ItemId::new(text).expect("valid id")
    }

    /// A dynamic declaration whose command records having run and then prints
    /// `value`.
    ///
    /// The marker file is the seam for "nothing ran", which no return value can
    /// prove: a pipeline that ran a command and got the expected string back is
    /// indistinguishable from one that skipped it, until the marker is there to
    /// look for.
    fn dynamic(var: &str, value: &str) -> String {
        format!("[vars.{var}]\ncommand = \"touch ran-{var}; printf %s {value}\"\n\n")
    }

    /// The effective value of `key` in `scope`, and where it came from.
    fn effective<'a>(scope: &'a Scope, key: &str) -> (Option<&'a str>, Source) {
        let variable = scope.get(key).unwrap_or_else(|| panic!("`{key}` is bound"));
        (variable.value.as_deref(), variable.source)
    }

    /// The scopes of an invocation, by inclusion label.
    fn by_label(reached: &Reached<'_>) -> BTreeMap<String, Scope> {
        reached
            .inclusion_scopes()
            .into_iter()
            .map(|inclusion| (inclusion.label, inclusion.scope))
            .collect()
    }

    /// A leaf declaring one Git remote that may run commands, plus whatever is
    /// appended.
    fn leaf_with(rest: &str) -> String {
        format!("[remotes.core]\ntype = 'git'\nurl = 'u'\nallow-dynamic-vars = true\n\n{rest}")
    }

    const INCLUDE_CORE: &str = "[[actions]]\ntype = 'include-remote'\nremote = 'core'\n\n";

    // ---- rendering -------------------------------------------------------

    #[test]
    fn both_renderings_name_both_sites() {
        // Asserted on the strings directly, because neither `Reporter::warn` nor
        // `Reporter::detail` has a seam a unit test can reach. A `[remotes]`
        // entry and an `include-remote` are the two sentences a user meets.
        let remote = Site::Remote(id("core"));
        let inclusion = Site::Inclusion {
            remote: id("core"),
            label: "shell".to_owned(),
        };

        assert_eq!(
            excluded(&Closed {
                site: remote.clone(),
                gate: Gate::When,
                condition: "work".to_owned(),
            }),
            "remote `core` is excluded by its `when` condition `work`"
        );
        assert_eq!(
            excluded(&Closed {
                site: inclusion.clone(),
                gate: Gate::Unless,
                condition: "facts.os == 'macos'".to_owned(),
            }),
            "include-remote `shell` of remote `core` is excluded by its `unless` \
             condition `facts.os == 'macos'`"
        );

        let error = EvalError::Undeclared {
            condition: "typo".to_owned(),
            name: "typo".to_owned(),
        };
        let warning = unevaluable(&remote, Gate::When, &error);
        assert!(
            warning.starts_with("excluding remote `core`, because its `when` cannot be decided — "),
            "{warning}"
        );
        assert!(warning.contains("`typo` is not declared"), "{warning}");

        let warning = unevaluable(&inclusion, Gate::Unless, &error);
        assert!(
            warning.starts_with(
                "excluding include-remote `shell` of remote `core`, \
                 because its `unless` cannot be decided — "
            ),
            "{warning}"
        );
    }

    // ---- gating ----------------------------------------------------------

    #[test]
    fn a_closed_remote_gate_excludes_every_inclusion_of_it() {
        let fixture = Fixture::new(
            "[vars]\nwork = 'no'\n\n\
             [remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'work'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'again'\nremote = 'core'\n",
        );
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let reached = fixture.reached();
        assert!(reached.effective().included.is_empty());
        assert!(reached.effective().inclusions.is_empty());
        assert!(reached.leaf().included.is_empty());
        assert!(reached.leaf().inclusions.is_empty());
        assert!(reached.inclusion_scopes().is_empty());

        // One entry for the remote, and none for either inclusion: the remote's
        // own record already says why they are gone.
        assert_eq!(
            reached.effective().closed,
            vec![Closed {
                site: Site::Remote(id("core")),
                gate: Gate::When,
                condition: "work".to_owned(),
            }]
        );
    }

    #[test]
    fn a_closed_inclusion_gate_excludes_only_that_inclusion() {
        let fixture = Fixture::new(&leaf_with(
            "[vars]\nwork = 'no'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'gated'\nremote = 'core'\nwhen = 'work'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'plain'\nremote = 'core'\n",
        ));
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let reached = fixture.reached();
        assert_eq!(reached.effective().included, BTreeSet::from([id("core")]));
        assert_eq!(reached.effective().inclusions, BTreeSet::from([1]));
        assert_eq!(
            reached.effective().closed,
            vec![Closed {
                site: Site::Inclusion {
                    remote: id("core"),
                    label: "gated".to_owned(),
                },
                gate: Gate::When,
                condition: "work".to_owned(),
            }]
        );

        // The surviving inclusion still reads the remote, so the remote itself
        // is in play.
        let scopes = by_label(&reached);
        assert_eq!(scopes.len(), 1);
        assert_eq!(
            effective(&scopes["plain"], "theme"),
            (Some("dark"), Source::RemoteVars)
        );
        assert_eq!(
            reached.leaf().included[&id("core")].state,
            RemoteState::Present
        );
    }

    #[test]
    fn a_false_unless_on_one_inclusion_leaves_the_other_alone() {
        let fixture = Fixture::new(&leaf_with(
            "[vars]\nwork = 'yes'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'gated'\nremote = 'core'\nunless = 'work'\n\n\
             [[actions]]\ntype = 'include-remote'\nid = 'plain'\nremote = 'core'\n",
        ));
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let reached = fixture.reached();
        assert_eq!(reached.effective().inclusions, BTreeSet::from([1]));
        assert_eq!(reached.effective().closed[0].gate, Gate::Unless);
        assert_eq!(by_label(&reached).keys().collect::<Vec<_>>(), ["plain"]);
    }

    #[test]
    fn an_unevaluable_gate_closes_in_both_spellings() {
        // The whole reason the direction is "close" rather than "read as false":
        // a false `unless` *opens* a gate, so a typo'd one would install the
        // thing it was written to suppress.
        for (spelling, gate) in [("when", Gate::When), ("unless", Gate::Unless)] {
            let fixture = Fixture::new(&format!(
                "[remotes.core]\ntype = 'git'\nurl = 'u'\n{spelling} = 'no_gui_'\n\n{INCLUDE_CORE}"
            ));
            fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

            let reached = fixture.reached();
            assert!(reached.effective().included.is_empty(), "{spelling}");
            assert_eq!(
                reached.effective().closed,
                vec![Closed {
                    site: Site::Remote(id("core")),
                    gate,
                    // The source text as written, so a diagnostic can quote it.
                    condition: "no_gui_".to_owned(),
                }]
            );
        }
    }

    #[test]
    fn an_unevaluable_inclusion_gate_closes_too() {
        let fixture = Fixture::new(&leaf_with(&format!(
            "[[actions]]\ntype = 'include-remote'\nremote = 'core'\nwhen = 'theme'\n\n\
             {INCLUDE_CORE}"
        )));
        // The remote declares `theme`, which is exactly what an inclusion's gate
        // cannot see: it is evaluated against the leaf scope.
        fixture.materialize("core", Some("[vars]\ntheme = 'yes'\n"));

        let reached = fixture.reached();
        assert_eq!(reached.effective().inclusions, BTreeSet::from([1]));
        assert_eq!(
            reached.effective().closed[0].site,
            Site::Inclusion {
                remote: id("core"),
                label: "remote=core".to_owned(),
            }
        );
    }

    #[test]
    fn an_inclusions_own_overrides_do_not_decide_its_own_gate() {
        // The override is an input to the scope the inclusion creates, not to
        // the decision to create it.
        let fixture = Fixture::new(&leaf_with(
            "[vars]\ngui = 'no'\n\n\
             [[actions]]\ntype = 'include-remote'\nremote = 'core'\nwhen = 'gui'\n\
             vars = { gui = 'yes' }\n",
        ));
        fixture.materialize("core", None);

        let reached = fixture.reached();
        assert!(reached.effective().inclusions.is_empty());
        assert_eq!(reached.effective().closed.len(), 1);
    }

    #[test]
    fn a_leaf_static_value_decides_a_remotes_gate() {
        let manifest = |work: &str| {
            format!(
                "[vars]\nwork = '{work}'\n\n\
                 [remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'work'\n\n{INCLUDE_CORE}"
            )
        };

        let fixture = Fixture::new(&manifest("yes"));
        fixture.materialize("core", None);
        assert_eq!(
            fixture.reached().effective().included,
            BTreeSet::from([id("core")])
        );

        let fixture = Fixture::new(&manifest("no"));
        fixture.materialize("core", None);
        assert!(fixture.reached().effective().included.is_empty());
    }

    #[test]
    fn an_excluded_remotes_manifest_is_never_read() {
        // A malformed manifest is fatal wherever it is read, so a pipeline that
        // succeeds over one proves the remote was never opened. It doubles as
        // the regression test for the behavior change in `Structure::read`.
        let fixture = Fixture::new(&format!(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'false'\n\n{INCLUDE_CORE}"
        ));
        fixture.materialize("core", Some("vars = \n"));

        let reached = fixture.reached();
        assert!(reached.leaf().included.is_empty());

        // And the same manifest is still fatal when the gate opens, so the test
        // above is about the gate rather than about manifests being ignored.
        let fixture = Fixture::new(&format!(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = 'true'\n\n{INCLUDE_CORE}"
        ));
        fixture.materialize("core", Some("vars = \n"));
        let error = fixture.error();
        assert!(
            matches!(error, Error::Load(LoadError::Manifest(_))),
            "{error:?}"
        );
    }

    #[test]
    fn a_condition_free_repository_closes_nothing() {
        let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let reached = fixture.reached();
        assert!(reached.effective().closed.is_empty());
        assert_eq!(reached.effective().inclusions, BTreeSet::from([0]));
        assert_eq!(reached.leaf_scope().values.len(), 0);
    }

    #[test]
    fn the_pipeline_reads_nothing_it_was_not_pointed_at() {
        let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
        let elsewhere = fixture.path().join("elsewhere/remotes/core");
        fs::create_dir_all(&elsewhere).expect("stray tree");
        fs::write(
            elsewhere.join(BatfilesConfig::FILE_NAME),
            "[vars]\ntheme = 'dark'\n",
        )
        .expect("stray manifest");

        let reached = fixture.reached();
        assert_eq!(
            reached.leaf().included[&id("core")].state,
            RemoteState::NotMaterialized
        );
    }

    // ---- the cache slot --------------------------------------------------

    #[test]
    fn nothing_declared_neither_loads_nor_creates_the_cache() {
        // Two assertions in one, and they are different guarantees: the poisoned
        // document proves the file was never *opened*, and the absent directory
        // proves nothing was written.
        let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

        let reached = fixture.reached();
        reached.commit().expect("nothing to write");
        assert!(!fixture.roots.dynamic_vars().exists());
        assert!(!fixture.roots.cache_dir.exists());

        let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
        fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));
        fixture.poison_cache();
        fixture.run().expect("the cache is never opened");
    }

    #[test]
    fn a_disallowed_remote_declaration_does_not_even_load_the_cache() {
        // The cross-check between phase 6's filter and the lazy slot: the filter
        // runs *before* the slot, so a declaration that may not run is not
        // "declared" for the do-not-load rule. An implementation that loads the
        // cache before applying the filter fails here and nowhere else.
        let fixture = Fixture::new(&format!(
            "[remotes.core]\ntype = 'git'\nurl = 'u'\n\n{INCLUDE_CORE}"
        ));
        fixture.materialize("core", Some(&dynamic("email", "remote@example.com")));
        fixture.poison_cache();

        let reached = fixture.run().expect("the cache is never opened");
        assert!(!fixture.ran(&fixture.remote_root("core"), "email"));
        // The lower layer's value stands — here, nothing at all.
        assert!(by_label(&reached)["remote=core"].get("email").is_none());
    }

    // ---- resolution ------------------------------------------------------

    #[cfg(unix)]
    mod dynamic_vars {
        use super::*;

        #[test]
        fn a_leaf_dynamic_declaration_decides_a_remotes_gate() {
            // What proves phase 2 runs before phase 3 rather than merely being
            // written above it.
            let fixture = Fixture::new(&format!(
                "{}\n[remotes.core]\ntype = 'git'\nurl = 'u'\nwhen = \"profile == 'work'\"\n\n\
                 {INCLUDE_CORE}",
                dynamic("profile", "work")
            ));
            fixture.materialize("core", Some("[vars]\ntheme = 'dark'\n"));

            let reached = fixture.reached();
            assert!(fixture.ran(&fixture.leaf_root(), "profile"));
            assert_eq!(reached.effective().included, BTreeSet::from([id("core")]));
            assert_eq!(
                effective(reached.leaf_scope(), "profile"),
                (Some("work"), Source::LeafVars)
            );
        }

        #[test]
        fn an_excluded_remote_runs_nothing_even_when_it_is_allowed_to() {
            // Decision 1's consequence, and the reason the gate is evaluated
            // before the remote layer resolves at all.
            let fixture = Fixture::new(&format!(
                "[remotes.core]\ntype = 'git'\nurl = 'u'\nallow-dynamic-vars = true\n\
                 when = 'false'\n\n{INCLUDE_CORE}"
            ));
            fixture.materialize("core", Some(&dynamic("email", "remote@example.com")));

            let reached = fixture.reached();
            reached.commit().expect("nothing to write");

            assert!(!fixture.ran(&fixture.remote_root("core"), "email"));
            assert!(!fixture.roots.dynamic_vars().exists());
            assert!(fixture.cache().entries.is_empty());
        }

        #[test]
        fn an_allowed_remote_declaration_runs_and_is_cached_under_its_own_key() {
            let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
            fixture.materialize("core", Some(&dynamic("email", "remote@example.com")));

            let reached = fixture.reached();
            reached.commit().expect("the capture should be written");

            assert!(fixture.ran(&fixture.remote_root("core"), "email"));
            assert_eq!(
                effective(&by_label(&reached)["remote=core"], "email"),
                (Some("remote@example.com"), Source::RemoteVars)
            );
            assert_eq!(
                fixture.cache().entries["remote:core.email"].value,
                "remote@example.com"
            );
        }

        #[test]
        fn a_disallowed_remote_declaration_contributes_nothing() {
            // Marker absent, no cache key, and the lower layer's value stands.
            let fixture = Fixture::new(&format!(
                "[vars]\nemail = 'leaf@example.com'\n\n\
                 [remotes.core]\ntype = 'git'\nurl = 'u'\n\n{INCLUDE_CORE}"
            ));
            fixture.materialize("core", Some(&dynamic("email", "remote@example.com")));

            let reached = fixture.reached();
            reached.commit().expect("nothing to write");

            assert!(!fixture.ran(&fixture.remote_root("core"), "email"));
            assert!(fixture.cache().entries.is_empty());
            assert_eq!(
                effective(&by_label(&reached)["remote=core"], "email"),
                (Some("leaf@example.com"), Source::LeafVars)
            );
        }

        #[test]
        fn two_inclusions_of_one_remote_share_one_capture() {
            // The command appends, so the file's length counts the runs — which
            // is what a marker file alone cannot do.
            let fixture = Fixture::new(&leaf_with(
                "[[actions]]\ntype = 'include-remote'\nid = 'first'\nremote = 'core'\n\n\
                 [[actions]]\ntype = 'include-remote'\nid = 'second'\nremote = 'core'\n",
            ));
            fixture.materialize(
                "core",
                Some("[vars.email]\ncommand = \"printf x >> runs; printf %s shared\"\n"),
            );

            let reached = fixture.reached();
            reached.commit().expect("the capture should be written");

            assert_eq!(
                fs::read_to_string(fixture.remote_root("core").join("runs")).expect("the marker"),
                "x",
                "one capture, however many inclusions"
            );
            let scopes = by_label(&reached);
            assert_eq!(scopes.len(), 2);
            for label in ["first", "second"] {
                assert_eq!(
                    effective(&scopes[label], "email"),
                    (Some("shared"), Source::RemoteVars)
                );
            }
            assert_eq!(fixture.cache().entries.len(), 1);
        }

        #[test]
        fn a_leaf_declaration_fills_the_cache_slot() {
            let fixture = Fixture::new(&format!(
                "{}{INCLUDE_CORE}",
                leaf_with(&dynamic("email", "x"))
            ));
            fixture.materialize("core", None);
            fixture.poison_cache();

            let error = fixture.error();
            assert!(matches!(error, Error::Cache(_)), "{error:?}");
        }

        #[test]
        fn a_remote_declaration_fills_the_cache_slot_too() {
            // The half an implementation that fills the slot only in the leaf
            // pass would miss.
            let fixture = Fixture::new(&leaf_with(INCLUDE_CORE));
            fixture.materialize("core", Some(&dynamic("email", "x")));
            fixture.poison_cache();

            let error = fixture.error();
            assert!(matches!(error, Error::Cache(_)), "{error:?}");
        }

        #[test]
        fn shadowing_marks_both_layers() {
            // The invariant test. `Refresh::Shadowed` is sticky in the merge, so
            // marking only one layer would produce a row reading "not run" over
            // a value that did run.
            let fixture = Fixture::with_machine(
                &format!("{}{INCLUDE_CORE}", leaf_with(&dynamic("email", "leaf"))),
                &[("email", "machine@example.com")],
            );
            fixture.materialize("core", Some(&dynamic("email", "remote")));

            let reached = fixture.run_lazily().expect("the fixture should reach");
            reached.commit().expect("nothing to write");

            assert!(!fixture.ran(&fixture.leaf_root(), "email"));
            assert!(!fixture.ran(&fixture.remote_root("core"), "email"));
            assert!(!fixture.roots.dynamic_vars().exists());

            let leaf = reached.leaf_scope().get("email").expect("bound");
            assert_eq!(leaf.refresh, Some(Refresh::Shadowed));
            assert_eq!(leaf.source, Source::Machine);

            let scopes = by_label(&reached);
            let inclusion = scopes["remote=core"].get("email").expect("bound");
            assert_eq!(inclusion.refresh, Some(Refresh::Shadowed));
            assert_eq!(inclusion.value.as_deref(), Some("machine@example.com"));
        }

        #[test]
        fn eager_shadowing_runs_the_declaration_and_caches_it() {
            // `docs/state.md`'s eager rule: the capture happens and is cached,
            // and the machine-local value stays effective.
            let fixture = Fixture::with_machine(
                &format!("{}{INCLUDE_CORE}", leaf_with(&dynamic("email", "captured"))),
                &[("email", "machine@example.com")],
            );
            fixture.materialize("core", None);

            let reached = fixture.reached();
            reached.commit().expect("the capture should be written");

            assert!(fixture.ran(&fixture.leaf_root(), "email"));
            assert_eq!(fixture.cache().entries["email"].value, "captured");
            assert_eq!(
                effective(reached.leaf_scope(), "email"),
                (Some("machine@example.com"), Source::Machine)
            );
        }

        #[test]
        fn a_fresh_entry_is_used_and_nothing_runs() {
            // The pipeline's own end of the policy matrix: `Timestamp::now` is
            // called inside the resolve, so a seeded entry has to be stamped
            // against the real clock.
            let fixture = Fixture::new(&leaf_with(&dynamic("email", "fresh")));
            let cache = DynamicVarCache {
                entries: BTreeMap::from([(
                    "email".to_owned(),
                    CachedVar {
                        value: "cached".to_owned(),
                        captured_at: Timestamp::now(),
                    },
                )]),
            };
            cache
                .save(&fixture.roots.dynamic_vars())
                .expect("seed the cache");

            let reached = fixture.reached();
            reached.commit().expect("nothing to write");

            assert!(!fixture.ran(&fixture.leaf_root(), "email"));
            assert_eq!(
                effective(reached.leaf_scope(), "email"),
                (Some("cached"), Source::LeafVars)
            );
            assert_eq!(fixture.cache(), cache);
        }
    }
}
