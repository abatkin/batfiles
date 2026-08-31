//! Which of a manifest's actions a run carries out.
//!
//! Two sources say an action should be passed over: the machine-local
//! [`Disabled`] lists, which persist, and the run-only skips given as
//! `--skip-action`/`--skip-group` or in `BATFILES_SKIP_ACTIONS`/
//! `BATFILES_SKIP_GROUPS`. They filter the same ordered list against the same
//! two namespaces, so there is one filter here rather than one per source.
//!
//! They differ in exactly one rule. A run-only name that matches nothing warns:
//! it was typed for this run, and `sync` has the manifest loaded, so it can
//! tell. A `disabled.toml` entry that matches nothing is silent, because
//! pre-registering a name a later branch introduces is what that file is for.
//!
//! This is the negative half of the question. Which entries a run *asks* for is
//! [`crate::execute::Target`]'s, and the two meet at one rule: **an exclusion no
//! finer-grained than what the command named is waived.** `sync` names nothing,
//! so every exclusion outranks it and none is waived; `apply-group` names a
//! group, which waives a disable or a skip on that group and leaves one naming a
//! member standing; nothing is finer-grained than the one action `apply-action`
//! names, so it waives all four lists. That granularity is [`Named`], and it is
//! the only thing the three commands' filters differ in.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::disabled::Disabled;
use crate::env::Environment;
use crate::item::ItemId;
use crate::manifest::action::Action;
use crate::output::Reporter;

/// The environment variable unioned with `--skip-action`.
const SKIP_ACTIONS: &str = "BATFILES_SKIP_ACTIONS";

/// The environment variable unioned with `--skip-group`.
const SKIP_GROUPS: &str = "BATFILES_SKIP_GROUPS";

/// How specifically a command named what it asked for.
///
/// The one thing the three commands' filters differ in: an exclusion no
/// finer-grained than this is waived. The variants are declared coarse to fine,
/// which is the comparison [`Named::honors_actions`] and [`Named::honors_groups`]
/// make.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Named {
    /// `sync`, which asks for the whole manifest. Every exclusion names
    /// something more specific than that, so none is waived.
    Nothing,
    /// `apply-group`, which names one group.
    Group,
    /// `apply-action`, which names one action.
    Action,
}

impl Named {
    /// Whether an exclusion naming an action's own ID still applies.
    fn honors_actions(self) -> bool {
        self < Self::Action
    }

    /// Whether an exclusion naming an action's group still applies.
    fn honors_groups(self) -> bool {
        self < Self::Group
    }
}

/// Why an action is not being run.
///
/// Borrows the name from whichever list matched, so reporting a skip allocates
/// nothing but the line itself.
#[derive(Debug)]
pub(crate) enum Skipped<'a> {
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

impl fmt::Display for Skipped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled { noun, name } => write!(f, "{noun} `{name}` is disabled"),
            Self::Run { name, origin } => write!(f, "`{name}` from {origin}"),
        }
    }
}

/// One run-only skip list: the names to pass over, each remembering the source
/// that supplied it.
///
/// A map rather than a set because the two sources union, and a line reporting
/// a skip should name the one the reader can go and change. Command-line
/// arguments outrank the environment everywhere else, so the option wins the
/// attribution where both named the same thing.
#[derive(Debug, Default)]
struct Skips {
    names: BTreeMap<ItemId, &'static str>,
}

