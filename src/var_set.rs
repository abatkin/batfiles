//! Resolve variable values and origins from manifest, machine, environment,
//! and CLI layers. Preserve shadowed declarations for provenance.
//! See [`docs/environment.md`](../docs/environment.md#variable-precedence).

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::env::Environment;
use crate::env_vars;
use crate::error::Error;
use crate::location::{Roots, StateRoots};
use crate::machine_vars::MachineVars;
use crate::manifest::Manifest;
use crate::output::{Reporter, quoted_value};
use crate::var::VarName;

/// Reported at `-vv`: variables are a run's inputs, while `-v` reports what it
/// did.
const DETAIL: u8 = 2;

/// How a report names a manifest's `[vars]`. An included manifest's also names
/// its inclusion.
const MANIFEST_LABEL: &str = "batfiles.toml";

/// Where a declaration came from. The order is the precedence order, lowest
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The `[vars]` of the manifest an `include-remote` opened, applying only to
    /// that inclusion's records. Labeled by the inclusion. At most one per set.
    IncludedManifest(String),
    /// The leaf repository's `[vars]`.
    Manifest,
    /// An `include-remote`'s `vars` overrides, applying only to that
    /// inclusion's records. Labeled by the inclusion. At most one per set.
    Inclusion(String),
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
            Self::Manifest => Cow::Borrowed(MANIFEST_LABEL),
            Self::Inclusion(label) => Cow::Borrowed(label),
            Self::Machine => Cow::Borrowed("vars.toml"),
            Self::Environment => Cow::Borrowed("BATFILES_VAR_*"),
            Self::CommandLine => Cow::Borrowed("--var"),
        }
    }

    /// The label of the inclusion this layer was
    /// [derived](VarSet::with_inclusion) for, or `None` for a run layer. Unlike
    /// [`Self::label`], unrendered: it heads the inclusion's `-vv` block.
    fn inclusion(&self) -> Option<&str> {
        match self {
            Self::IncludedManifest(label) | Self::Inclusion(label) => Some(label),
            _ => None,
        }
    }
}

/// One source and everything it declared.
#[derive(Debug, Clone)]
struct Layer {
    origin: Origin,
    values: BTreeMap<VarName, String>,
}

/// Variable layers for one condition scope, stored in increasing precedence
/// order. A run has its own; inclusions may derive their own scopes.
#[derive(Debug)]
pub(crate) struct VarSet {
    layers: Vec<Layer>,
}

impl VarSet {
    /// Read the machine and environment layers and stack them with the leaf's
    /// `[vars]` (`manifest`) and every `--var` (`cli`, in written order).
    ///
    /// An unreadable or malformed `vars.toml` fails; [`env_vars`] warns about
    /// and drops unusable environment names.
    pub fn resolve(
        manifest: &BTreeMap<VarName, String>,
        roots: &StateRoots,
        env: &Environment,
        cli: &[(VarName, String)],
        reporter: &Reporter,
    ) -> Result<Self, Error> {
        let machine = MachineVars::load(&roots.machine_vars())?;
        Ok(Self::stack(
            manifest.clone(),
            machine.values,
            env_vars::overrides(env, reporter),
            cli,
        ))
    }

    /// Stack four already-read layers in increasing precedence order.
    pub fn stack(
        manifest: BTreeMap<VarName, String>,
        machine: BTreeMap<VarName, String>,
        environment: BTreeMap<VarName, String>,
        cli: &[(VarName, String)],
    ) -> Self {
        Self {
            layers: vec![
                Layer {
                    origin: Origin::Manifest,
                    values: manifest,
                },
                Layer {
                    origin: Origin::Machine,
                    values: machine,
                },
                Layer {
                    origin: Origin::Environment,
                    values: environment,
                },
                Layer {
                    origin: Origin::CommandLine,
                    values: cli_overrides(cli),
                },
            ],
        }
    }

