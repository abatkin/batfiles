//! Which of a manifest's actions a run carries out.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use crate::condition::{Bindings, Exclusion};
use crate::disabled::Disabled;
use crate::env::Environment;
use crate::error::Error;
use crate::execute::record::{RunList, RunRecord};
use crate::item::{ItemAddress, ItemId};
use crate::manifest::action::Action;
use crate::output::Reporter;

/// The environment variable unioned with `--skip-action`.
const SKIP_ACTIONS: &str = "BATFILES_SKIP_ACTIONS";

/// The environment variable unioned with `--skip-group`.
const SKIP_GROUPS: &str = "BATFILES_SKIP_GROUPS";

/// The outcome clause of an undecidable condition's warning. Without it, a
/// failed `unless` could read as installing the record.
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
    fn wants(&self, record: &RunRecord) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(id) => record.address.as_ref() == Some(*id),
            Self::Group(group) => record.group_address.as_ref() == Some(*group),
        }
    }

    /// Whether this target could name something inside the inclusion with `id`,
    /// so its manifest must be read. An inclusion without an `id` is opened only
    /// for [`Everything`](Self::Everything).
    fn reaches_into(&self, id: Option<&ItemId>) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(address) | Self::Group(address) => {
                id.is_some_and(|id| address.qualified_by(id))
            }
        }
    }

    /// The error for a target that matched no record; `None` for
    /// [`Everything`](Self::Everything).
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

    /// Whether exclusions naming a record's own address apply. `apply-action`
    /// waives them, and with them the record's own condition.
    fn honors_action_exclusions(&self) -> bool {
        !matches!(self, Self::Action(_))
    }

    /// Whether exclusions naming a record's group apply; only
    /// [`Everything`](Self::Everything) honors them.
    fn honors_group_exclusions(&self) -> bool {
        matches!(self, Self::Everything)
    }
}

/// Why an action is not being run.
#[derive(Debug)]
pub(crate) enum SkipReason<'a> {
    /// A `disabled.toml` entry; `noun` names its list.
    Disabled {
        noun: &'static str,
        name: &'a ItemAddress,
    },
    /// A run-only skip, and the option or variable that supplied it.
    Run {
        name: &'a ItemAddress,
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
    fn origin(&self, candidate: &ItemAddress) -> Option<&'static str> {
        self.names.get(candidate).copied()
    }

    /// Warn once per name not in `present`. Names qualified by an
    /// `unread_inclusions` ID are skipped silently: nothing read could answer
    /// them.
    fn warn_unmatched(
        &self,
        present: &[&ItemAddress],
        unread_inclusions: &[&ItemId],
        noun: &str,
        reporter: &Reporter,
    ) {
        for (name, origin) in &self.names {
            if present.contains(&name) || unread_inclusions.iter().any(|id| name.qualified_by(id)) {
                continue;
            }
            reporter.warn(&format!("{origin} `{name}` matched no {noun}"));
        }
    }
}

/// One namespace's run-only skips: the option's names unioned with the
/// variable's, or empty and unread where the target waives them.
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

/// What one run executes: the records its target asks for, less those that
/// `disabled.toml` or a run-only skip excludes.
#[derive(Debug)]
pub(crate) struct Selection<'a> {
    target: Target<'a>,
    actions: SkipList,
    groups: SkipList,
    disabled: Disabled,
}

