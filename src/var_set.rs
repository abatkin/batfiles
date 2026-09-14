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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The leaf repository's `[vars]`.
    Manifest,
    /// The machine-local `vars.toml`.
    Machine,
    /// A `BATFILES_VAR_*` variable in this run's environment.
    Environment,
    /// A `--var` on this command line.
    CommandLine,
}

impl Origin {
    /// The document or input channel to name in a report.
    fn label(self) -> &'static str {
        match self {
            Self::Manifest => "batfiles.toml",
            Self::Machine => "vars.toml",
            Self::Environment => "BATFILES_VAR_*",
            Self::CommandLine => "--var",
        }
    }
}

/// One source and everything it declared.
#[derive(Debug)]
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

    /// The highest-precedence value, or `None` if no layer declares the name.
    /// A declared empty string is a value. The result borrows only from the set.
    pub fn get<'a>(&'a self, name: &str) -> Option<&'a str> {
        self.declaring(name).next().map(|(_, value)| value)
    }

    /// Declarations of arbitrary `name` text, highest precedence first.
    /// Returns an empty iterator if no layer declares it.
    fn declaring<'a>(&'a self, name: &str) -> impl Iterator<Item = (Origin, &'a str)> {
        self.layers
            .iter()
            .rev()
            .filter_map(move |layer| Some((layer.origin, layer.values.get(name)?.as_str())))
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
        if !reporter.shows_detail(DETAIL) {
            return;
        }
        let lines = self.lines();
        if lines.is_empty() {
            return;
        }
        reporter.detail(DETAIL, "variables:");
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
        let names = self.names();
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
        (value.to_owned(), origin)
    }

    /// The layers that value overrode, highest first.
    fn shadowed(set: &VarSet, key: &str) -> Vec<Origin> {
        set.declaring(key)
            .skip(1)
            .map(|(origin, _)| origin)
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
