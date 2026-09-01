//! Which of a manifest's actions a run carries out.
//!
//! A run asks for part of the manifest — everything, one action, or one group —
//! and then passes over whatever the machine-local [`Disabled`] lists or this
//! run's `--skip-action`/`--skip-group` name. Both halves are [`Selection`],
//! because one rule joins them: **an exclusion no finer-grained than what the
//! command asked for is waived.** `sync` asks for everything, so every exclusion
//! outranks it and none is waived; `apply-group` names a group, which waives an
//! exclusion on that group and leaves one naming a member standing; nothing is
//! finer-grained than the one action `apply-action` names, so it waives all four
//! lists.
//!
//! The two exclusion sources differ in exactly one rule. A run-only name that
//! matches nothing warns: it was typed for this run, and the manifest is loaded,
//! so it can be told. A `disabled.toml` entry that matches nothing is silent,
//! because pre-registering a name a later branch introduces is what that file is
//! for.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

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

/// Which of the manifest's records a command asked for.
///
/// Holds the borrowed name rather than resolving to an index, because a group
/// names any number of records and an action's position is what the report
/// calls it.
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
    ///
    /// Naming an action or a group that nothing in the manifest answers to is a
    /// command resolving nothing, and each gets its own failure. A group is
    /// nothing but the actions naming it, so an empty one and an absent one are
    /// the same failure. `sync` names nothing to resolve: an empty manifest is
    /// an ordinary successful run that did nothing.
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
///
/// Borrows the name from whichever list matched, so reporting a skip allocates
/// nothing but the line itself.
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
///
/// A map rather than a set because the two sources union, and a line reporting
/// a skip should name the one the reader can go and change. Command-line
/// arguments outrank the environment everywhere else, so the option wins the
/// attribution where both named the same thing.
#[derive(Debug, Default)]
struct SkipList {
    names: BTreeMap<ItemAddress, &'static str>,
}

impl SkipList {
    /// Validate and add every name one source supplied.
    ///
    /// A name that is not an address cannot match anything, which is the outcome
    /// a non-match already has, so it warns and is dropped rather than failing
    /// the run. That is deliberately not the rule for `disable-action`, which
    /// fails: recording the name is all that command does, so a rejected one
    /// leaves it with nothing to do.
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
///
/// The option is read first, so a name given both ways is attributed to it.
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
    ///
    /// A waived namespace's run-only list is not read at all, so no name in it
    /// warns about its syntax or its failure to match. The machine-local lists
    /// are taken whole and waived in [`Selection::skipped`].
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
    pub fn skipped<'b>(&self, action: &'b Action) -> Option<SkipReason<'b>> {
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
        if let Some((name, origin)) = group.and_then(|g| Some((g, self.groups.origin(g)?))) {
            return Some(SkipReason::Run { name, origin });
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

    // The waiver: an exclusion no finer-grained than what the command asked for
    // does not apply. Each case names the record every list above names.

    #[test]
    fn asking_for_a_group_waives_the_group_level_exclusions_only() {
        // `apply-group`. The disable and the skip naming the group go; the ones
        // naming the action itself are more specific than what was asked for
        // and stay, in the order a `sync` reports them.
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
        // It is well formed, so it is not dropped; it names an action an
        // included remote would have contributed, and there are none, so it
        // selects nothing and is left to `warn_unmatched` to complain about.
        let by_skip = selection(&["core.zshrc"], &["core.shell"], &[], Disabled::default());
        assert_eq!(reason(&by_skip, &action("zshrc", "shell")), None);

        // Same shape in the persistent lists, which are silent about it.
        let by_disable = selection(&[], &[], &[], disabled(&["core.zshrc"], &["core.shell"]));
        assert_eq!(reason(&by_disable, &action("zshrc", "shell")), None);
    }
}
