//! Resolve variable values and origins from manifest, machine, environment,
//! and CLI layers. Preserve shadowed declarations for provenance. A manifest's
//! values are templates, evaluated lazily and once per scope.
//! See [`docs/environment.md`](../../docs/environment.md#variable-precedence).

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::rc::Rc;

use crate::env::Environment;
use crate::error::Error;
use crate::location::MachineRoots;
use crate::manifest::check::ValueRule;
use crate::manifest::condition::{Bindings, EvalError, HostNamespaces};
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::template::{EnvAccess, TemplateError, TemplateString};
use crate::manifest::vars::VarSpec;
use crate::output::{Reporter, quoted_value};
use crate::vars::dynamic::{DynamicVarResolver, ManifestSource, ResolvedDynamicVar};
use crate::vars::env_overrides;
use crate::vars::machine::MachineVars;
use crate::vars::name::VarName;

/// Verbosity level for variable-value reports (`-vv`).
const DETAIL: u8 = 2;

/// The most templates one read may evaluate nested inside each other.
pub(crate) const MAX_DEPTH: usize = 32;

/// How a report names a manifest's `[vars]`. An included manifest's also names
/// its inclusion.
const MANIFEST_LABEL: &str = "batfiles.toml";

/// Where a declaration came from. The order is the precedence order, lowest
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The `[vars]` of the manifest an `include-remote` opened, applying only to
    /// that inclusion's records. Labeled by the inclusion. At most one per scope.
    IncludedManifest(String),
    /// The leaf repository's `[vars]`.
    LeafManifest,
    /// An `include-remote`'s `vars` overrides, applying only to that
    /// inclusion's records. Labeled by the inclusion. At most one per scope.
    InclusionOverrides(String),
    /// The machine-local `vars.toml`.
    Machine,
    /// A `BATFILES_VAR_*` variable in this run's environment.
    Environment,
    /// A `--var` on this command line.
    CommandLine,
}

impl Origin {
    /// How a report names this origin.
    fn label(&self) -> Cow<'_, str> {
        match self {
            Self::IncludedManifest(label) => Cow::Owned(format!("{MANIFEST_LABEL} of {label}")),
            Self::LeafManifest => Cow::Borrowed(MANIFEST_LABEL),
            Self::InclusionOverrides(label) => Cow::Borrowed(label),
            Self::Machine => Cow::Borrowed("vars.toml"),
            Self::Environment => Cow::Borrowed("BATFILES_VAR_*"),
            Self::CommandLine => Cow::Borrowed("--var"),
        }
    }

    /// Return the inclusion's unformatted label, or `None` for a run-level origin.
    fn inclusion(&self) -> Option<&str> {
        match self {
            Self::IncludedManifest(label) | Self::InclusionOverrides(label) => Some(label),
            _ => None,
        }
    }
}

/// One declared value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum VarValue {
    /// A literal string: written in a state file, the environment, or on the
    /// command line, or a manifest value with no `${...}` in it.
    Static(String),
    /// A manifest's `[vars]` value, evaluated in the scope that first reads it, or an
    /// inclusion's `vars` override that a higher layer overrides, never evaluated.
    Template(TemplateString),
    /// An inclusion's `vars` override, already evaluated in the leaf's scope.
    Evaluated {
        template: TemplateString,
        result: Resolved,
    },
    /// What a manifest's dynamic declaration resolved to. One that produced no
    /// value still overrides the layers beneath it, and has no value.
    Dynamic(ResolvedDynamicVar),
}

impl VarValue {
    /// A manifest value as a layer holds it: literal text as is, anything else as a template.
    pub fn written(template: &TemplateString) -> Self {
        match template.literal() {
            Some(literal) => Self::Static(literal.to_owned()),
            None => Self::Template(template.clone()),
        }
    }

    /// The template a listing shows beside the value, if the value was computed from one.
    fn template(&self) -> Option<&TemplateString> {
        match self {
            Self::Template(template) | Self::Evaluated { template, .. } => Some(template),
            Self::Static(_) | Self::Dynamic(_) => None,
        }
    }

    /// How a listing describes where a dynamic value came from, after its
    /// origin. `None` for any other value.
    fn state(&self) -> Option<String> {
        match self {
            Self::Dynamic(resolved) => Some(resolved.describe()),
            Self::Static(_) | Self::Template(_) | Self::Evaluated { .. } => None,
        }
    }
}

/// A declared variable's value, or why it has none.
pub(crate) type Resolved = Result<String, NoValue>;

