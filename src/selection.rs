//! Which of a manifest's actions a run carries out.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use crate::condition::{Bindings, Exclusion};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::item::{ItemAddress, ItemId};
use crate::manifest::action::Action;
use crate::output::Reporter;

/// The environment variable unioned with `--skip-action`.
const SKIP_ACTIONS: &str = "BATFILES_SKIP_ACTIONS";

/// The environment variable unioned with `--skip-group`.
const SKIP_GROUPS: &str = "BATFILES_SKIP_GROUPS";

/// What the warning for an undecidable condition says the run did about it.
/// A gate is closed either way, but without the clause an `unless` reads as
/// though the record went in: a false one is what installs it.
const NOT_INSTALLED: &str = "it is not installed";

/// Which of the manifest's records a command asked for.
#[derive(Debug)]
pub(crate) enum Target<'a> {
    /// Every record, which is `sync`.
    Everything,
    /// The one record answering to this address.
    Action(&'a ItemAddress),
    /// Every record naming this group.
    Group(&'a ItemAddress),
}

impl Target<'_> {
    /// Whether this record is one of the ones asked for.
    fn wants(&self, action: &Action) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(id) => action.id().is_some_and(|declared| id.names(declared)),
            Self::Group(group) => action.group().is_some_and(|declared| group.names(declared)),
        }
    }

    /// What it means for this target to have matched no record at all, which
    /// only the arm that asked can say.
    fn unresolved(&self, manifest: PathBuf) -> Option<Error> {
        match self {
            Self::Everything => None,
            Self::Action(id) => Some(Error::UnknownAction {
                path: manifest,
                id: (*id).clone(),
            }),
            Self::Group(group) => Some(Error::UnknownGroup {
                path: manifest,
                group: (*group).clone(),
            }),
        }
    }

    /// Whether an exclusion naming an action's own ID still applies. Nothing is
    /// finer-grained than the one action `apply-action` asked for.
    fn honors_actions(&self) -> bool {
        !matches!(self, Self::Action(_))
    }

    /// Whether an exclusion naming an action's group still applies. Only a run
    /// that asked for the whole manifest asked for something coarser than a
    /// group.
    fn honors_groups(&self) -> bool {
        matches!(self, Self::Everything)
    }
}

/// Why an action is not being run.
#[derive(Debug)]
pub(crate) enum SkipReason<'a> {
    /// A `disabled.toml` entry. The noun says which of its two lists.
    Disabled {
        noun: &'static str,
        name: &'a ItemId,
    },
    /// A run-only skip, and the option or variable that supplied it — which
    /// says by itself whether an action or a group was named.
    Run {
        name: &'a ItemId,
        origin: &'static str,
    },
}

impl fmt::Display for SkipReason<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled { noun, name } => write!(f, "{noun} `{name}` is disabled"),
            Self::Run { name, origin } => write!(f, "`{name}` from {origin}"),
        }
    }
}

/// One run-only skip list: the names to pass over, each remembering the source
/// that supplied it.
#[derive(Debug, Default)]
struct SkipList {
    names: BTreeMap<ItemAddress, &'static str>,
}

impl SkipList {
    /// Validate and add every name one source supplied.
    fn extend(&mut self, values: &[String], origin: &'static str, reporter: &Reporter) {
        for value in values {
            match ItemAddress::try_from(value.clone()) {
                // First writer wins, so the caller adds the option's names ahead
                // of the environment's.
                Ok(name) => {
                    self.names.entry(name).or_insert(origin);
                }
                Err(error) => reporter.warn(&format!("{origin}: {error}")),
            }
        }
    }

    /// The source that named `candidate`, if any did.
    fn origin(&self, candidate: &ItemId) -> Option<&'static str> {
        self.names
            .iter()
            .find_map(|(name, origin)| name.names(candidate).then_some(*origin))
    }

    /// Warn once per name that nothing in the manifest answers to.
    fn warn_unmatched(&self, present: &BTreeSet<&ItemId>, noun: &str, reporter: &Reporter) {
        for (name, origin) in &self.names {
            if !present.iter().any(|id| name.names(id)) {
                reporter.warn(&format!("{origin} `{name}` matched no {noun}"));
            }
        }
    }
}

