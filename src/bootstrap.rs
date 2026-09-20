//! Bootstrap adoption: what a machine starts with switched off, decided once by
//! `clone` and written to [`disabled.toml`](crate::disabled).
//!
//! Three sources say something about it -- the leaf's `[default-disabled]`
//! candidates, the `BATFILES_*` bootstrap lists, and the command's own
//! enable/disable options -- and the
//! [precedence](../../docs/environment.md#bootstrap-enable-and-disable-lists)
//! between them is a sequence: each source is applied over the one before it, so
//! the last to name an action or group is the one that decides it.
//!
//! Only a machine with no `disabled.toml` is offered the candidates. The
//! document existing is the machine having an opinion of its own, and
//! [the section](../../docs/repoformat.md#default-disabled-bootstrap-entries)
//! cannot switch an action off again on a machine that has already enabled it.
//! The explicit decisions apply either way: they were written for this
//! invocation rather than by the repository.

use crate::cli::BootstrapOptions;
use crate::condition::{Bindings, Gate};
use crate::disabled::{Change, Disabled, DisabledList, apply, outcome};
use crate::env::Environment;
use crate::error::Error;
use crate::item::ItemAddress;
use crate::location::StateRoots;
use crate::manifest::default_disabled::DefaultDisabled;
use crate::output::Reporter;
use crate::paths;

/// What the warning for an undecidable candidate condition says adoption did
/// about it. A gate that cannot be decided closes, here as everywhere, so the
/// candidate is not offered -- which is what leaves the action or group enabled.
const NOT_DISABLED: &str = "it is left enabled";

/// The origin reported for a candidate the repository declared.
const DECLARED: &str = "default-disabled";

/// The explicit decisions one `clone` was given, in the order they apply.
///
/// Read before the repository is cloned, so a malformed address fails the
/// command with nothing downloaded.
#[derive(Debug)]
pub(crate) struct Bootstrap {
    decisions: Vec<Decision>,
}

/// One explicit decision: what it names, which of the document's two lists it
/// belongs to, which way it moves the name, and what said so.
#[derive(Debug)]
struct Decision {
    list: DisabledList,
    change: Change,
    origin: &'static str,
    name: ItemAddress,
}

impl Bootstrap {
    /// Read the four variables and the four options into one ordered list.
    ///
    /// A malformed option value fails the command, as it does for
    /// `disable-action`; a malformed variable value warns and is dropped, as a
    /// run-only skip does. The variables are written by generated installers,
    /// where one unusable name is not worth refusing a machine's whole setup
    /// over.
    pub fn read(
        options: &BootstrapOptions,
        env: &Environment,
        reporter: &Reporter,
    ) -> Result<Self, Error> {
        use Change::{Disable, Enable};
        use DisabledList::{Actions, Groups};

        let mut decisions = Vec::new();
        // The two tables below are the precedence: the environment first and
        // the command line over it, and within each source disable before
        // enable, so enable wins where both name the same thing. Actions and
        // groups are separate sets, so their order within one level settles
        // nothing and is only the order two lines are printed in.
        for (list, change, variable) in [
            (Actions, Disable, "BATFILES_DISABLE_ACTIONS"),
            (Groups, Disable, "BATFILES_DISABLE_GROUPS"),
            (Actions, Enable, "BATFILES_ENABLE_ACTIONS"),
            (Groups, Enable, "BATFILES_ENABLE_GROUPS"),
        ] {
            for value in env.list(variable) {
                match ItemAddress::try_from(value) {
                    Ok(name) => decisions.push(Decision {
                        list,
                        change,
                        origin: variable,
                        name,
                    }),
                    Err(error) => reporter.warn(&format!("{variable}: {error}")),
                }
            }
        }
        for (list, change, option, values) in [
            (
                Actions,
                Disable,
                "--disable-action",
                &options.disable_actions,
            ),
            (Groups, Disable, "--disable-group", &options.disable_groups),
            (Actions, Enable, "--enable-action", &options.enable_actions),
            (Groups, Enable, "--enable-group", &options.enable_groups),
        ] {
            for value in values {
                decisions.push(Decision {
                    list,
                    change,
                    origin: option,
                    name: ItemAddress::try_from(value.clone())?,
                });
            }
        }
        Ok(Self { decisions })
    }