/// Why a declared variable has no value. The reason names variables, never values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NoValue {
    reason: String,
    /// Whether the reason is already in the listing, as a dynamic declaration's state.
    dynamic: bool,
}

impl NoValue {
    /// A dynamic declaration that produced no value.
    fn dynamic(resolved: &ResolvedDynamicVar) -> Self {
        Self {
            reason: resolved.describe(),
            dynamic: true,
        }
    }

    /// A template that could not be evaluated.
    fn template(error: &TemplateError) -> Self {
        let reason = match error {
            TemplateError::Evaluation { error, .. } => match error {
                EvalError::Undeclared { name } => format!("`{name}` is not declared"),
                EvalError::NoValue { name, reason } => format!("reads `{name}`: {reason}"),
                other => other.to_string(),
            },
            other => other.to_string(),
        };
        Self {
            reason,
            dynamic: false,
        }
    }

    /// A read its scope recorded as a [`FailedRead`], which makes the command fail.
    fn failed(read: &FailedRead) -> Self {
        let reason = match read {
            FailedRead::Cycle(_) => "its value refers back to itself",
            FailedRead::TooDeep(_) => "it reads through too many variables",
        };
        Self {
            reason: reason.to_owned(),
            dynamic: false,
        }
    }
}

/// A read that fails the command, with the names in the order they were read.
#[derive(Debug, Clone)]
enum FailedRead {
    /// A read of a variable already being evaluated; the chain ends with the repeated name.
    Cycle(Vec<String>),
    /// A read that would nest more than [`MAX_DEPTH`] templates; the chain ends with the
    /// name that would exceed it.
    TooDeep(Vec<String>),
}

impl FailedRead {
    fn error(&self) -> Error {
        match self {
            Self::Cycle(chain) => Error::VariableCycle {
                chain: chain.join(" -> "),
            },
            Self::TooDeep(chain) => Error::VariableDepth {
                chain: chain.join(" -> "),
                limit: MAX_DEPTH,
            },
        }
    }
}

/// The reason alone, such as `command failed` or `` `nope` is not declared ``.
impl fmt::Display for NoValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

/// One source and everything it declared.
#[derive(Debug, Clone)]
struct Layer {
    origin: Origin,
    values: BTreeMap<VarName, VarValue>,
    /// The remote whose templates these are, when it may not read `env`.
    env_denied_by: Option<ItemId>,
}

impl Layer {
    fn new(origin: Origin, values: BTreeMap<VarName, VarValue>) -> Self {
        Self {
            origin,
            values,
            env_denied_by: None,
        }
    }

    /// Whether this layer's templates may read `env`.
    fn env(&self) -> EnvAccess<'_> {
        match &self.env_denied_by {
            Some(remote) => EnvAccess::DeniedBy(remote),
            None => EnvAccess::Allowed,
        }
    }
}

/// Variable layers for one condition scope, stored in increasing precedence
/// order, and the values read from them so far. A run has its own; inclusions
/// may derive their own scopes.
#[derive(Debug)]
pub(crate) struct VarScope {
    layers: Vec<Layer>,
    /// Each name read so far, by its winning declaration's result.
    evaluated: RefCell<BTreeMap<String, Resolved>>,
    /// The names whose templates are being evaluated, outermost first.
    active: RefCell<Vec<String>>,
    /// The first read that fails the command.
    failed: RefCell<Option<FailedRead>>,
}

impl VarScope {
    /// Read the machine and environment layers and stack them with the leaf's
    /// `[vars]` (`manifest`, declared in `repository`) and every `--var`
    /// (`cli`, in written order). `dynamic` resolves the leaf's declarations.
    ///
    /// An unreadable or malformed `vars.toml` or dynamic-variable cache fails;
    /// [`env_overrides`] warns about and drops unusable environment names.
    pub fn resolve(
        manifest: &BTreeMap<VarName, VarSpec>,
        repository: &std::path::Path,
        roots: &MachineRoots,
        env: &Environment,
        cli: &[(VarName, String)],
        dynamic: &mut DynamicVarResolver<'_>,
        reporter: &Reporter,
    ) -> Result<Self, Error> {
        let machine = MachineVars::load(&roots.machine_vars_path())?;
        let environment = env_overrides::overrides(env, reporter);
        let source = ManifestSource {
            tree: Tree::Leaf,
            root: repository,
        };
        let manifest = dynamic.layer(manifest, &source, &machine.values)?;
        Ok(Self::stack(manifest, machine.values, environment, cli))
    }