/// One namespace's run-only skips: the option's names unioned with the
/// variable's, or nothing at all where this run waives them.
fn run_only(
    consulted: bool,
    values: &[String],
    option: &'static str,
    variable: &'static str,
    env: &Environment,
    reporter: &Reporter,
) -> SkipList {
    let mut skips = SkipList::default();
    if consulted {
        skips.extend(values, option, reporter);
        skips.extend(&env.list(variable), variable, reporter);
    }
    skips
}

/// What one run carries out: the records it asked for, less the ones either
/// exclusion source names.
#[derive(Debug)]
pub(crate) struct Selection<'a> {
    target: Target<'a>,
    actions: SkipList,
    groups: SkipList,
    disabled: Disabled,
}

impl<'a> Selection<'a> {
    /// The filter one run applies: everything the target asked for, less what
    /// either source names and the target does not waive.
    pub fn new(
        target: Target<'a>,
        skip_actions: &[String],
        skip_groups: &[String],
        env: &Environment,
        disabled: Disabled,
        reporter: &Reporter,
    ) -> Self {
        Self {
            actions: run_only(
                target.honors_actions(),
                skip_actions,
                "--skip-action",
                SKIP_ACTIONS,
                env,
                reporter,
            ),
            groups: run_only(
                target.honors_groups(),
                skip_groups,
                "--skip-group",
                SKIP_GROUPS,
                env,
                reporter,
            ),
            target,
            disabled,
        }
    }

    /// Whether this record is one of the ones the command asked for.
    pub fn wants(&self, action: &Action) -> bool {
        self.target.wants(action)
    }

    /// The failure for a run whose target matched no record — or `None` where
    /// matching none of them is an ordinary outcome.
    pub fn unresolved(&self, manifest: PathBuf) -> Option<Error> {
        self.target.unresolved(manifest)
    }

    /// Warn about every run-only skip that names nothing the manifest declares.
    pub fn warn_unmatched(&self, actions: &[Action], reporter: &Reporter) {
        let ids: BTreeSet<&ItemId> = actions.iter().filter_map(Action::id).collect();
        let groups: BTreeSet<&ItemId> = actions.iter().filter_map(Action::group).collect();
        self.actions.warn_unmatched(&ids, "action", reporter);
        self.groups.warn_unmatched(&groups, "group", reporter);
    }

    /// Why this run is passing the record over, or `None` where it carries it
    /// out.
    ///
    /// A condition batfiles cannot decide closes the gate like any other
    /// exclusion, and says so at every verbosity: the record is the one thing a
    /// reader can act on, and it is named where the run reports it rather than
    /// here.
    ///
    /// The record's own condition is consulted last, so a record some list
    /// already excludes is never evaluated: a condition that cannot be
    /// evaluated then costs only the runs that would otherwise have carried the
    /// record out.
    pub fn exclusion(&self, action: &Action, bindings: &Bindings<'_>) -> Option<Exclusion> {
        if let Some(reason) = self.listed_reason(action) {
            return Some(Exclusion::Expected(reason.to_string()));
        }
        // Waived exactly where the action's own name is. `apply-action` names
        // one record and nothing is finer-grained than that, so naming it
        // reaches it whatever this machine makes of its condition.
        let gate = action.gate().filter(|_| self.target.honors_actions())?;
        gate.verdict(bindings, Some(NOT_INSTALLED)).exclusion()
    }

