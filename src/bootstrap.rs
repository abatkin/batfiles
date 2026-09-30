//! Bootstrap disabled state for `clone`. Apply eligible `[default-disabled]` entries only when
//! `disabled.toml` is absent, then apply environment and CLI decisions in [precedence
//! order](../docs/environment.md#bootstrap-enable-and-disable-lists).

use crate::cli::BootstrapOptions;
use crate::condition::{Bindings, Gate};
use crate::disabled::{Change, DisabledItems, apply, outcome};
use crate::env::Environment;
use crate::error::Error;
use crate::item::{ItemAddress, ItemKind};
use crate::location::StateRoots;
use crate::manifest::default_disabled::DefaultDisabled;
use crate::output::Reporter;
use crate::paths;

/// Warning suffix for a candidate whose condition cannot be evaluated.
const NOT_DISABLED: &str = "it is left enabled";

/// The origin reported for a candidate the repository declared.
const DECLARED: &str = "default-disabled";

/// Explicit enable/disable decisions for `clone`, in precedence order.
#[derive(Debug)]
pub(crate) struct BootstrapDecisions {
    decisions: Vec<Decision>,
}

/// An enable/disable decision with its item kind, address, and origin.
#[derive(Debug)]
struct Decision {
    kind: ItemKind,
    change: Change,
    origin: &'static str,
    name: ItemAddress,
}

impl BootstrapDecisions {
    /// Read bootstrap environment variables and CLI options in precedence order. Invalid option
    /// values fail; invalid environment values warn and are dropped.
    pub fn read(
        options: &BootstrapOptions,
        env: &Environment,
        reporter: &Reporter,
    ) -> Result<Self, Error> {
        use Change::{Disable, Enable};
        use ItemKind::{Action, Group};

        let mut decisions = Vec::new();
        // Environment precedes CLI; within each, enable overrides disable.
        for (kind, change, variable) in [
            (Action, Disable, "BATFILES_DISABLE_ACTIONS"),
            (Group, Disable, "BATFILES_DISABLE_GROUPS"),
            (Action, Enable, "BATFILES_ENABLE_ACTIONS"),
            (Group, Enable, "BATFILES_ENABLE_GROUPS"),
        ] {
            for value in env.list(variable) {
                match ItemAddress::try_from(value) {
                    Ok(name) => decisions.push(Decision {
                        kind,
                        change,
                        origin: variable,
                        name,
                    }),
                    Err(error) => reporter.warn(&format!("{variable}: {error}")),
                }
            }
        }
        for (kind, change, option, values) in [
            (
                Action,
                Disable,
                "--disable-action",
                &options.disable_actions,
            ),
            (Group, Disable, "--disable-group", &options.disable_groups),
            (Action, Enable, "--enable-action", &options.enable_actions),
            (Group, Enable, "--enable-group", &options.enable_groups),
        ] {
            for value in values {
                decisions.push(Decision {
                    kind,
                    change,
                    origin: option,
                    name: ItemAddress::try_from(value.clone())?,
                });
            }
        }
        Ok(Self { decisions })
    }

    /// Apply bootstrap decisions and report each one. Save disabled state only when it changes.
    pub fn adopt(
        &self,
        candidates: &DefaultDisabled,
        bindings: &Bindings<'_>,
        roots: &StateRoots,
        reporter: &Reporter,
    ) -> Result<(), Error> {
        let path = roots.disabled_path();
        // An existing empty document still means bootstrap defaults were already considered.
        let offered = !paths::occupied(&path)?;
        let mut disabled = DisabledItems::load(&path)?;

        let mut lines = Vec::new();
        let mut changed = false;

        if offered {
            for (kind, name, gate) in entries(candidates) {
                if let Some(exclusion) =
                    gate.and_then(|gate| gate.exclusion(bindings, Some(NOT_DISABLED)))
                {
                    exclusion.report_heading(reporter, &heading(kind, name));
                    continue;
                }
                let moved = apply(disabled.list_mut(kind), Change::Disable, name);
                changed |= moved;
                lines.push(line(kind, Change::Disable, name, moved, DECLARED));
            }
        }

        for decision in &self.decisions {
            let moved = apply(
                disabled.list_mut(decision.kind),
                decision.change,
                &decision.name,
            );
            changed |= moved;
            lines.push(line(
                decision.kind,
                decision.change,
                &decision.name,
                moved,
                decision.origin,
            ));
        }

        if changed {
            disabled.save(&path)?;
        }
        for line in &lines {
            reporter.info(line);
        }
        Ok(())
    }
}