    /// The scope for one inclusion's records: these layers plus the included
    /// remote's `[vars]` as the lowest layer and the inclusion's `vars`
    /// overrides directly above the leaf's `[vars]`, as
    /// [`docs/environment.md`](../docs/environment.md#variable-precedence)
    /// orders them. `label` names the inclusion on both layers.
    ///
    /// `self` must be the run's own set, not a derived one: a set holds at most
    /// one layer of each kind. Derive one scope per opened inclusion.
    pub fn with_inclusion(
        &self,
        remote: &BTreeMap<VarName, String>,
        overrides: &BTreeMap<VarName, String>,
        label: &str,
    ) -> Self {
        let mut layers = vec![Layer {
            origin: Origin::IncludedManifest(label.to_owned()),
            values: remote.clone(),
        }];
        for layer in self.base_layers() {
            let leaf = matches!(layer.origin, Origin::Manifest);
            layers.push(layer.clone());
            // `stack` always writes a leaf layer, so the overrides are added
            // exactly once.
            if leaf {
                layers.push(Layer {
                    origin: Origin::Inclusion(label.to_owned()),
                    values: overrides.clone(),
                });
            }
        }
        Self { layers }
    }

    /// The run's own layers, from which an inclusion scope is derived.
    ///
    /// Requires the run's own set. Panics in debug builds if it contains
    /// inclusion layers; release builds omit those layers.
    fn base_layers(&self) -> impl Iterator<Item = &Layer> {
        debug_assert!(
            self.derived_layers().next().is_none(),
            "an inclusion scope is derived from the run's own set, not from another scope"
        );
        self.layers
            .iter()
            .filter(|layer| layer.origin.inclusion().is_none())
    }

    /// The highest-precedence value, or `None` if no layer declares the name.
    /// A declared empty string is a value. The result borrows only from the set.
    pub fn get<'a>(&'a self, name: &str) -> Option<&'a str> {
        self.declaring(name).next().map(|(_, value)| value)
    }

    /// Declarations of arbitrary `name` text, highest precedence first.
    /// Returns an empty iterator if no layer declares it.
    fn declaring<'a>(&'a self, name: &str) -> impl Iterator<Item = (&'a Origin, &'a str)> {
        self.layers
            .iter()
            .rev()
            .filter_map(move |layer| Some((&layer.origin, layer.values.get(name)?.as_str())))
    }

    /// Every name any layer declared, in order and without repeats.
    fn names(&self) -> BTreeSet<&VarName> {
        self.layers
            .iter()
            .flat_map(|layer| layer.values.keys())
            .collect()
    }

    /// Report effective values and origins at `-vv`, indented under a heading.
    /// An empty set produces no output. Values are quoted for single-line output.
    pub fn report(&self, reporter: &Reporter) {
        self.report_lines("variables:", self.names(), reporter);
    }

    /// Report at `-vv`, under the inclusion's label, every name its two layers
    /// declare, each with the layer in force; an overridden declaration shows
    /// as shadowed. Silent for the run's own set.
    pub fn report_inclusion(&self, reporter: &Reporter) {
        let Some((heading, names)) = self.inclusion_block() else {
            return;
        };
        self.report_lines(&heading, names, reporter);
    }

    /// The heading and names of the inclusion block, or `None` for the run's
    /// own set.
    fn inclusion_block(&self) -> Option<(String, BTreeSet<&VarName>)> {
        let mut inclusion = None;
        let mut names = BTreeSet::new();
        for layer in self.derived_layers() {
            // Both layers carry the same label.
            inclusion = layer.origin.inclusion();
            names.extend(layer.values.keys());
        }
        Some((format!("{} variables:", inclusion?), names))
    }

    /// The layers [derived](Self::with_inclusion) for one inclusion: the
    /// included remote's `[vars]` and the inclusion's own overrides, lowest
    /// first. Empty for the run's own set.
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
    /// An empty set produces only a status message on standard error.
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

    /// The line for one name: the value in force, its layer, and the layers it
    /// overrode; `None` if no layer declares the name. Values are untrusted
    /// text, so they go through [`quoted_value`].
    fn line(&self, name: &VarName, width: usize) -> Option<String> {
        let name = name.as_str();
        let mut declaring = self.declaring(name);
        let (origin, winner) = declaring.next()?;
        let value = quoted_value(winner);
        let label = origin.label();
        let shadowed: Vec<Cow<'_, str>> = declaring.map(|(origin, _)| origin.label()).collect();
        let over = if shadowed.is_empty() {
            String::new()
        } else {
            format!("; over {}", shadowed.join(", "))
        };
        Some(format!("{name:<width$} = {value} ({label}{over})"))
    }
}