    /// Stack four already-read layers in increasing precedence order.
    pub fn stack(
        manifest: BTreeMap<VarName, VarValue>,
        machine: BTreeMap<VarName, String>,
        environment: BTreeMap<VarName, String>,
        cli: &[(VarName, String)],
    ) -> Self {
        Self::of(vec![
            Layer::new(Origin::LeafManifest, manifest),
            Layer::new(Origin::Machine, statics(machine)),
            Layer::new(Origin::Environment, statics(environment)),
            Layer::new(Origin::CommandLine, statics(cli_overrides(cli))),
        ])
    }

    /// A scope over `layers`, with nothing evaluated yet.
    fn of(layers: Vec<Layer>) -> Self {
        Self {
            layers,
            evaluated: RefCell::default(),
            active: RefCell::default(),
            failed: RefCell::default(),
        }
    }

    /// Derive an inclusion scope with remote variables below the leaf layer and inclusion
    /// overrides directly above it. `label` identifies both added layers. `remote_env` is the
    /// included remote when its templates may not read `env`.
    ///
    /// Each override in force is evaluated now, in this scope, reading `env` and the host
    /// namespaces in `host`; one that fails has no value. One a machine, environment, or
    /// command-line value overrides is kept unevaluated, for provenance. Fails as
    /// [`check_reads`](Self::check_reads) does for the variables an override reads.
    ///
    /// Call on the run's base scope. Each scope contains at most one layer
    /// of each kind.
    pub fn with_inclusion(
        self: &Rc<Self>,
        remote: &BTreeMap<VarName, VarValue>,
        remote_env: Option<&ItemId>,
        overrides: &BTreeMap<VarName, TemplateString>,
        label: &str,
        host: &HostNamespaces,
    ) -> Result<Self, Error> {
        let overrides = overrides
            .iter()
            .map(|(name, template)| {
                let value = match VarValue::written(template) {
                    VarValue::Template(template) if !self.overridden_above_leaf(name) => {
                        VarValue::Evaluated {
                            result: self.evaluate(&template, EnvAccess::Allowed, host),
                            template,
                        }
                    }
                    unevaluated => unevaluated,
                };
                (name.clone(), value)
            })
            .collect();
        self.check_reads()?;
        let mut layers = vec![Layer {
            origin: Origin::IncludedManifest(label.to_owned()),
            values: remote.clone(),
            env_denied_by: remote_env.cloned(),
        }];
        let mut overrides = Some(overrides);
        for layer in self.base_layers() {
            let leaf = matches!(layer.origin, Origin::LeafManifest);
            layers.push(layer.clone());
            // `stack` always writes a leaf layer, so the overrides are added
            // exactly once.
            if leaf && let Some(values) = overrides.take() {
                layers.push(Layer::new(
                    Origin::InclusionOverrides(label.to_owned()),
                    values,
                ));
            }
        }
        Ok(Self::of(layers))
    }

    /// Whether a layer above the leaf's `[vars]`, and so above any inclusion override,
    /// declares `name`.
    fn overridden_above_leaf(&self, name: &VarName) -> bool {
        self.declaring(name.as_str())
            .next()
            .is_some_and(|(origin, _)| {
                matches!(
                    origin,
                    Origin::Machine | Origin::Environment | Origin::CommandLine
                )
            })
    }

    /// The run's own layers, from which an inclusion scope is derived.
    ///
    /// Requires the run's base scope. Panics in debug builds if it contains
    /// inclusion layers; release builds omit those layers.
    fn base_layers(&self) -> impl Iterator<Item = &Layer> {
        debug_assert!(
            self.derived_layers().next().is_none(),
            "an inclusion scope is derived from the run's base scope, not from another scope"
        );
        self.layers
            .iter()
            .filter(|layer| layer.origin.inclusion().is_none())
    }