    /// Decide this machine's starting point and record it.
    ///
    /// Reports one line per decision, and saves only where something moved: a
    /// bootstrap that decides nothing leaves no document behind, so the next one
    /// is offered the candidates in its turn.
    pub fn adopt(
        &self,
        candidates: &DefaultDisabled,
        bindings: &Bindings<'_>,
        roots: &StateRoots,
        reporter: &Reporter,
    ) -> Result<(), Error> {
        let path = roots.disabled();
        // Presence rather than content: a document listing nothing is still a
        // machine that has been set up, and an empty answer is an answer.
        let offered = !paths::occupied(&path)?;
        let mut disabled = Disabled::load(&path)?;

        let mut lines = Vec::new();
        let mut changed = false;

        if offered {
            for (list, name, gate) in entries(candidates) {
                if let Some(exclusion) =
                    gate.and_then(|gate| gate.exclusion(bindings, Some(NOT_DISABLED)))
                {
                    exclusion.report_heading(reporter, &heading(list, name));
                    continue;
                }
                let moved = apply(list.set_in(&mut disabled), Change::Disable, name);
                changed |= moved;
                lines.push(line(list, Change::Disable, name, moved, DECLARED));
            }
        }

        for decision in &self.decisions {
            let moved = apply(
                decision.list.set_in(&mut disabled),
                decision.change,
                &decision.name,
            );
            changed |= moved;
            lines.push(line(
                decision.list,
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

/// Every candidate the leaf declared, in the order they are offered: the
/// actions and then the groups, each with the gate that says whether this
/// machine is offered it at all.
fn entries(
    candidates: &DefaultDisabled,
) -> impl Iterator<Item = (DisabledList, &ItemAddress, Option<Gate<'_>>)> {
    let actions = candidates
        .actions
        .iter()
        .map(|entry| (DisabledList::Actions, &entry.id, entry.gate()));
    let groups = candidates
        .groups
        .iter()
        .map(|entry| (DisabledList::Groups, &entry.group, entry.gate()));
    actions.chain(groups)
}

/// How a report names one candidate, which is the record rather than the change
/// it would have made.
fn heading(list: DisabledList, name: &ItemAddress) -> String {
    format!("candidate {} `{name}`", list.noun())
}

/// One decision's line: what asked for the change, and what the change did.
///
/// The origin leads, as it does wherever else an input is named for what it
/// did, which also keeps it clear of the parenthetical an enable already
/// carries.
fn line(
    list: DisabledList,
    change: Change,
    name: &ItemAddress,
    moved: bool,
    origin: &'static str,
) -> String {
    format!("{origin}: {}", outcome(list, change, name, moved))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::condition::HostNamespaces;
    use crate::output::Verbosity;
    use crate::var_set::VarSet;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    /// Warnings print at every verbosity, so what these tests read is the
    /// outcome rather than the output. The wording reaches a user through a CLI
    /// test.
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
    ) -> Bootstrap {
        Bootstrap::read(&options(pairs), &env(variables), &quiet()).expect("valid addresses")
    }

    /// Each decision as `origin name`, which is the whole of what one is once
    /// the list and the change are read off the origin.
    fn decisions(bootstrap: &Bootstrap) -> Vec<String> {
        bootstrap
            .decisions
            .iter()
            .map(|decision| format!("{} {}", decision.origin, decision.name))
            .collect()
    }

    /// The candidates a manifest declares, written the way a manifest writes
    /// them rather than as the section on its own.
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
        bootstrap: &Bootstrap,
        candidates: &DefaultDisabled,
        vars: &[(&str, &str)],
        dir: &tempfile::TempDir,
    ) -> Option<Disabled> {
        let values: BTreeMap<_, _> = vars
            .iter()
            .map(|(name, value)| {
                (
                    crate::var::VarName::try_from((*name).to_owned()).expect("valid name"),
                    (*value).to_owned(),
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
            .disabled()
            .exists()
            .then(|| Disabled::load(&roots.disabled()).expect("the written document"))
    }

    fn temp() -> tempfile::TempDir {
        tempfile::tempdir().expect("temp dir")
    }

    fn names(addresses: &std::collections::BTreeSet<ItemAddress>) -> Vec<String> {
        addresses.iter().map(ToString::to_string).collect()
    }

    // Reading the two sources.

    #[test]
    fn the_decisions_are_ordered_by_the_precedence_they_apply_in() {
        // The whole of the precedence in one list: the environment before the
        // command line, and disable before enable within each.
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
        // A generated installer's one bad name is not worth refusing the whole
        // machine over, so this warns and carries on.
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
        // The other half of the same rule: an option is what this invocation
        // asked for, so a name it cannot use is a mistake to report rather than
        // to work around.
        let error = Bootstrap::read(
            &options([("--disable-action", "core..p10k")]),
            &env([]),
            &quiet(),
        )
        .expect_err("an empty segment is not an address");
        assert!(error.to_string().contains("`core..p10k`"), "{error}");
    }

    // Adopting.

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
        Disabled::default()
            .save(&roots.disabled())
            .expect("an existing document");

        let document = adopt(
            &read([("--disable-group", "gui")], []),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\n"),
            &[],
            &dir,
        )
        .expect("a document");

        // The candidate is passed over and the explicit decision is not: one was
        // the repository's standing opinion and the other was written for this
        // invocation.
        assert!(document.actions.is_empty());
        assert_eq!(names(&document.groups), ["gui"]);
    }

    #[test]
    fn deciding_nothing_leaves_no_document_behind() {
        // Which is what keeps the rule above from latching a machine that was
        // never actually set up.
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
        // Nothing was left disabled, and the two decisions cancelled out, so
        // there is nothing to write either.
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

    // What a candidate's condition decides.

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
        // A gate that cannot be decided closes, here as everywhere else, so the
        // candidate is not offered and the action it names stays enabled.
        let dir = temp();
        let document = adopt(
            &read([], []),
            &candidates("[[default-disabled.actions]]\nid = \"p10k\"\nwhen = \"nowhere\"\n"),
            &[],
            &dir,
        );
        assert!(document.is_none(), "an undecidable candidate was adopted");
    }

    // What a decision says it did.

    #[test]
    fn every_line_names_what_asked_for_the_change() {
        let name = ItemAddress::try_from("p10k".to_owned()).expect("valid address");
        assert_eq!(
            line(
                DisabledList::Actions,
                Change::Disable,
                &name,
                true,
                DECLARED
            ),
            "default-disabled: disabled action `p10k`"
        );
        assert_eq!(
            line(
                DisabledList::Actions,
                Change::Enable,
                &name,
                false,
                "--enable-action"
            ),
            "--enable-action: action `p10k` was already enabled"
        );
        assert_eq!(
            heading(DisabledList::Groups, &name),
            "candidate group `p10k`"
        );
    }
}