impl Skips {
    /// Validate and add every name one source supplied.
    ///
    /// A name that is not an ID cannot match anything, which is the outcome a
    /// non-match already has, so it warns and is dropped rather than failing the
    /// run. That is deliberately not the rule for `disable-action`, which fails:
    /// recording the name is all that command does, so a rejected one leaves it
    /// with nothing to do.
    fn extend(&mut self, values: &[String], origin: &'static str, reporter: &Reporter) {
        for value in values {
            match ItemId::try_from(value.clone()) {
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
        self.names.get(candidate).copied()
    }

    /// Warn once per name that nothing in the manifest answers to.
    fn warn_unmatched(&self, present: &BTreeSet<&ItemId>, noun: &str, reporter: &Reporter) {
        for (name, origin) in &self.names {
            if !present.contains(name) {
                reporter.warn(&format!("{origin} `{name}` matched no {noun}"));
            }
        }
    }
}

/// One namespace's run-only skips: the option's names unioned with the
/// variable's, or nothing at all where this run waives them.
///
/// The option is read first, so a name given both ways is attributed to it.
fn run_only(
    consulted: bool,
    values: &[String],
    option: &'static str,
    variable: &'static str,
    env: &Environment,
    reporter: &Reporter,
) -> Skips {
    let mut skips = Skips::default();
    if consulted {
        skips.extend(values, option, reporter);
        skips.extend(&env.list(variable), variable, reporter);
    }
    skips
}

/// The two sources and the waiver, combined into the one filter a run applies.
#[derive(Debug)]
pub(crate) struct Selection {
    named: Named,
    actions: Skips,
    groups: Skips,
    disabled: Disabled,
}

impl Selection {
    /// The filter one run applies: everything either source names, less
    /// whatever `named` waives.
    ///
    /// A waived namespace's run-only list is not read at all, so no name in it
    /// warns about its syntax or its failure to match. The machine-local lists
    /// are taken whole and waived in [`Selection::skipped`].
    pub fn new(
        named: Named,
        skip_actions: &[String],
        skip_groups: &[String],
        env: &Environment,
        disabled: Disabled,
        reporter: &Reporter,
    ) -> Self {
        Self {
            actions: run_only(
                named.honors_actions(),
                skip_actions,
                "--skip-action",
                SKIP_ACTIONS,
                env,
                reporter,
            ),
            groups: run_only(
                named.honors_groups(),
                skip_groups,
                "--skip-group",
                SKIP_GROUPS,
                env,
                reporter,
            ),
            named,
            disabled,
        }
    }

    /// Warn about every run-only skip that names nothing the manifest declares.
    ///
    /// Reported ahead of the first action rather than at the end, because it is
    /// a complaint about the invocation and a run that fails partway should not
    /// swallow it. A waived namespace says nothing here, its list never having
    /// been read.
    pub fn warn_unmatched(&self, actions: &[Action], reporter: &Reporter) {
        let ids: BTreeSet<&ItemId> = actions.iter().filter_map(Action::id).collect();
        let groups: BTreeSet<&ItemId> = actions.iter().filter_map(Action::group).collect();
        self.actions.warn_unmatched(&ids, "action", reporter);
        self.groups.warn_unmatched(&groups, "group", reporter);
    }

    /// Why this action is being passed over, or `None` to carry it out.
    ///
    /// The waiver is applied first, by taking the waived name away: in a
    /// namespace the request already outranks, the record presents nothing for
    /// either list — persistent or run-only — to match.
    ///
    /// Among the reasons that remain, a disable is reported ahead of a skip
    /// because it is the reason that will still be there tomorrow, when the skip
    /// that was typed for one run is gone. An action's own name is reported
    /// ahead of its group's for the same kind of reason: it is the more specific
    /// of the two.
    ///
    /// Every name reported is the record's own, so a reason borrows the action
    /// rather than the list that matched it.
    pub fn skipped<'a>(&self, action: &'a Action) -> Option<Skipped<'a>> {
        let id = action.id().filter(|_| self.named.honors_actions());
        let group = action.group().filter(|_| self.named.honors_groups());

        if let Some(name) = id.filter(|id| self.disabled.actions.contains(id)) {
            return Some(Skipped::Disabled {
                noun: "action",
                name,
            });
        }
        if let Some(name) = group.filter(|group| self.disabled.groups.contains(group)) {
            return Some(Skipped::Disabled {
                noun: "group",
                name,
            });
        }
        if let Some((name, origin)) = id.and_then(|id| Some((id, self.actions.origin(id)?))) {
            return Some(Skipped::Run { name, origin });
        }
        if let Some((name, origin)) = group.and_then(|g| Some((g, self.groups.origin(g)?))) {
            return Some(Skipped::Run { name, origin });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::Verbosity;

    fn quiet() -> Reporter {
        let mut reporter = Reporter::new(false);
        reporter.set_verbosity(Verbosity::Quiet);
        reporter
    }

    fn id(id: &str) -> ItemId {
        ItemId::try_from(id.to_owned()).expect("valid ID")
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
            actions: actions.iter().map(|name| id(name)).collect(),
            groups: groups.iter().map(|name| id(name)).collect(),
        }
    }

    /// A `sync`'s filter: it names nothing, so it waives nothing.
    fn selection(
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: Disabled,
    ) -> Selection {
        filter(Named::Nothing, skip_actions, skip_groups, env, disabled)
    }

    fn filter(
        named: Named,
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: Disabled,
    ) -> Selection {
        Selection::new(
            named,
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
    /// it.
    fn reason(selection: &Selection, action: &Action) -> Option<String> {
        selection.skipped(action).map(|why| why.to_string())
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
        // Every list names this record. The disable is the reason still standing
        // once the run-only skip is gone, and the action's own name is more
        // specific than its group's.
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

    // The waiver: an exclusion no finer-grained than what the command named
    // does not apply. Each case names the record every list above names.

    #[test]
    fn naming_a_group_waives_the_group_level_exclusions_only() {
        // `apply-group`. The disable and the skip naming the group go; the ones
        // naming the action itself are more specific than what was asked for
        // and stay, in the order a `sync` reports them.
        let by_skip = filter(
            Named::Group,
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
            Named::Group,
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
    fn naming_an_action_waives_every_exclusion() {
        // `apply-action`. Nothing is finer-grained than the one action named,
        // so all four lists are waived.
        let selection = filter(
            Named::Action,
            &["zshrc"],
            &["shell"],
            &[(SKIP_ACTIONS, "zshrc"), (SKIP_GROUPS, "shell")],
            disabled(&["zshrc"], &["shell"]),
        );
        assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
    }

    #[test]
    fn a_name_that_is_not_an_id_is_dropped_rather_than_kept() {
        // It could never match, so it is warned about at the point it is read
        // and takes no part in the filtering.
        let mut skips = Skips::default();
        skips.extend(
            &names_of(&["core.zshrc", "zshrc"]),
            "--skip-action",
            &quiet(),
        );
        assert_eq!(skips.origin(&id("zshrc")), Some("--skip-action"));
        assert_eq!(skips.names.len(), 1);
    }
}
