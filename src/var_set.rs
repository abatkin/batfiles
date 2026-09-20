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

/// The detail level the set is reported at: `-vv`, since `-v` is an account of
/// what a run did and this is the inputs it worked from.
const DETAIL: u8 = 2;

/// What a manifest's `[vars]` is named by. The leaf's answers to it alone; an
/// included one is this and the inclusion that opened it.
const MANIFEST_LABEL: &str = "batfiles.toml";

/// Where a declaration came from. The order is the precedence order, lowest
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The `[vars]` of a manifest one `include-remote` opened, reaching what
    /// that inclusion contributed and nothing else. The label is the
    /// inclusion's, not the remote's or the document's. One set holds at most
    /// one of these.
    IncludedManifest(String),
    /// The leaf repository's `[vars]`.
    Manifest,
    /// One `include-remote`'s own `vars` overrides, reaching what that
    /// inclusion contributed and nothing else. The label is the inclusion the
    /// overrides were written on, since two inclusions writing overrides are
    /// two sources. One set holds at most one of these.
    Inclusion(String),
    /// The machine-local `vars.toml`.
    Machine,
    /// A `BATFILES_VAR_*` variable in this run's environment.
    Environment,
    /// A `--var` on this command line.
    CommandLine,
}

impl Origin {
    /// The document, input channel, or record to name in a report. An included
    /// manifest names both the document and the inclusion that opened it.
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

    /// The inclusion this layer was [derived](VarSet::with_inclusion) for, or
    /// `None` for a layer of the run's own set. The label itself, not the
    /// rendering [`Self::label`] wraps it in: this is what heads the block both
    /// of an inclusion's layers are reported in.
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
    /// Read the layers a run does not already hold, and stack all four.
    ///
    /// `manifest` is the leaf repository's `[vars]`, already parsed, and `cli`
    /// is every `--var` in the order it was written. The machine layer is read
    /// from `roots`, so an unreadable or malformed `vars.toml` fails the run;
    /// the environment layer warns about names it cannot use and drops them,
    /// which is [`env_vars`]' rule rather than this function's.
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