    /// The first exclusion either list names, or `None` where neither does.
    /// Persistent disables precede run-only skips; action exclusions precede
    /// group exclusions within each source. The target determines which
    /// exclusions are honored.
    fn listed_reason<'b>(&self, action: &'b Action) -> Option<SkipReason<'b>> {
        let id = action.id().filter(|_| self.target.honors_actions());
        let group = action.group().filter(|_| self.target.honors_groups());

        let listed = |list: &BTreeSet<ItemAddress>, id: &ItemId| {
            list.iter().any(|address| address.names(id))
        };

        if let Some(name) = id.filter(|id| listed(&self.disabled.actions, id)) {
            return Some(SkipReason::Disabled {
                noun: "action",
                name,
            });
        }
        if let Some(name) = group.filter(|group| listed(&self.disabled.groups, group)) {
            return Some(SkipReason::Disabled {
                noun: "group",
                name,
            });
        }
        if let Some((name, origin)) = id.and_then(|id| Some((id, self.actions.origin(id)?))) {
            return Some(SkipReason::Run { name, origin });
        }
        group.and_then(|group| {
            Some(SkipReason::Run {
                name: group,
                origin: self.groups.origin(group)?,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::condition::HostNamespaces;
    use crate::output::Verbosity;
    use crate::var::VarName;
    use crate::var_set::VarSet;

    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(Verbosity::Quiet);
        reporter
    }

    fn address(address: &str) -> ItemAddress {
        ItemAddress::try_from(address.to_owned()).expect("valid address")
    }

    /// One `create-dir` naming both an action and a group, which is the record
    /// every rule below is decided against.
    fn action(id: &str, group: &str) -> Action {
        toml::from_str(&format!(
            "type = \"create-dir\"\nid = \"{id}\"\ngroup = \"{group}\"\ndest = \"~/x\"\n"
        ))
        .expect("the record should parse")
    }

    fn disabled(actions: &[&str], groups: &[&str]) -> Disabled {
        Disabled {
            actions: actions.iter().map(|name| address(name)).collect(),
            groups: groups.iter().map(|name| address(name)).collect(),
        }
    }

    /// A `sync`'s filter: it asks for everything, so it waives nothing.
    fn selection(
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: Disabled,
    ) -> Selection<'static> {
        filter(Target::Everything, skip_actions, skip_groups, env, disabled)
    }

    fn filter<'a>(
        target: Target<'a>,
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: Disabled,
    ) -> Selection<'a> {
        Selection::new(
            target,
            &names_of(skip_actions),
            &names_of(skip_groups),
            &Environment::from_pairs(env.iter().copied()),
            disabled,
            &quiet(),
        )
    }

    fn names_of(values: &[&str]) -> Vec<String> {
        values.iter().copied().map(str::to_owned).collect()
    }

    /// What a selection says about one record, rendered the way a run reports
    /// it. The bindings are empty, since most of the rules below decide a
    /// record that declares no condition at all.
    fn reason(selection: &Selection, action: &Action) -> Option<String> {
        decided(selection, action, &[]).map(|exclusion| exclusion.reason().to_owned())
    }

    /// The same, against a variable set, and keeping which kind of exclusion
    /// it is.
    fn decided(selection: &Selection, action: &Action, vars: &[(&str, &str)]) -> Option<Exclusion> {
        let variables = Rc::new(VarSet::stack(
            vars.iter()
                .map(|(name, value)| {
                    (
                        VarName::try_from((*name).to_owned()).expect("valid name"),
                        (*value).to_owned(),
                    )
                })
                .collect(),
            BTreeMap::new(),
            BTreeMap::new(),
            &[],
        ));
        let empty = Environment::from_pairs(std::iter::empty::<(&str, &str)>());
        let host = HostNamespaces::capture(&empty);
        let bindings = Bindings::new(&variables, &host);
        selection.exclusion(action, &bindings)
    }

    /// The reason a record was passed over as asked, for a case that expects
    /// one rather than a condition batfiles could not decide.
    fn expected_reason(
        selection: &Selection,
        action: &Action,
        vars: &[(&str, &str)],
    ) -> Option<String> {
        match decided(selection, action, vars) {
            Some(Exclusion::Expected(reason)) => Some(reason),
            Some(other) => panic!("expected an ordinary exclusion, got {other:?}"),
            None => None,
        }
    }

    /// A `create-dir` carrying one condition, in the spelling named.
    fn conditioned(spelling: &str, condition: &str) -> Action {
        toml::from_str(&format!(
            "type = \"create-dir\"\nid = \"zshrc\"\ngroup = \"shell\"\n\
             dest = \"~/x\"\n{spelling} = \"{condition}\"\n"
        ))
        .expect("the record should parse")
    }

    #[test]
    fn an_action_named_by_nothing_runs() {
        let selection = selection(&["other"], &["other"], &[], Disabled::default());
        assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
    }

    #[test]
    fn either_namespace_can_name_an_action() {
        let by_action = selection(&["zshrc"], &[], &[], Disabled::default());
        assert_eq!(
            reason(&by_action, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );
        let by_group = selection(&[], &["shell"], &[], Disabled::default());
        assert_eq!(
            reason(&by_group, &action("zshrc", "shell")).as_deref(),
            Some("`shell` from --skip-group")
        );
    }

    #[test]
    fn the_two_run_only_sources_union() {
        let selection = selection(
            &[],
            &[],
            &[(SKIP_ACTIONS, "zshrc"), (SKIP_GROUPS, "gui")],
            Disabled::default(),
        );
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from BATFILES_SKIP_ACTIONS")
        );
        assert_eq!(
            reason(&selection, &action("gtk", "gui")).as_deref(),
            Some("`gui` from BATFILES_SKIP_GROUPS")
        );
    }

    #[test]
    fn the_option_is_credited_when_both_sources_name_one_thing() {
        // Command-line arguments outrank the environment, and the line points at
        // the input the reader is likelier to be able to change.
        let selection = selection(
            &["zshrc"],
            &[],
            &[(SKIP_ACTIONS, "zshrc")],
            Disabled::default(),
        );
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );
    }

    #[test]
    fn a_disable_is_reported_ahead_of_a_skip() {
        let selection = selection(
            &["zshrc"],
            &["shell"],
            &[],
            disabled(&["zshrc"], &["shell"]),
        );
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("action `zshrc` is disabled")
        );
    }

    #[test]
    fn a_disabled_group_is_reported_where_the_action_itself_is_not_disabled() {
        let selection = selection(&["zshrc"], &[], &[], disabled(&[], &["shell"]));
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("group `shell` is disabled")
        );
    }

    #[test]
    fn an_actions_own_name_is_reported_ahead_of_its_groups() {
        let selection = selection(&["zshrc"], &["shell"], &[], Disabled::default());
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );
    }

    // The waiver: an exclusion no finer-grained than what the command asked for
    // does not apply. Each case names the record every list above names.

    #[test]
    fn asking_for_a_group_waives_the_group_level_exclusions_only() {
        let shell = address("shell");
        let by_skip = filter(
            Target::Group(&shell),
            &["zshrc"],
            &["shell"],
            &[(SKIP_GROUPS, "shell")],
            disabled(&[], &["shell"]),
        );
        assert_eq!(reason(&by_skip, &action("other", "shell")), None);
        assert_eq!(
            reason(&by_skip, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );

        let by_disable = filter(
            Target::Group(&shell),
            &[],
            &[],
            &[],
            disabled(&["zshrc"], &["shell"]),
        );
        assert_eq!(
            reason(&by_disable, &action("zshrc", "shell")).as_deref(),
            Some("action `zshrc` is disabled")
        );
    }

    #[test]
    fn asking_for_an_action_waives_every_exclusion() {
        // `apply-action`. Nothing is finer-grained than the one action asked
        // for, so all four lists are waived.
        let zshrc = address("zshrc");
        let selection = filter(
            Target::Action(&zshrc),
            &["zshrc"],
            &["shell"],
            &[(SKIP_ACTIONS, "zshrc"), (SKIP_GROUPS, "shell")],
            disabled(&["zshrc"], &["shell"]),
        );
        assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
    }

    // The record's own condition, which is the one exclusion the manifest
    // rather than the machine declares.

    #[test]
    fn a_condition_decides_the_record_it_is_written_on() {
        let selection = selection(&[], &[], &[], Disabled::default());
        let vars = &[("work", "false")];

        assert_eq!(
            expected_reason(&selection, &conditioned("when", "work"), vars).as_deref(),
            Some("when \"work\" is false")
        );
        assert_eq!(
            expected_reason(&selection, &conditioned("unless", "work"), vars),
            None
        );

        // And the other way around, so neither spelling is the negation of the
        // other by accident.
        let vars = &[("work", "true")];
        assert_eq!(
            expected_reason(&selection, &conditioned("when", "work"), vars),
            None
        );
        assert_eq!(
            expected_reason(&selection, &conditioned("unless", "work"), vars).as_deref(),
            Some("unless \"work\" is true")
        );
    }

    #[test]
    fn a_condition_is_consulted_only_where_nothing_else_excludes_the_record() {
        // The reason a run with one bad condition on a disabled action still
        // works: the arm is last, so it is never reached for a record some list
        // already names. `nowhere` is declared by no layer.
        let skipping = selection(&["zshrc"], &[], &[], Disabled::default());
        assert_eq!(
            expected_reason(&skipping, &conditioned("when", "nowhere"), &[]).as_deref(),
            Some("`zshrc` from --skip-action")
        );

        // Reached, and failing, once nothing else has an opinion.
        let plain = selection(&[], &[], &[], Disabled::default());
        assert!(matches!(
            decided(&plain, &conditioned("when", "nowhere"), &[]),
            Some(Exclusion::EvaluationFailed(_))
        ));
    }

    #[test]
    fn a_condition_that_cannot_be_decided_closes_the_gate_in_either_spelling() {
        // The asymmetry the spellings hide: a false `unless` opens a gate, so
        // reading a failure as false would install the very record the line was
        // written to suppress. Both close, and both say which field decided it.
        let selection = selection(&[], &[], &[], Disabled::default());

        for spelling in ["when", "unless"] {
            let Some(Exclusion::EvaluationFailed(why)) =
                decided(&selection, &conditioned(spelling, "nowhere"), &[])
            else {
                panic!("`{spelling}` should close on a condition nothing can decide");
            };
            assert!(why.starts_with(&format!("{spelling} \"nowhere\"")), "{why}");
            assert!(why.contains("cannot be evaluated"), "{why}");
            assert!(why.contains(NOT_INSTALLED), "{why}");
            // The fix, which is the half of the line a reader acts on.
            assert!(why.contains("`nowhere` is not declared"), "{why}");
            assert!(why.contains("batfiles vars set nowhere"), "{why}");
        }
    }

    #[test]
    fn asking_for_one_action_waives_its_condition_too() {
        // The waiver is the action tier's, and a condition sits in it: naming
        // one record is the finest thing a command can ask for, so it reaches
        // the record whatever this machine makes of its condition. A run that
        // cannot decide the condition is not stopped by it either.
        let zshrc = address("zshrc");
        let by_name = filter(Target::Action(&zshrc), &[], &[], &[], Disabled::default());
        assert_eq!(
            decided(&by_name, &conditioned("when", "work"), &[("work", "false")]),
            None
        );
        assert_eq!(
            decided(&by_name, &conditioned("when", "nowhere"), &[]),
            None
        );

        // A group is coarser than one record, so its members keep theirs.
        let shell = address("shell");
        let by_group = filter(Target::Group(&shell), &[], &[], &[], Disabled::default());
        assert_eq!(
            expected_reason(
                &by_group,
                &conditioned("when", "work"),
                &[("work", "false")]
            )
            .as_deref(),
            Some("when \"work\" is false")
        );
    }

    #[test]
    fn a_record_with_no_condition_is_decided_by_the_lists_alone() {
        let selection = selection(&[], &[], &[], Disabled::default());
        assert_eq!(decided(&selection, &action("zshrc", "shell"), &[]), None);
    }

    #[test]
    fn a_condition_is_repeated_back_as_written_and_cannot_forge_a_line() {
        // Repository text reaching a report, so it goes through the escaping
        // every other repeated value does.
        let selection = selection(&[], &[], &[], Disabled::default());
        let record = conditioned("when", "work && vars['a\\nb']");
        let why = expected_reason(&selection, &record, &[("work", "true")])
            .expect("the condition should be false");
        assert!(!why.contains('\n'), "{why}");
        assert!(why.contains("\\n"), "{why}");
    }

    #[test]
    fn a_name_that_is_not_an_address_is_dropped_rather_than_kept() {
        // It could never match, so it is warned about at the point it is read
        // and takes no part in the filtering.
        let mut skips = SkipList::default();
        skips.extend(&names_of(&["a..b", "zshrc"]), "--skip-action", &quiet());
        assert_eq!(
            skips.origin(&ItemId::try_from("zshrc".to_owned()).expect("valid ID")),
            Some("--skip-action")
        );
        assert_eq!(skips.names.len(), 1);
    }

    #[test]
    fn a_qualified_skip_is_kept_and_names_no_leaf_record() {
        let by_skip = selection(&["core.zshrc"], &["core.shell"], &[], Disabled::default());
        assert_eq!(reason(&by_skip, &action("zshrc", "shell")), None);

        // Same shape in the persistent lists, which are silent about it.
        let by_disable = selection(&[], &[], &[], disabled(&["core.zshrc"], &["core.shell"]));
        assert_eq!(reason(&by_disable, &action("zshrc", "shell")), None);
    }
}