    /// The value of `name` in this scope, or `None` for an undeclared name. A
    /// declaration with no value still overrides lower layers.
    ///
    /// A template is evaluated the first time its name is read, against this scope and
    /// `host`, reading `env` as its declaring manifest may; its result is kept for later reads.
    /// Reading a name whose template is being evaluated, or one that would nest more than
    /// [`MAX_DEPTH`] templates, records a failed read, which
    /// [`check_reads`](Self::check_reads) then reports, and has no value.
    pub fn lookup(self: &Rc<Self>, name: &str, host: &HostNamespaces) -> Option<Resolved> {
        if let Some(resolved) = self.evaluated.borrow().get(name) {
            return Some(resolved.clone());
        }
        let (layer, value) = self.declaring_layers(name).next()?;
        let resolved = match value {
            VarValue::Static(value) => Ok(value.clone()),
            VarValue::Evaluated { result, .. } => result.clone(),
            VarValue::Dynamic(resolved) => resolved
                .value
                .clone()
                .ok_or_else(|| NoValue::dynamic(resolved)),
            VarValue::Template(template) => {
                {
                    let mut active = self.active.borrow_mut();
                    let failed = if let Some(start) = active.iter().position(|a| a == name) {
                        Some(FailedRead::Cycle(chain(&active[start..], name)))
                    } else if active.len() >= MAX_DEPTH {
                        Some(FailedRead::TooDeep(chain(&active, name)))
                    } else {
                        None
                    };
                    if let Some(failed) = failed {
                        let no_value = NoValue::failed(&failed);
                        self.failed.borrow_mut().get_or_insert(failed);
                        return Some(Err(no_value));
                    }
                    active.push(name.to_owned());
                }
                let resolved = self.evaluate(template, layer.env(), host);
                self.active.borrow_mut().pop();
                resolved
            }
        };
        self.evaluated
            .borrow_mut()
            .insert(name.to_owned(), resolved.clone());
        Some(resolved)
    }

    /// Why `name`, already read in this scope, has no value; `None` if it has one, is
    /// undeclared, or has not been read.
    pub fn no_value(&self, name: &str) -> Option<NoValue> {
        self.evaluated.borrow().get(name)?.clone().err()
    }

    /// Fail if any read in this scope found variables that refer to each other in a cycle
    /// ([`Error::VariableCycle`]) or nest more than [`MAX_DEPTH`] templates
    /// ([`Error::VariableDepth`]).
    pub fn check_reads(&self) -> Result<(), Error> {
        match &*self.failed.borrow() {
            Some(failed) => Err(failed.error()),
            None => Ok(()),
        }
    }

    /// `template`, evaluated against this scope and `host`, reading `env` as `env` permits.
    fn evaluate(
        self: &Rc<Self>,
        template: &TemplateString,
        env: EnvAccess<'_>,
        host: &HostNamespaces,
    ) -> Resolved {
        let bindings = Bindings::new(self, host);
        template
            .evaluate(ValueRule::Text, &bindings, env)
            .map(Cow::into_owned)
            .map_err(|error| NoValue::template(&error))
    }