    /// The same layers with one inclusion's two of its own added: the included
    /// remote's `[vars]` beneath them all, and the inclusion's `vars` overrides
    /// directly above the leaf's `[vars]`. Both are the places
    /// [`docs/environment.md`](../docs/environment.md#variable-precedence) gives
    /// them. This is the scope every record that inclusion contributed is
    /// decided against, in place of the run's own. `label` is how a report names
    /// the inclusion, and names both layers.
    ///
    /// Must be called on the run's own set rather than on another derived one:
    /// a set holds at most one layer of each kind. One derived set per opened
    /// inclusion, not one per record it contributed.
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
            // `stack` always writes a leaf layer, empty `[vars]` or not, so
            // this is reached once and the overrides are never dropped.
            if leaf {
                layers.push(Layer {
                    origin: Origin::Inclusion(label.to_owned()),
                    values: overrides.clone(),
                });
            }
        }
        Self { layers }
    }

    /// The run's own layers, which is what an inclusion scope is derived from.
    ///
    /// Requires a base scope. Panics in debug builds if it contains inclusion
    /// layers; release builds omit those layers.
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

    /// Report how one inclusion's scope differs from the run's set, at `-vv`
    /// under a heading naming the inclusion: the names its own two layers
    /// declare and no others, each with the layer in force, so a declaration a
    /// higher layer beat is listed as having lost. A set no inclusion derived
    /// produces no output.
    pub fn report_inclusion(&self, reporter: &Reporter) {
        let Some((heading, names)) = self.inclusion_block() else {
            return;
        };
        self.report_lines(&heading, names, reporter);
    }

    /// The heading and the names of that block, or `None` for a set no
    /// inclusion derived.
    fn inclusion_block(&self) -> Option<(String, BTreeSet<&VarName>)> {
        let mut inclusion = None;
        let mut names = BTreeSet::new();
        for layer in self.derived_layers() {
            // Both layers carry the same inclusion label, so either answers for
            // the heading; it is that label rather than `Origin::label`'s
            // rendering of it.
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

    /// One `-vv` block: the heading, then a line for each name, indented under
    /// it. Nothing at all where no name has a line or the detail is not shown.
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

    /// One line for each of `names`, aligned against each other: a block is as
    /// wide as the names in it rather than as wide as the set they came from.
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

    /// The line for one name: the value in force, the layer it came from, and
    /// the layers it overrode. `None` where no layer declares the name, which
    /// is not a name [`Self::names`] produces.
    ///
    /// A name is a checked [`VarName`] and needs nothing done to it. A value is
    /// whatever a repository, a state file, or the environment put there, so it
    /// goes through [`quoted_value`].
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
        // The same value `get` answers with, which is asserted below.
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
        // What a condition reads, and the reason it cannot disagree with a
        // `-vv` line: both are the same walk down the layers.
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
        // `""` and `None` are the two answers a bare identifier turns into a
        // false condition and an undeclared-identifier error respectively, so
        // collapsing them here would collapse them there.
        let set = stacked([], [], [], [("empty", "")]);
        assert_eq!(set.get("empty"), Some(""));
        assert_eq!(set.get("missing"), None);
    }

    #[test]
    fn a_value_outlives_the_name_it_was_looked_up_by() {
        // A compile-time property rather than a runtime one: the answer borrows
        // from the set, so a caller holding it does not also have to keep the
        // string it asked with -- a condition indexing `vars` builds those.
        let set = stacked([("a", "1")], [], [], []);
        let value = {
            let name = "a".to_owned();
            set.get(&name)
        };
        assert_eq!(value, Some("1"));
    }

    #[test]
    fn get_accepts_text_that_is_not_a_valid_name() {
        // The `vars` namespace can be indexed with anything, and no layer can
        // hold a key like this, so the answer is always that nothing declares
        // it -- not a panic on the way to finding out.
        let set = stacked([("a", "1")], [], [], []);
        assert_eq!(set.get("has-dash"), None);
        assert_eq!(set.get(""), None);
    }

    #[test]
    fn an_empty_value_overrides_like_any_other() {
        // The trap in a precedence rule written as "the highest value set":
        // `--var profile=` sets `profile`, and it sets it to the empty string.
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
        // A layer is a map, so it cannot hold a name twice however many times
        // the name was written: the account of a repeated `--var` says `--var`
        // once.
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
        // Two names, not one: the rule `var.rs` sets, applied where it decides
        // whether a layer overrides another or adds to it.
        let set = stacked([("editor", "nvim")], [("EDITOR", "vi")], [], []);
        assert_eq!(
            winner(&set, "editor"),
            ("nvim".to_owned(), Origin::Manifest)
        );
        assert_eq!(winner(&set, "EDITOR"), ("vi".to_owned(), Origin::Machine));
    }

    // What one inclusion does to the set.

    /// The inclusion's own label, as `include_remote::label` spells one.
    const CORP: &str = "include-remote `corp`";

    /// How a line names the `[vars]` of the manifest that inclusion opened.
    const CORP_MANIFEST: &str = "batfiles.toml of include-remote `corp`";

    /// The set a record contributed by an inclusion writing `overrides` is
    /// decided against, where the remote it opened declared nothing of its own.
    fn derived<const N: usize>(base: &VarSet, overrides: [(&str, &str); N]) -> VarSet {
        base.with_inclusion(&BTreeMap::new(), &layer(overrides), CORP)
    }

    /// The same, for an inclusion of a remote that declared `remote` in its own
    /// `[vars]`.
    fn derived_from<const M: usize, const N: usize>(
        base: &VarSet,
        remote: [(&str, &str); M],
        overrides: [(&str, &str); N],
    ) -> VarSet {
        base.with_inclusion(&layer(remote), &layer(overrides), CORP)
    }

    /// The lines [`VarSet::report_inclusion`] would print, without a reporter to
    /// print them to: the names the inclusion's two layers declare and nothing
    /// else.
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
        // Caught rather than answered: a set holds at most one layer of each
        // kind, and deriving from a scope would give this inclusion two of
        // each, decide its records partly against the other inclusion's values,
        // and head one `-vv` block with both names.
        let corp = derived(&stacked([], [], [], []), [("profile", "work")]);
        let _ = derived(&corp, [("profile", "lab")]);
    }

    #[test]
    fn an_override_beats_the_leaf_and_loses_to_the_machine() {
        // Where the overrides sit, read from both sides: the leaf `[vars]` an
        // inclusion may override, and the three layers no inclusion reaches
        // past.
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
        // Two inclusions writing different overrides are two scopes, and both
        // are derived from the same set: one of them changing it would decide
        // the other's records, and the leaf's.
        let base = stacked([("profile", "personal")], [], [], []);
        let work = derived(&base, [("profile", "work")]);
        let other = derived(&base, [("profile", "lab")]);
        assert_eq!(base.get("profile"), Some("personal"));
        assert_eq!(work.get("profile"), Some("work"));
        assert_eq!(other.get("profile"), Some("lab"));
    }

    #[test]
    fn an_override_block_lists_the_names_the_inclusion_declared() {
        // Only those: the block is as long as the record is, rather than
        // repeating the whole set under every inclusion.
        let base = stacked([("editor", "vi"), ("profile", "personal")], [], [], []);
        let set = derived(&base, [("profile", "work")]);
        assert_eq!(
            override_lines(&set),
            ["profile = \"work\" (include-remote `corp`; over batfiles.toml)"]
        );
    }

    #[test]
    fn an_override_a_higher_layer_beat_is_listed_with_the_layer_that_won() {
        // The reason such a name stays in the block: that the override lost to
        // `vars.toml` is the thing worth seeing.
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
        // The lowest layer, read against all five above it: a leaf composing a
        // remote overrides what it declares without having to know it is there.
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
        // What the layer is for: a remote's records decide against the values
        // that remote wrote, wherever the machine it is being installed on has
        // nothing to say about them.
        let set = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
        assert_eq!(
            winner(&set, "profile"),
            ("work".to_owned(), Origin::IncludedManifest(CORP.to_owned()))
        );
        assert!(shadowed(&set, "profile").is_empty());
    }

    #[test]
    fn an_included_remotes_vars_stay_inside_the_scope_they_were_read_into() {
        // The leaf's own records, and a second inclusion's, are decided against
        // sets that never saw this remote's declarations.
        let base = stacked([], [], [], []);
        let corp = derived_from(&base, [("profile", "work")], []);
        let other = derived_from(&base, [("profile", "lab")], []);
        assert_eq!(base.get("profile"), None);
        assert_eq!(corp.get("profile"), Some("work"));
        assert_eq!(other.get("profile"), Some("lab"));
    }

    #[test]
    fn a_remotes_declaration_is_listed_under_the_inclusion_that_opened_it() {
        // `batfiles.toml` alone names three documents in a run including two
        // remotes, so the line names the inclusion the block is headed by.
        let set = derived_from(&stacked([], [], [], []), [("profile", "work")], []);
        assert_eq!(
            override_lines(&set),
            [format!("profile = \"work\" ({CORP_MANIFEST})")]
        );
    }

    #[test]
    fn a_remotes_declaration_the_leaf_overrode_reads_as_having_lost() {
        // Both layers' names are in the block, and each line names the layer in
        // force: this is what a leaf composing a remote looks like from inside
        // the scope.
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
        // Both layers are named for the inclusion, and one of them renders that
        // name as a document: the heading is the inclusion's own label, so the
        // block is not headed `batfiles.toml of include-remote `corp``.
        let set = derived_from(&stacked([], [], [], []), [("a", "remote")], [("b", "corp")]);
        assert_eq!(block_heading(&set), Some(format!("{CORP} variables:")));
        assert_eq!(block_heading(&stacked([("a", "1")], [], [], [])), None);
    }

    #[test]
    fn an_inclusion_overriding_nothing_still_reports_what_its_remote_declared() {
        // The block is the account of how this scope differs from the run's
        // set, and a remote declaring variables of its own is a difference
        // whether or not the leaf wrote anything on the record.
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
        // Quoted for exactly this: an unquoted empty value would read as a
        // variable with no value at all, which is a different thing.
        let set = stacked([], [], [], [("profile", "")]);
        assert_eq!(set.lines(), ["profile = \"\" (--var)"]);
    }

    #[test]
    fn an_empty_set_has_nothing_to_show() {
        assert!(stacked([], [], [], []).lines().is_empty());
    }
}
