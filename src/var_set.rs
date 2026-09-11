//! Resolve variable values and origins from manifest, machine, environment,
//! and command-line layers, in increasing precedence order.
//!
//! All layers form one scope. Keep shadowed declarations for provenance;
//! lookup takes the highest-precedence declaration, including an empty value.
//! See [`docs/environment.md`](../docs/environment.md#variable-precedence).
//!
//! Resolve the set on every action run, even without conditions. `-vv` and
//! `vars list` report the same effective values and origins.

use std::collections::{BTreeMap, BTreeSet};

use crate::env::Environment;
use crate::env_vars;
use crate::error::Error;
use crate::location::{Roots, StateRoots};
use crate::machine_vars::MachineVars;
use crate::manifest::Manifest;
use crate::output::{Reporter, quoted_value};
use crate::var::VarName;

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
    /// The layer named the way the user would go and change it, which is the
    /// only thing a report has any use for. Each name identifies its layer on
    /// its own, since every layer is one document or one channel.
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
            env_vars::one_shot(env, reporter),
            cli,
        ))
    }

    /// Stack four layers that have already been read, lowest precedence first.
    ///
    /// Separate from [`Self::resolve`] because which layers a command reads is
    /// the command's question: [`list_machine`] reads one of them.
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
                    values: one_shot(cli),
                },
            ],
        }
    }

    /// The value in force for `name`, or `None` where no layer declares it.
    ///
    /// This is the precedence rule itself rather than a lookup into a flattened
    /// copy of it, so a condition reading a variable and a `-vv` line reporting
    /// one cannot disagree. `None` is what an undeclared identifier is
    /// diagnosed from; it is not the same answer as a declared empty value.
    ///
    /// The value borrows from `name` as well as from the set, because
    /// [`Self::declaring`] holds the name while it walks. Callers read the
    /// answer immediately, so the shorter borrow costs them nothing.
    pub fn get<'a>(&'a self, name: &'a str) -> Option<&'a str> {
        let layer = self.declaring(name).next()?;
        Some(&layer.values[name])
    }

    /// The layers declaring `name`, highest precedence first: the first is the
    /// value in force and the rest are the ones it overrode. An empty iterator
    /// means no layer declared the name at all.
    ///
    /// The name is arbitrary text, because a condition can index the `vars`
    /// namespace with anything at all.
    fn declaring<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Layer> {
        self.layers
            .iter()
            .rev()
            .filter(move |layer| layer.values.contains_key(name))
    }

    /// Every name any layer declared, in order and without repeats.
    fn names(&self) -> BTreeSet<&VarName> {
        self.layers
            .iter()
            .flat_map(|layer| layer.values.keys())
            .collect()
    }

    /// Show the set at `-vv`, the way `-v` shows the resolved roots: precedence
    /// is hard to reason about from outside, so a run can be asked what it
    /// worked out rather than having it inferred.
    ///
    /// This prints values, unlike the machine-local commands' outcome lines,
    /// which deliberately name a key and never its value. The difference is what
    /// was asked for: `-vv` is a request for exactly this, and a listing that
    /// withheld the values could not show which layer won.
    ///
    /// Indented under a heading, which is what separates it from [`Self::list`]:
    /// here the set is detail about a run doing something else, and there it is
    /// the whole of what was asked for.
    pub fn report(&self, reporter: &Reporter) {
        let lines = self.lines();
        if lines.is_empty() {
            return;
        }
        reporter.detail(2, "variables:");
        for line in lines {
            reporter.detail(2, &format!("  {line}"));
        }
    }

    /// Answer `vars list` with the set, on standard output.
    ///
    /// The same lines `-vv` reports, from the same walk down the layers, so the
    /// command and the run cannot disagree about what a variable is worth or
    /// which layer decided it. Precedence is the reason to run the command, so
    /// each line carries its origin rather than being made shell-parseable.
    ///
    /// A set with nothing in it prints nothing at all, since standard output
    /// carries data and there is none; the account of that goes to standard
    /// error like every other line describing what a command did.
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
        let winner = declaring.next()?;
        let value = quoted_value(&winner.values[name]);
        let label = winner.origin.label();
        let shadowed: Vec<&str> = declaring.map(|layer| layer.origin.label()).collect();
        let over = if shadowed.is_empty() {
            String::new()
        } else {
            format!("; over {}", shadowed.join(", "))
        };
        Some(format!("{name:<width$} = {value} ({label}{over})"))
    }
}

/// Answer `vars list` for the selected leaf repository.
///
/// Every layer a run resolves except the command line, which `vars list` does
/// not accept: listing the variables of an invocation that set one would say
/// less about the machine than about the invocation.
pub(crate) fn list(roots: &Roots, env: &Environment, reporter: &Reporter) -> Result<(), Error> {
    let manifest = Manifest::load(&roots.manifest())?;
    VarSet::resolve(&manifest.vars, &roots.state, env, &[], reporter)?.list(reporter);
    Ok(())
}

/// Answer `vars list --machine-only` from `vars.toml` alone.
///
/// One layer, so nothing here can be overridden and no repository or process
/// environment is consulted: the answer is what this machine has persisted, and
/// every line of it names a variable [`vars unset`](crate::machine_vars::unset)
/// would remove.
pub(crate) fn list_machine(state: &StateRoots, reporter: &Reporter) -> Result<(), Error> {
    let machine = MachineVars::load(&state.machine_vars())?;
    VarSet::stack(BTreeMap::new(), machine.values, BTreeMap::new(), &[]).list(reporter);
    Ok(())
}

/// The command line as a layer.
///
/// It is the one layer that can name a variable twice, and inserting the pairs
/// in the order they were written is what settles it: the last value wins,
/// which is the rule between layers applied within one.
fn one_shot(cli: &[(VarName, String)]) -> BTreeMap<VarName, String> {
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
        let layer = set
            .declaring(key)
            .next()
            .expect("some layer should declare it");
        // The same value `get` answers with, which is asserted below.
        (layer.values[key].clone(), layer.origin)
    }

    /// The layers that value overrode, highest first.
    fn shadowed(set: &VarSet, key: &str) -> Vec<Origin> {
        set.declaring(key)
            .skip(1)
            .map(|layer| layer.origin)
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