/// List variables for the selected leaf repository, without CLI overrides.
pub(crate) fn list(roots: &Roots, env: &Environment, reporter: &Reporter) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    VarSet::resolve(&manifest.vars, &roots.state, env, &[], reporter)?.list(reporter);
    Ok(())
}

/// List only persisted machine variables; do not read the repository or environment.
pub(crate) fn list_machine(state: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let machine = MachineVars::load(&state.machine_vars())?;
    VarSet::stack(BTreeMap::new(), machine.values, BTreeMap::new(), &[]).list(reporter);
    Ok(())
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
mod tests {
    use super::*;

    fn name(name: &str) -> VarName {
        VarName::try_from(name.to_owned()).expect("valid name")
    }

    fn layer<const N: usize>(pairs: [(&str, &str); N]) -> BTreeMap<VarName, String> {
        pairs
            .into_iter()
            .map(|(key, value)| (name(key), value.to_owned()))
            .collect()
    }

    fn command_line<const N: usize>(pairs: [(&str, &str); N]) -> Vec<(VarName, String)> {
        pairs
            .into_iter()
            .map(|(key, value)| (name(key), value.to_owned()))
            .collect()
    }

    /// The four layers, in precedence order.
    fn stacked<const A: usize, const B: usize, const C: usize, const D: usize>(
        manifest: [(&str, &str); A],
        machine: [(&str, &str); B],
        environment: [(&str, &str); C],
        cli: [(&str, &str); D],
    ) -> VarSet {
        VarSet::stack(
            layer(manifest),
            layer(machine),
            layer(environment),
            &command_line(cli),
        )
    }

    /// The value in force and where it came from.
    fn winner(set: &VarSet, key: &str) -> (String, Origin) {
        let (origin, value) = set
            .declaring(key)
            .next()
            .expect("some layer should declare it");
        (value.to_owned(), origin.clone())
    }

    /// The layers that value overrode, highest first.
    fn shadowed(set: &VarSet, key: &str) -> Vec<Origin> {
        set.declaring(key)
            .skip(1)
            .map(|(origin, _)| origin.clone())
            .collect()
    }

    #[test]
    fn each_layer_overrides_the_one_below_it() {
        let set = stacked(
            [("a", "manifest"), ("b", "manifest"), ("c", "manifest")],
            [("a", "machine"), ("b", "machine")],
            [("a", "environment")],
            [],
        );
        assert_eq!(
            winner(&set, "a"),
            ("environment".to_owned(), Origin::Environment)
        );
        assert_eq!(winner(&set, "b"), ("machine".to_owned(), Origin::Machine));
        assert_eq!(winner(&set, "c"), ("manifest".to_owned(), Origin::Manifest));
    }

    #[test]
    fn the_command_line_wins_over_every_other_layer() {
        let set = stacked(
            [("a", "manifest")],
            [("a", "machine")],
            [("a", "environment")],
            [("a", "cli")],
        );
        assert_eq!(winner(&set, "a"), ("cli".to_owned(), Origin::CommandLine));
    }

    #[test]
    fn a_name_only_one_layer_declares_takes_that_layer_alone() {
        let set = stacked([], [], [], [("only", "cli")]);
        assert_eq!(
            winner(&set, "only"),
            ("cli".to_owned(), Origin::CommandLine)
        );
        assert!(shadowed(&set, "only").is_empty());
    }

    #[test]
    fn the_set_is_the_union_of_the_layers() {
        let set = stacked([("a", "1")], [("b", "2")], [("c", "3")], [("d", "4")]);
        assert_eq!(
            set.names()
                .into_iter()
                .map(VarName::to_string)
                .collect::<Vec<_>>(),
            ["a", "b", "c", "d"]
        );
    }

    #[test]
    fn a_name_no_layer_declares_is_declared_by_no_layer() {
        let set = stacked([("a", "1")], [], [], []);
        assert_eq!(set.declaring("absent").count(), 0);
        assert_eq!(set.get("absent"), None);
    }

    #[test]
    fn get_answers_with_the_value_in_force() {
        // Conditions read `get`; `-vv` lines walk the same layers.
        let set = stacked(
            [("a", "manifest"), ("b", "manifest")],
            [("a", "machine")],
            [],
            [],
        );
        assert_eq!(set.get("a"), Some("machine"));
        assert_eq!(set.get("b"), Some("manifest"));
    }

    #[test]
    fn get_tells_a_declared_empty_value_from_an_undeclared_name() {
        // A bare identifier reads `""` as false and `None` as an undeclared
        // error.
        let set = stacked([], [], [], [("empty", "")]);
        assert_eq!(set.get("empty"), Some(""));
        assert_eq!(set.get("missing"), None);
    }

    #[test]
    fn a_value_outlives_the_name_it_was_looked_up_by() {
        // Checked at compile time: the result borrows only from the set.
        let set = stacked([("a", "1")], [], [], []);
        let value = {
            let name = "a".to_owned();
            set.get(&name)
        };
        assert_eq!(value, Some("1"));
    }

    #[test]
    fn get_accepts_text_that_is_not_a_valid_name() {
        // A condition can index `vars` with any text.
        let set = stacked([("a", "1")], [], [], []);
        assert_eq!(set.get("has-dash"), None);
        assert_eq!(set.get(""), None);
    }

    #[test]
    fn an_empty_value_overrides_like_any_other() {
        // `--var profile=` declares `profile` as the empty string.
        let set = stacked([("profile", "personal")], [], [], [("profile", "")]);
        assert_eq!(
            winner(&set, "profile"),
            (String::new(), Origin::CommandLine)
        );
    }

    #[test]
    fn a_repeated_command_line_value_keeps_the_last_one_written() {
        let set = stacked([], [], [], [("p", "first"), ("p", "second"), ("q", "only")]);
        assert_eq!(
            winner(&set, "p"),
            ("second".to_owned(), Origin::CommandLine)
        );
        assert_eq!(winner(&set, "q"), ("only".to_owned(), Origin::CommandLine));
    }

    #[test]
    fn a_repeated_command_line_value_is_one_layer_and_not_two() {
        let set = stacked([("p", "manifest")], [], [], [("p", "first"), ("p", "last")]);
        assert_eq!(
            shadowed(&set, "p"),
            [Origin::Manifest],
            "a repeated `--var` should not shadow itself"
        );
    }

    #[test]
    fn a_variable_remembers_the_layers_it_overrode() {
        let set = stacked(
            [("a", "manifest")],
            [("a", "machine")],
            [("a", "environment")],
            [("a", "cli")],
        );
        assert_eq!(
            shadowed(&set, "a"),
            [Origin::Environment, Origin::Machine, Origin::Manifest],
            "the layers should be listed highest first"
        );
    }

    #[test]
    fn a_name_is_case_sensitive_across_layers() {
        let set = stacked([("editor", "nvim")], [("EDITOR", "vi")], [], []);
        assert_eq!(
            winner(&set, "editor"),
            ("nvim".to_owned(), Origin::Manifest)
        );
        assert_eq!(winner(&set, "EDITOR"), ("vi".to_owned(), Origin::Machine));
    }

    // What one inclusion does to the set.

    /// An inclusion label, as [`Inclusion::at`](crate::action::Inclusion::at)
    /// spells one.
    const CORP: &str = "include-remote `corp`";

    /// How a line names that inclusion's remote `[vars]`.
    const CORP_MANIFEST: &str = "batfiles.toml of include-remote `corp`";

    /// `base` derived for an inclusion with `overrides` and no remote `[vars]`.
    fn derived<const N: usize>(base: &VarSet, overrides: [(&str, &str); N]) -> VarSet {
        base.with_inclusion(&BTreeMap::new(), &layer(overrides), CORP)
    }

    /// `base` derived for an inclusion with remote `[vars]` and `overrides`.
    fn derived_from<const M: usize, const N: usize>(
        base: &VarSet,
        remote: [(&str, &str); M],
        overrides: [(&str, &str); N],
    ) -> VarSet {
        base.with_inclusion(&layer(remote), &layer(overrides), CORP)
    }

    /// The lines [`VarSet::report_inclusion`] would print.
    fn override_lines(set: &VarSet) -> Vec<String> {
        match set.inclusion_block() {
            Some((_, names)) => set.lines_for(names),
            None => Vec::new(),
        }
    }

    /// The heading that block is printed under.
    fn block_heading(set: &VarSet) -> Option<String> {
        set.inclusion_block().map(|(heading, _)| heading)
    }

    /// Every layer's origin, lowest precedence first.
    fn origins(set: &VarSet) -> Vec<Origin> {
        set.layers
            .iter()
            .map(|layer| layer.origin.clone())
            .collect()
    }

    #[test]
    fn a_derived_scope_places_each_of_its_layers_by_origin() {
        let set = derived_from(&stacked([], [], [], []), [("a", "remote")], [("b", "corp")]);
        assert_eq!(
            origins(&set),
            [
                Origin::IncludedManifest(CORP.to_owned()),
                Origin::Manifest,
                Origin::Inclusion(CORP.to_owned()),
                Origin::Machine,
                Origin::Environment,
                Origin::CommandLine,
            ]
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "derived from the run's own set")]
    fn a_scope_is_not_derived_from_another_scope() {
        // Deriving from a scope would stack two inclusions' layers into one.
        let corp = derived(&stacked([], [], [], []), [("profile", "work")]);
        let _ = derived(&corp, [("profile", "lab")]);
    }

    #[test]
    fn an_override_beats_the_leaf_and_loses_to_the_machine() {
        let base = stacked(
            [("a", "leaf"), ("b", "leaf"), ("c", "leaf"), ("d", "leaf")],
            [("b", "machine")],
            [("c", "environment")],
            [("d", "cli")],
        );
        let set = derived(
            &base,
            [("a", "corp"), ("b", "corp"), ("c", "corp"), ("d", "corp")],
        );
        assert_eq!(
            winner(&set, "a"),
            ("corp".to_owned(), Origin::Inclusion(CORP.to_owned()))
        );
        assert_eq!(winner(&set, "b"), ("machine".to_owned(), Origin::Machine));
        assert_eq!(
            winner(&set, "c"),
            ("environment".to_owned(), Origin::Environment)
        );
        assert_eq!(winner(&set, "d"), ("cli".to_owned(), Origin::CommandLine));
    }

    #[test]
    fn an_override_adds_a_name_no_other_layer_declares() {
        let set = derived(&stacked([], [], [], []), [("profile", "work")]);
        assert_eq!(set.get("profile"), Some("work"));
        assert!(shadowed(&set, "profile").is_empty());
    }

    #[test]
    fn deriving_a_scope_leaves_the_runs_own_set_alone() {
        let base = stacked([("profile", "personal")], [], [], []);
        let work = derived(&base, [("profile", "work")]);
        let other = derived(&base, [("profile", "lab")]);
        assert_eq!(base.get("profile"), Some("personal"));
        assert_eq!(work.get("profile"), Some("work"));
        assert_eq!(other.get("profile"), Some("lab"));
    }

    #[test]
    fn an_override_block_lists_the_names_the_inclusion_declared() {
        // Only those; the rest of the set is in the run's own block.
        let base = stacked([("editor", "vi"), ("profile", "personal")], [], [], []);
        let set = derived(&base, [("profile", "work")]);
        assert_eq!(
            override_lines(&set),
            ["profile = \"work\" (include-remote `corp`; over batfiles.toml)"]
        );
    }

    #[test]
    fn an_override_a_higher_layer_beat_is_listed_with_the_layer_that_won() {
        let base = stacked([("profile", "personal")], [("profile", "machine")], [], []);
        let set = derived(&base, [("profile", "work")]);
        assert_eq!(
            override_lines(&set),
            ["profile = \"machine\" (vars.toml; over include-remote `corp`, batfiles.toml)"]
        );
    }

    #[test]
    fn a_set_no_inclusion_derived_has_no_block_of_its_own() {
        assert!(override_lines(&stacked([("a", "1")], [], [], [])).is_empty());
    }

    #[test]
    fn an_override_map_declaring_nothing_has_nothing_to_report() {
        let set = derived(&stacked([("a", "1")], [], [], []), []);
        assert!(override_lines(&set).is_empty());
        assert_eq!(set.get("a"), Some("1"));
    }

    // What the included remote's own `[vars]` does to the set.

    #[test]
    fn an_included_remotes_vars_lose_to_every_other_layer() {
        let base = stacked(
            [("b", "leaf")],
            [("c", "machine")],
            [("d", "environment")],
            [("e", "cli")],
        );
        let set = derived_from(
            &base,
            [
                ("a", "remote"),
                ("b", "remote"),
                ("c", "remote"),
                ("d", "remote"),
                ("e", "remote"),
            ],
            [("a", "corp")],
        );
        assert_eq!(
            winner(&set, "a"),
            ("corp".to_owned(), Origin::Inclusion(CORP.to_owned()))
        );
        assert_eq!(winner(&set, "b"), ("leaf".to_owned(), Origin::Manifest));
        assert_eq!(winner(&set, "c"), ("machine".to_owned(), Origin::Machine));
        assert_eq!(
            winner(&set, "d"),
            ("environment".to_owned(), Origin::Environment)
        );
        assert_eq!(winner(&set, "e"), ("cli".to_owned(), Origin::CommandLine));
    }

    #[test]
    fn an_included_remotes_declaration_stands_where_nothing_overrides_it() {
        let set = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
        assert_eq!(
            winner(&set, "profile"),
            ("work".to_owned(), Origin::IncludedManifest(CORP.to_owned()))
        );
        assert!(shadowed(&set, "profile").is_empty());
    }

    #[test]
    fn an_included_remotes_vars_stay_inside_the_scope_they_were_read_into() {
        let base = stacked([], [], [], []);
        let corp = derived_from(&base, [("profile", "work")], []);
        let other = derived_from(&base, [("profile", "lab")], []);
        assert_eq!(base.get("profile"), None);
        assert_eq!(corp.get("profile"), Some("work"));
        assert_eq!(other.get("profile"), Some("lab"));
    }

    #[test]
    fn a_remotes_declaration_is_listed_under_the_inclusion_that_opened_it() {
        // Every repository's manifest is a `batfiles.toml`.
        let set = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
        assert_eq!(
            override_lines(&set),
            [format!("profile = \"work\" ({CORP_MANIFEST})")]
        );
    }

    #[test]
    fn a_remotes_declaration_the_leaf_overrode_reads_as_having_lost() {
        let base = stacked([("profile", "personal")], [], [], []);
        let set = derived_from(&base, [("profile", "work"), ("editor", "vi")], []);
        assert_eq!(
            override_lines(&set),
            [
                format!("editor  = \"vi\" ({CORP_MANIFEST})"),
                format!("profile = \"personal\" (batfiles.toml; over {CORP_MANIFEST})"),
            ]
        );
    }

    #[test]
    fn the_block_is_headed_by_the_inclusion_and_not_by_either_document() {
        // Not `batfiles.toml of include-remote `corp``.
        let set = derived_from(&stacked([], [], [], []), [("a", "remote")], [("b", "corp")]);
        assert_eq!(block_heading(&set), Some(format!("{CORP} variables:")));
        assert_eq!(block_heading(&stacked([("a", "1")], [], [], [])), None);
    }

    #[test]
    fn an_inclusion_overriding_nothing_still_reports_what_its_remote_declared() {
        let set = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
        assert_eq!(set.derived_layers().count(), 2);
        assert!(!override_lines(&set).is_empty());
    }

    // What `-vv` shows.

    #[test]
    fn a_line_gives_the_value_its_layer_and_what_it_overrode() {
        let set = stacked(
            [("editor", "vi"), ("profile", "personal")],
            [("editor", "nvim")],
            [],
            [("editor", "emacs")],
        );
        assert_eq!(
            set.lines(),
            [
                "editor  = \"emacs\" (--var; over vars.toml, batfiles.toml)",
                "profile = \"personal\" (batfiles.toml)",
            ]
        );
    }

    #[test]
    fn an_empty_value_is_visible_in_a_line() {
        // Quoted, so it does not read as a missing value.
        let set = stacked([], [], [], [("profile", "")]);
        assert_eq!(set.lines(), ["profile = \"\" (--var)"]);
    }

    #[test]
    fn an_empty_set_has_nothing_to_show() {
        assert!(stacked([], [], [], []).lines().is_empty());
    }
}