/// Iterate declared candidates and their conditions: actions first, then groups.
fn entries(
    candidates: &DefaultDisabled,
) -> impl Iterator<Item = (ItemKind, &ItemAddress, Option<Gate<'_>>)> {
    let actions = candidates
        .actions
        .iter()
        .map(|entry| (ItemKind::Action, &entry.id, entry.gate()));
    let groups = candidates
        .groups
        .iter()
        .map(|entry| (ItemKind::Group, &entry.group, entry.gate()));
    actions.chain(groups)
}

/// Format a candidate's kind and address for diagnostics.
fn heading(kind: ItemKind, name: &ItemAddress) -> String {
    format!("candidate {kind} `{name}`")
}

/// Format a decision with its origin and outcome.
fn line(
    kind: ItemKind,
    change: Change,
    name: &ItemAddress,
    moved: bool,
    origin: &'static str,
) -> String {
    format!("{origin}: {}", outcome(kind, change, name, moved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::HostNamespaces;
    use crate::output::Verbosity;
    use crate::var_set::VarSet;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    /// Create a reporter that suppresses informational output.
    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(Verbosity::Quiet);
        reporter
    }

    fn options<const N: usize>(pairs: [(&str, &str); N]) -> BootstrapOptions {
        let mut options = BootstrapOptions {
            enable_actions: Vec::new(),
            disable_actions: Vec::new(),
            enable_groups: Vec::new(),
            disable_groups: Vec::new(),
        };
        for (option, value) in pairs {
            let list = match option {
                "--enable-action" => &mut options.enable_actions,
                "--disable-action" => &mut options.disable_actions,
                "--enable-group" => &mut options.enable_groups,
                "--disable-group" => &mut options.disable_groups,
                other => panic!("no such option: {other}"),
            };
            list.push(value.to_owned());
        }
        options
    }

    fn env<const N: usize>(pairs: [(&str, &str); N]) -> Environment {
        Environment::from_pairs(pairs)
    }

    fn read<const N: usize, const M: usize>(
        pairs: [(&str, &str); N],
        variables: [(&str, &str); M],
    ) -> BootstrapDecisions {
        BootstrapDecisions::read(&options(pairs), &env(variables), &quiet())
            .expect("valid addresses")
    }

    /// Format each decision as `origin name`.
    fn decisions(bootstrap: &BootstrapDecisions) -> Vec<String> {
        bootstrap
            .decisions
            .iter()
            .map(|decision| format!("{} {}", decision.origin, decision.name))
            .collect()
    }

    /// Parse the `[default-disabled]` section from a complete manifest.
    fn candidates(manifest: &str) -> DefaultDisabled {
        toml::from_str::<crate::manifest::Manifest>(manifest)
            .expect("valid candidates")
            .default_disabled
    }

    /// A machine whose state roots are a fresh temporary directory.
    fn machine(dir: &tempfile::TempDir) -> StateRoots {
        StateRoots {
            config_dir: dir.path().to_path_buf(),
            cache_dir: dir.path().to_path_buf(),
        }
    }

    /// Adopt against the given variables, and answer with the document that was
    /// left behind, or `None` where none was written.
    fn adopt(
        bootstrap: &BootstrapDecisions,
        candidates: &DefaultDisabled,
        vars: &[(&str, &str)],
        dir: &tempfile::TempDir,
    ) -> Option<DisabledItems> {
        let values: BTreeMap<_, _> = vars
            .iter()
            .map(|(name, value)| {
                (
                    crate::var::VarName::try_from((*name).to_owned()).expect("valid name"),
                    crate::var_set::VarValue::Static((*value).to_owned()),
                )
            })
            .collect();
        let variables = Rc::new(VarSet::stack(values, BTreeMap::new(), BTreeMap::new(), &[]));
        let host = HostNamespaces::capture(&env([]));
        let roots = machine(dir);
        bootstrap
            .adopt(
                candidates,
                &Bindings::new(&variables, &host),
                &roots,
                &quiet(),
            )
            .expect("adoption should succeed");
        roots
            .disabled_path()
            .exists()
            .then(|| DisabledItems::load(&roots.disabled_path()).expect("the written document"))
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn names(addresses: &std::collections::BTreeSet<ItemAddress>) -> Vec<String> {
        addresses.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn the_decisions_are_ordered_by_the_precedence_they_apply_in() {
        let bootstrap = read(
            [("--enable-action", "c"), ("--disable-action", "d")],
            [
                ("BATFILES_ENABLE_ACTIONS", "a"),
                ("BATFILES_DISABLE_ACTIONS", "b"),
            ],
        );
        assert_eq!(
            decisions(&bootstrap),
            [
                "BATFILES_DISABLE_ACTIONS b",
                "BATFILES_ENABLE_ACTIONS a",
                "--disable-action d",
                "--enable-action c",
            ]
        );
    }

    #[test]
    fn both_namespaces_are_read_from_both_sources() {
        let bootstrap = read(
            [("--disable-group", "gui")],
            [("BATFILES_ENABLE_GROUPS", "shell")],
        );
        assert_eq!(
            decisions(&bootstrap),
            ["BATFILES_ENABLE_GROUPS shell", "--disable-group gui"]
        );
    }

    #[test]
    fn an_unusable_variable_name_is_dropped_and_the_rest_are_kept() {
        let bootstrap = read([], [("BATFILES_DISABLE_ACTIONS", "p10k,my action,zshrc")]);
        assert_eq!(
            decisions(&bootstrap),
            [
                "BATFILES_DISABLE_ACTIONS p10k",
                "BATFILES_DISABLE_ACTIONS zshrc"
            ]
        );
    }

    #[test]
    fn an_unusable_option_value_fails_the_command() {
        let error = BootstrapDecisions::read(
            &options([("--disable-action", "core..p10k")]),
            &env([]),
            &quiet(),
        )
        .expect_err("an empty segment is not an address");
        assert!(error.to_string().contains("`core..p10k`"), "{error}");
    }

    #[test]
    fn a_fresh_machine_takes_the_candidates_the_repository_declared() {
        let dir = temp();
        let document = adopt(
            &read([], []),
            &candidates(
                "[[default-disabled.actions]]\nid = \"p10k\"\n\n\
                 [[default-disabled.groups]]\ngroup = \"gui\"\n",
            ),
            &[],
            &dir,
        )
        .expect("a document");
        assert_eq!(names(&document.actions), ["p10k"]);
        assert_eq!(names(&document.groups), ["gui"]);
    }

    #[test]
    fn a_machine_that_already_has_a_document_is_not_offered_them() {
        let dir = temp();
        let roots = machine(&dir);
        DisabledItems::default()
            .save(&roots.disabled_path())
            .expect("an existing document");

        let document = adopt(
            &read([("--disable-group", "gui")], []),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
            &[],
            &dir,
        )
        .expect("a document");

        assert!(document.actions.is_empty());
        assert_eq!(names(&document.groups), ["gui"]);
    }

    #[test]
    fn deciding_nothing_leaves_no_document_behind() {
        let dir = temp();
        assert!(adopt(&read([], []), &candidates(""), &[], &dir).is_none());
    }

    #[test]
    fn an_explicit_enable_outranks_the_candidate_that_named_it() {
        let dir = temp();
        let document = adopt(
            &read([("--enable-action", "p10k")], []),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
            &[],
            &dir,
        );
        assert!(document.is_none_or(|document| document.actions.is_empty()));
    }

    #[test]
    fn the_command_line_outranks_the_environment() {
        let dir = temp();
        let document = adopt(
            &read(
                [("--disable-action", "p10k")],
                [("BATFILES_ENABLE_ACTIONS", "p10k")],
            ),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
            &[],
            &dir,
        )
        .expect("a document");
        assert_eq!(names(&document.actions), ["p10k"]);
    }

    #[test]
    fn enable_outranks_disable_within_one_source() {
        let dir = temp();
        let document = adopt(
            &read(
                [("--disable-action", "p10k"), ("--enable-action", "p10k")],
                [],
            ),
            &candidates(""),
            &[],
            &dir,
        );
        assert!(document.is_none_or(|document| document.actions.is_empty()));
    }

    #[test]
    fn a_candidate_is_offered_only_where_its_condition_admits_it() {
        let declared = candidates(
            "[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"slow\"\n\n\
             [[default-disabled.actions]]\nid = \"nvim\"\nunless = \"slow\"\n",
        );

        let slow = temp();
        let document = adopt(&read([], []), &declared, &[("slow", "true")], &slow)
            .expect("the `when` candidate");
        assert_eq!(names(&document.actions), ["p10k"]);

        let quick = temp();
        let document = adopt(&read([], []), &declared, &[("slow", "false")], &quick)
            .expect("the `unless` candidate");
        assert_eq!(names(&document.actions), ["nvim"]);
    }

    #[test]
    fn a_condition_this_machine_cannot_decide_leaves_the_candidate_alone() {
        let dir = temp();
        let document = adopt(
            &read([], []),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"nowhere\"\n"),
            &[],
            &dir,
        );
        assert!(document.is_none(), "an undecidable candidate was adopted");
    }

    #[test]
    fn every_line_names_what_asked_for_the_change() {
        let name = ItemAddress::try_from("p10k".to_owned()).expect("valid address");
        assert_eq!(
            line(ItemKind::Action, Change::Disable, &name, true, DECLARED),
            "default-disabled: disabled action `p10k`"
        );
        assert_eq!(
            line(
                ItemKind::Action,
                Change::Enable,
                &name,
                false,
                "--enable-action"
            ),
            "--enable-action: action `p10k` was already enabled"
        );
        assert_eq!(heading(ItemKind::Group, &name), "candidate group `p10k`");
    }
}