impl<'a> Selection<'a> {
    /// Build the run's selection, reading only the skips the target honors.
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
                target.honors_action_exclusions(),
                skip_actions,
                "--skip-action",
                SKIP_ACTIONS,
                env,
                reporter,
            ),
            groups: run_only(
                target.honors_group_exclusions(),
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

    /// Everything, less what `disabled.toml` excludes: no run-only skip is
    /// read. What `vars refresh` decides reachability with.
    pub fn persistent(disabled: Disabled) -> Self {
        Self {
            target: Target::Everything,
            actions: SkipList::default(),
            groups: SkipList::default(),
            disabled,
        }
    }

    /// Whether the target asks for `record`.
    pub fn wants(&self, record: &RunRecord) -> bool {
        self.target.wants(record)
    }

    /// See [`Target::reaches_into`].
    pub fn reaches_into(&self, id: Option<&ItemId>) -> bool {
        self.target.reaches_into(id)
    }

    /// See [`Target::unresolved`].
    pub fn unresolved(&self, manifest: PathBuf) -> Option<Error> {
        self.target.unresolved(manifest)
    }

    /// The error for a target naming a record the command cannot run, or
    /// `None`. Only `apply-action` naming an `include-remote` is refused: the
    /// inclusion's `id` qualifies its records' addresses, so naming the
    /// inclusion itself names the wrong thing.
    pub fn refusal(&self, action: &Action) -> Option<Error> {
        match (&self.target, action) {
            (Target::Action(id), Action::IncludeRemote(_)) => {
                Some(Error::InclusionNotApplyable { id: (*id).clone() })
            }
            _ => None,
        }
    }

    /// Warn about every run-only skip that names nothing in the assembled list,
    /// which includes contributed records. Skips into an unread inclusion are
    /// not warned about.
    pub fn warn_unmatched(&self, run_list: &RunList, reporter: &Reporter) {
        let names = |of: fn(&RunRecord) -> &Option<ItemAddress>| -> Vec<&ItemAddress> {
            run_list
                .records()
                .filter_map(|it| of(it).as_ref())
                .collect()
        };
        let unread_inclusions: Vec<&ItemId> = run_list.unread_inclusions().collect();
        self.actions.warn_unmatched(
            &names(|it| &it.address),
            &unread_inclusions,
            "action",
            reporter,
        );
        self.groups.warn_unmatched(
            &names(|it| &it.group_address),
            &unread_inclusions,
            "group",
            reporter,
        );
    }

    /// Why this run passes the record over, or `None` if it runs.
    ///
    /// Listed exclusions come first. The record's own condition is evaluated
    /// only when nothing else excludes it, so an undecidable condition affects
    /// only runs that would execute the record; there it excludes the record
    /// like any other exclusion, reported at every verbosity.
    pub fn exclusion(&self, record: &RunRecord, bindings: &Bindings<'_>) -> Option<Exclusion> {
        if let Some(reason) = self.listed_reason(record) {
            return Some(Exclusion::Expected(reason.to_string()));
        }
        // Waived exactly where an exclusion naming the action is.
        let gate = record
            .action
            .gate()
            .filter(|_| self.target.honors_action_exclusions())?;
        gate.exclusion(bindings, Some(NOT_INSTALLED))
    }

    /// The first exclusion either list names, or `None` where neither does.
    /// Persistent disables precede run-only skips; action exclusions precede
    /// group exclusions within each source. The target determines which
    /// exclusions are honored.
    fn listed_reason<'b>(&self, record: &'b RunRecord) -> Option<SkipReason<'b>> {
        let address = record
            .address
            .as_ref()
            .filter(|_| self.target.honors_action_exclusions());
        let group_address = record
            .group_address
            .as_ref()
            .filter(|_| self.target.honors_group_exclusions());

        let listed = |list: &BTreeSet<ItemAddress>, item: &ItemAddress| list.contains(item);

        if let Some(name) = address.filter(|it| listed(&self.disabled.actions, it)) {
            return Some(SkipReason::Disabled {
                noun: "action",
                name,
            });
        }
        if let Some(name) = group_address.filter(|it| listed(&self.disabled.groups, it)) {
            return Some(SkipReason::Disabled {
                noun: "group",
                name,
            });
        }
        if let Some((name, origin)) = address.and_then(|it| Some((it, self.actions.origin(it)?))) {
            return Some(SkipReason::Run { name, origin });
        }
        group_address.and_then(|it| {
            Some(SkipReason::Run {
                name: it,
                origin: self.groups.origin(it)?,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use std::rc::Rc;

    use super::*;
    use crate::condition::HostNamespaces;
    use crate::execute::inclusion::Inclusion;
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

    /// A leaf `create-dir` with both an `id` and a `group`.
    fn action(id: &str, group: &str) -> RunRecord {
        RunRecord::leaf(create_dir(id, group), 1)
    }

    /// The same record, contributed by an inclusion with `id = inclusion`
    /// (`None`: no `id`). Built from a parsed `include-remote` so it matches what
    /// a run produces.
    fn included(id: &str, group: &str, inclusion: Option<&str>) -> RunRecord {
        let named = match inclusion {
            Some(id) => format!("id = \"{id}\"\n"),
            None => String::new(),
        };
        let declaration = toml::from_str(&format!("{named}remote = \"corporate\"\n"))
            .expect("the record should parse");
        let by = Inclusion::at(&declaration, 1);
        RunRecord::contributed(create_dir(id, group), 1, by.contributor())
    }

    fn create_dir(id: &str, group: &str) -> Action {
        toml::from_str(&format!(
            "type = \"create-dir\"\nid = \"{id}\"\ngroup = \"{group}\"\ndest = \"~/x\"\n"
        ))
        .expect("the record should parse")
    }

    fn item(id: &str) -> ItemId {
        ItemId::try_from(id.to_owned()).expect("valid ID")
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

    /// The exclusion reason for `record`, with no variables bound.
    fn reason(selection: &Selection, record: &RunRecord) -> Option<String> {
        decided(selection, record, &[]).map(|exclusion| exclusion.reason().to_owned())
    }

    /// The exclusion for `record` with `vars` bound.
    fn decided(
        selection: &Selection,
        record: &RunRecord,
        vars: &[(&str, &str)],
    ) -> Option<Exclusion> {
        let variables = Rc::new(VarSet::stack(
            vars.iter()
                .map(|(name, value)| {
                    (
                        VarName::try_from((*name).to_owned()).expect("valid name"),
                        crate::var_set::VarValue::Static((*value).to_owned()),
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
        selection.exclusion(record, &bindings)
    }

    /// The reason for an [`Expected`](Exclusion::Expected) exclusion; panics on
    /// an evaluation failure.
    fn expected_reason(
        selection: &Selection,
        record: &RunRecord,
        vars: &[(&str, &str)],
    ) -> Option<String> {
        match decided(selection, record, vars) {
            Some(Exclusion::Expected(reason)) => Some(reason),
            Some(other) => panic!("expected an ordinary exclusion, got {other:?}"),
            None => None,
        }
    }

    /// A `create-dir` carrying one condition, in the spelling named.
    fn conditioned(spelling: &str, condition: &str) -> RunRecord {
        RunRecord::leaf(
            toml::from_str(&format!(
                "type = \"create-dir\"\nid = \"zshrc\"\ngroup = \"shell\"\n\
                 dest = \"~/x\"\n{spelling} = \"{condition}\"\n"
            ))
            .expect("the record should parse"),
            1,
        )
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
        // `apply-action`: all four lists at once.
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
        // `nowhere` is declared by no layer, so evaluating it would fail.
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
        // A false `unless` installs, so treating a failure as false would
        // install the record `unless` was written to suppress.
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
            // The fix the reader acts on.
            assert!(why.contains("`nowhere` is not declared"), "{why}");
            assert!(why.contains("batfiles vars set nowhere"), "{why}");
        }
    }

    #[test]
    fn asking_for_one_action_waives_its_condition_too() {
        // Including a condition this machine cannot decide.
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
        // Repository text in a report is escaped.
        let selection = selection(&[], &[], &[], Disabled::default());
        let record = conditioned("when", "work && vars['a\\nb']");
        let why = expected_reason(&selection, &record, &[("work", "true")])
            .expect("the condition should be false");
        assert!(!why.contains('\n'), "{why}");
        assert!(why.contains("\\n"), "{why}");
    }

    #[test]
    fn a_name_that_is_not_an_address_is_dropped_rather_than_kept() {
        // Warned about when read, and never matched.
        let mut skips = SkipList::default();
        skips.extend(&names_of(&["a..b", "zshrc"]), "--skip-action", &quiet());
        assert_eq!(skips.origin(&address("zshrc")), Some("--skip-action"));
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

    // What an inclusion contributed, which is the same rule read from the other
    // side: a qualified name reaches it and an unqualified one does not.

    #[test]
    fn an_included_record_answers_to_its_qualified_name_alone() {
        let contributed = included("zshrc", "shell", Some("core"));

        for (skips, groups, expected) in [
            (["core.zshrc"], [""; 1], "`core.zshrc` from --skip-action"),
            ([""; 1], ["core.shell"], "`core.shell` from --skip-group"),
        ] {
            let selection = selection(&skips, &groups, &[], Disabled::default());
            assert_eq!(reason(&selection, &contributed).as_deref(), Some(expected));
        }

        // Unqualified names mean the leaf's records only.
        let unqualified = selection(
            &["zshrc"],
            &["shell"],
            &[],
            disabled(&["zshrc"], &["shell"]),
        );
        assert_eq!(reason(&unqualified, &contributed), None);
    }

    #[test]
    fn a_record_from_an_unnamed_inclusion_answers_to_nothing() {
        // Neither spelling reaches such a record: the qualified one has no first
        // segment to match, and the unqualified one means the leaf's own.
        let contributed = included("zshrc", "shell", None);
        let selection = selection(
            &["zshrc", "core.zshrc"],
            &["shell", "core.shell"],
            &[],
            disabled(&["zshrc", "core.zshrc"], &["shell", "core.shell"]),
        );
        assert_eq!(reason(&selection, &contributed), None);
    }

    #[test]
    fn a_target_names_an_included_record_by_its_qualified_address() {
        let contributed = included("zshrc", "shell", Some("core"));
        let leaf = action("zshrc", "shell");

        let qualified = address("core.zshrc");
        let by_action = filter(
            Target::Action(&qualified),
            &[],
            &[],
            &[],
            Disabled::default(),
        );
        assert!(by_action.wants(&contributed));
        assert!(!by_action.wants(&leaf));

        let qualified = address("core.shell");
        let by_group = filter(
            Target::Group(&qualified),
            &[],
            &[],
            &[],
            Disabled::default(),
        );
        assert!(by_group.wants(&contributed));
        assert!(!by_group.wants(&leaf));
    }

    #[test]
    fn a_target_reaches_into_the_inclusion_its_address_is_qualified_by() {
        // Asking for everything opens every inclusion, even unnamed ones.
        let core = item("core");
        let everything = selection(&[], &[], &[], Disabled::default());
        assert!(everything.reaches_into(Some(&core)));
        assert!(everything.reaches_into(None));

        let qualified = address("core.zshrc");
        let named = filter(
            Target::Action(&qualified),
            &[],
            &[],
            &[],
            Disabled::default(),
        );
        assert!(named.reaches_into(Some(&core)));
        assert!(!named.reaches_into(Some(&item("work"))));
        assert!(!named.reaches_into(None));

        // An address naming the inclusion itself reaches the record, not inside
        // it; `apply-action` refuses that one rather than opening it.
        let record = address("core");
        let inclusion = filter(Target::Action(&record), &[], &[], &[], Disabled::default());
        assert!(!inclusion.reaches_into(Some(&core)));
    }
}