    /// Declarations of arbitrary `name` text, highest precedence first.
    /// Returns an empty iterator if no layer declares it.
    fn declaring<'a>(&'a self, name: &str) -> impl Iterator<Item = (&'a Origin, &'a VarValue)> {
        self.declaring_layers(name)
            .map(|(layer, value)| (&layer.origin, value))
    }

    /// [`Self::declaring`], with each declaration's whole layer.
    fn declaring_layers<'a>(
        &'a self,
        name: &str,
    ) -> impl Iterator<Item = (&'a Layer, &'a VarValue)> {
        self.layers
            .iter()
            .rev()
            .filter_map(move |layer| Some((layer, layer.values.get(name)?)))
    }

    /// Every name any layer declared, in order and without repeats.
    fn names(&self) -> BTreeSet<&VarName> {
        self.layers
            .iter()
            .flat_map(|layer| layer.values.keys())
            .collect()
    }

    /// Report effective values and origins at `-vv`, indented under a heading, evaluating
    /// templates against `host`. An empty scope produces no output. Values are quoted for
    /// single-line output. A cycle among the variables shown fails.
    pub fn report(
        self: &Rc<Self>,
        host: &HostNamespaces,
        reporter: &Reporter,
    ) -> Result<(), Error> {
        self.report_lines("variables:", self.names(), host, reporter)
    }

    /// Report at `-vv`, under the inclusion's label, every name its two layers
    /// declare, each with the layer in force; an overridden declaration shows
    /// as shadowed. Silent for the run's base scope. Fails as [`Self::report`] does.
    pub fn report_inclusion(
        self: &Rc<Self>,
        host: &HostNamespaces,
        reporter: &Reporter,
    ) -> Result<(), Error> {
        let Some((heading, names)) = self.inclusion_block() else {
            return Ok(());
        };
        self.report_lines(&heading, names, host, reporter)
    }

    /// The heading and names of the inclusion block, or `None` for the run's
    /// base scope.
    fn inclusion_block(&self) -> Option<(String, BTreeSet<&VarName>)> {
        let mut inclusion = None;
        let mut names = BTreeSet::new();
        for layer in self.derived_layers() {
            // Both inclusion layers use the same label.
            inclusion = layer.origin.inclusion();
            names.extend(layer.values.keys());
        }
        Some((format!("{} variables:", inclusion?), names))
    }

    /// The layers [derived](Self::with_inclusion) for one inclusion: the
    /// included remote's `[vars]` and the inclusion's own overrides, lowest
    /// first. Empty for the run's base scope.
    fn derived_layers(&self) -> impl Iterator<Item = &Layer> {
        self.layers
            .iter()
            .filter(|layer| layer.origin.inclusion().is_some())
    }

    /// One `-vv` block: the heading, then each name's line indented. Silent
    /// when there are no lines or `-vv` is not shown.
    fn report_lines(
        self: &Rc<Self>,
        heading: &str,
        names: BTreeSet<&VarName>,
        host: &HostNamespaces,
        reporter: &Reporter,
    ) -> Result<(), Error> {
        if !reporter.shows_detail(DETAIL) {
            return Ok(());
        }
        let lines = self.lines_for(names, host)?;
        if lines.is_empty() {
            return Ok(());
        }
        reporter.detail(DETAIL, heading);
        for line in lines {
            reporter.detail(DETAIL, &format!("  {line}"));
        }
        Ok(())
    }

    /// Print effective values and origins to standard output, evaluating templates against
    /// `host`. An empty scope produces only a status message on standard error. A cycle among
    /// the variables fails before anything is printed.
    pub fn list(self: &Rc<Self>, host: &HostNamespaces, reporter: &Reporter) -> Result<(), Error> {
        let lines = self.lines_for(self.names(), host)?;
        if lines.is_empty() {
            reporter.info("no variables are set");
            return Ok(());
        }
        for line in lines {
            reporter.data(&line);
        }
        Ok(())
    }

    /// One line per name in `names`, aligned to the widest of them. Fails on a cycle.
    fn lines_for(
        self: &Rc<Self>,
        names: BTreeSet<&VarName>,
        host: &HostNamespaces,
    ) -> Result<Vec<String>, Error> {
        let width = names
            .iter()
            .map(|name| name.as_str().len())
            .max()
            .unwrap_or_default();
        let lines = names
            .into_iter()
            .filter_map(|name| self.line(name, width, host))
            .collect();
        self.check_reads()?;
        Ok(lines)
    }

    /// Format a variable's effective value, origin, template, dynamic state or reason for
    /// having no value, and shadowed origins. Quote values and templates for single-line
    /// output. Return `None` if no layer declares the name.
    fn line(
        self: &Rc<Self>,
        name: &VarName,
        width: usize,
        host: &HostNamespaces,
    ) -> Option<String> {
        let name = name.as_str();
        let resolved = self.lookup(name, host)?;
        let mut declaring = self.declaring(name);
        let (origin, winner) = declaring.next()?;
        let value = match &resolved {
            Ok(value) => quoted_value(value),
            Err(_) => "no value".to_owned(),
        };
        let mut label = vec![origin.label().into_owned()];
        if let Some(template) = winner.template() {
            label.push(format!("template {}", quoted_value(template.source())));
        }
        label.extend(winner.state());
        if let Err(no_value) = &resolved
            && !no_value.dynamic
        {
            label.push(no_value.to_string());
        }
        let shadowed: Vec<Cow<'_, str>> = declaring.map(|(origin, _)| origin.label()).collect();
        let over = if shadowed.is_empty() {
            String::new()
        } else {
            format!("; over {}", shadowed.join(", "))
        };
        Some(format!(
            "{name:<width$} = {value} ({}{over})",
            label.join(", ")
        ))
    }
}

/// `names` read in order, then `next`.
fn chain(names: &[String], next: &str) -> Vec<String> {
    names.iter().cloned().chain([next.to_owned()]).collect()
}

/// A layer of plain strings.
pub(crate) fn statics(values: BTreeMap<VarName, String>) -> BTreeMap<VarName, VarValue> {
    values
        .into_iter()
        .map(|(name, value)| (name, VarValue::Static(value)))
        .collect()
}

/// Collect `--var` pairs in argument order; the last value for each name wins.
fn cli_overrides(cli: &[(VarName, String)]) -> BTreeMap<VarName, String> {
    let mut values = BTreeMap::new();
    for (name, value) in cli {
        values.insert(name.clone(), value.clone());
    }
    values
}

#[cfg(test)]
mod tests;
