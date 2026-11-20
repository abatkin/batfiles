//! Resolve variable values and origins from manifest, machine, environment,
//! and CLI layers. Preserve shadowed declarations for provenance.
//! See [`docs/environment.md`](../../docs/environment.md#variable-precedence).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::env::Environment;
use crate::error::Error;
use crate::location::MachineRoots;
use crate::manifest::remote::Tree;
use crate::manifest::vars::VarSpec;
use crate::output::{Reporter, quoted_value};
use crate::vars::dynamic::{DynamicVarResolver, ManifestSource, ResolvedDynamicVar};
use crate::vars::env_overrides;
use crate::vars::machine::MachineVars;
use crate::vars::name::VarName;

/// Verbosity level for variable-value reports (`-vv`).
const DETAIL: u8 = 2;

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
    /// A string written in a manifest, a state file, the environment, or on
    /// the command line.
    Static(String),
    /// What a manifest's dynamic declaration resolved to. One that produced no
    /// value still overrides the layers beneath it, and reads as empty.
    Dynamic(ResolvedDynamicVar),
}

impl VarValue {
    /// The text a condition reads.
    fn text(&self) -> &str {
        match self {
            Self::Static(value) => value,
            Self::Dynamic(resolved) => resolved.value.as_deref().unwrap_or_default(),
        }
    }

    /// How a listing shows the value: quoted, or `no value`.
    fn shown(&self) -> String {
        match self {
            Self::Static(value) => quoted_value(value),
            Self::Dynamic(ResolvedDynamicVar {
                value: Some(value), ..
            }) => quoted_value(value),
            Self::Dynamic(ResolvedDynamicVar { value: None, .. }) => "no value".to_owned(),
        }
    }

    /// How a listing describes where a dynamic value came from, after its
    /// origin. `None` for a static value.
    fn state(&self) -> Option<String> {
        match self {
            Self::Static(_) => None,
            Self::Dynamic(resolved) => Some(resolved.describe()),
        }
    }
}

/// One source and everything it declared.
#[derive(Debug, Clone)]
struct Layer {
    origin: Origin,
    values: BTreeMap<VarName, VarValue>,
}

/// Variable layers for one condition scope, stored in increasing precedence
/// order. A run has its own; inclusions may derive their own scopes.
#[derive(Debug)]
pub(crate) struct VarScope {
    layers: Vec<Layer>,
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
        Self {
            layers: vec![
                Layer {
                    origin: Origin::LeafManifest,
                    values: manifest,
                },
                Layer {
                    origin: Origin::Machine,
                    values: statics(machine),
                },
                Layer {
                    origin: Origin::Environment,
                    values: statics(environment),
                },
                Layer {
                    origin: Origin::CommandLine,
                    values: statics(cli_overrides(cli)),
                },
            ],
        }
    }

    /// Derive an inclusion scope with remote variables below the leaf layer and inclusion
    /// overrides directly above it. `label` identifies both added layers.
    ///
    /// Call on the run's base scope. Each scope contains at most one layer
    /// of each kind.
    pub fn with_inclusion(
        &self,
        remote: &BTreeMap<VarName, VarValue>,
        overrides: &BTreeMap<VarName, String>,
        label: &str,
    ) -> Self {
        let mut layers = vec![Layer {
            origin: Origin::IncludedManifest(label.to_owned()),
            values: remote.clone(),
        }];
        for layer in self.base_layers() {
            let leaf = matches!(layer.origin, Origin::LeafManifest);
            layers.push(layer.clone());
            // `stack` always writes a leaf layer, so the overrides are added
            // exactly once.
            if leaf {
                layers.push(Layer {
                    origin: Origin::InclusionOverrides(label.to_owned()),
                    values: statics(overrides.clone()),
                });
            }
        }
        Self { layers }
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

    /// Return the highest-priority value, or `None` for an undeclared name. A dynamic
    /// declaration with no captured value returns an empty string and still overrides lower
    /// layers.
    pub fn get<'a>(&'a self, name: &str) -> Option<&'a str> {
        self.declaring(name).next().map(|(_, value)| value.text())
    }

    /// Declarations of arbitrary `name` text, highest precedence first.
    /// Returns an empty iterator if no layer declares it.
    fn declaring<'a>(&'a self, name: &str) -> impl Iterator<Item = (&'a Origin, &'a VarValue)> {
        self.layers
            .iter()
            .rev()
            .filter_map(move |layer| Some((&layer.origin, layer.values.get(name)?)))
    }

    /// Every name any layer declared, in order and without repeats.
    fn names(&self) -> BTreeSet<&VarName> {
        self.layers
            .iter()
            .flat_map(|layer| layer.values.keys())
            .collect()
    }

    /// Report effective values and origins at `-vv`, indented under a heading.
    /// An empty scope produces no output. Values are quoted for single-line output.
    pub fn report(&self, reporter: &Reporter) {
        self.report_lines("variables:", self.names(), reporter);
    }

    /// Report at `-vv`, under the inclusion's label, every name its two layers
    /// declare, each with the layer in force; an overridden declaration shows
    /// as shadowed. Silent for the run's base scope.
    pub fn report_inclusion(&self, reporter: &Reporter) {
        let Some((heading, names)) = self.inclusion_block() else {
            return;
        };
        self.report_lines(&heading, names, reporter);
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
    fn report_lines(&self, heading: &str, names: BTreeSet<&VarName>, reporter: &Reporter) {
        if !reporter.shows_detail(DETAIL) {
            return;
        }
        let lines = self.lines_for(names);
        if lines.is_empty() {
            return;
        }
        reporter.detail(DETAIL, heading);
        for line in lines {
            reporter.detail(DETAIL, &format!("  {line}"));
        }
    }

    /// Print effective values and origins to standard output.
    /// An empty scope produces only a status message on standard error.
    pub fn list(&self, reporter: &Reporter) {
        let lines = self.lines();
        if lines.is_empty() {
            reporter.info("no variables are set");
            return;
        }
        for line in lines {
            reporter.data(&line);
        }
    }

    /// One line for every name any layer declared.
    fn lines(&self) -> Vec<String> {
        self.lines_for(self.names())
    }

    /// One line per name in `names`, aligned to the widest of them.
    fn lines_for(&self, names: BTreeSet<&VarName>) -> Vec<String> {
        let width = names
            .iter()
            .map(|name| name.as_str().len())
            .max()
            .unwrap_or_default();
        names
            .into_iter()
            .filter_map(|name| self.line(name, width))
            .collect()
    }

    /// Format a variable's effective value, origin, dynamic state, and shadowed origins. Quote
    /// values for single-line output. Return `None` if no layer declares the name.
    fn line(&self, name: &VarName, width: usize) -> Option<String> {
        let name = name.as_str();
        let mut declaring = self.declaring(name);
        let (origin, winner) = declaring.next()?;
        let value = winner.shown();
        let label = match winner.state() {
            Some(state) => format!("{}, {state}", origin.label()),
            None => origin.label().into_owned(),
        };
        let shadowed: Vec<Cow<'_, str>> = declaring.map(|(origin, _)| origin.label()).collect();
        let over = if shadowed.is_empty() {
            String::new()
        } else {
            format!("; over {}", shadowed.join(", "))
        };
        Some(format!("{name:<width$} = {value} ({label}{over})"))
    }
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
