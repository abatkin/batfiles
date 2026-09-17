//! Resolve variable values and origins from manifest, machine, environment,
//! and CLI layers. Preserve shadowed declarations for provenance.
//! See [`docs/environment.md`](../docs/environment.md#variable-precedence).

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

/// Where a declaration came from. The order is the precedence order, lowest
/// first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The leaf repository's `[vars]`.
    Manifest,
    /// One `include-remote`'s own `vars` overrides, reaching what that
    /// inclusion contributed and nothing else.
    ///
    /// The label is the record's rather than a field name, because two
    /// inclusions writing overrides are two sources: what a report has to name
    /// is which inclusion decided the value. One set holds at most one of
    /// these, since an inclusion's scope is derived from the run's own.
    Inclusion(String),
    /// The machine-local `vars.toml`.
    Machine,
    /// A `BATFILES_VAR_*` variable in this run's environment.
    Environment,
    /// A `--var` on this command line.
    CommandLine,
}

impl Origin {
    /// The document, input channel, or record to name in a report.
    fn label(&self) -> &str {
        match self {
            Self::Manifest => "batfiles.toml",
            Self::Inclusion(label) => label,
            Self::Machine => "vars.toml",
            Self::Environment => "BATFILES_VAR_*",
            Self::CommandLine => "--var",
        }
    }
}

/// One source and everything it declared.
#[derive(Debug, Clone)]
struct Layer {
    origin: Origin,
    values: BTreeMap<VarName, String>,
}

/// The effective variable set for one run: every layer, lowest precedence
/// first.
#[derive(Debug)]
pub(crate) struct VarSet {
    layers: Vec<Layer>,
}

/// Where an inclusion's overrides go: directly above the leaf's `[vars]`, which
/// is the position [`docs/environment.md`](../docs/environment.md#variable-precedence)
/// gives them.
const INCLUSION_LAYER: usize = 1;

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

    /// The same layers with one inclusion's `vars` overrides above the leaf's
    /// `[vars]`: the set every record that inclusion contributed is decided
    /// against, in place of the run's own. `label` is how a report names the
    /// inclusion.
    ///
    /// Derived from the run's set rather than from another derived one, which is
    /// the same rule that keeps inclusion one level deep: a set holds at most
    /// one override layer. There is one of these per opened inclusion that wrote
    /// overrides, not one per record it contributed.
    pub fn with_inclusion(&self, overrides: &BTreeMap<VarName, String>, label: &str) -> Self {
        let mut layers = self.layers.clone();
        layers.insert(
            INCLUSION_LAYER,
            Layer {
                origin: Origin::Inclusion(label.to_owned()),
                values: overrides.clone(),
            },
        );
        Self { layers }
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

    /// Report what one inclusion's overrides did, under a heading naming the
    /// inclusion: the names its own layer declares, and no others, so the block
    /// is as long as the record is rather than as long as the run's set.
    ///
    /// A name a higher layer also declares is still listed, with that layer in
    /// force: the override is what this block is about, and that it lost to
    /// `vars.toml` is the thing worth seeing. A set with no override layer has
    /// nothing of its own to say.
    pub fn report_inclusion(&self, reporter: &Reporter) {
        let Some(layer) = self.inclusion_layer() else {
            return;
        };
        let heading = format!("{} variables:", layer.origin.label());
        self.report_lines(&heading, layer.values.keys().collect(), reporter);
    }

    /// The override layer, for a set [derived](Self::with_inclusion) for one
    /// inclusion.
    fn inclusion_layer(&self) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|layer| matches!(layer.origin, Origin::Inclusion(_)))
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
        let shadowed: Vec<&str> = declaring.map(|(origin, _)| origin.label()).collect();
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

    // What one inclusion's overrides do to the set.

    /// The inclusion's own label, as `include_remote::label` spells one.
    const CORP: &str = "include-remote `corp`";

    /// The set a record contributed by an inclusion writing `overrides` is
    /// decided against.
    fn derived<const N: usize>(base: &VarSet, overrides: [(&str, &str); N]) -> VarSet {
        base.with_inclusion(&layer(overrides), CORP)
    }

    /// The lines [`VarSet::report_inclusion`] would print, without a reporter to
    /// print them to: the names the override layer declares and nothing else.
    fn override_lines(set: &VarSet) -> Vec<String> {
        let Some(layer) = set.inclusion_layer() else {
            return Vec::new();
        };
        set.lines_for(layer.values.keys().collect())
    }

    #[test]
    fn an_override_beats_the_leaf_and_loses_to_the_machine() {
        // The position the layer is inserted at, read from both sides: the leaf
        // `[vars]` an inclusion may override, and the three layers no inclusion
        // reaches past.
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
