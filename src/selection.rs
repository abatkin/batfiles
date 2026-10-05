//! Select actions and clone-list entries using command targets, disabled state, run-only
//! skips, and conditions.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use crate::condition::{Bindings, Exclusion, Gate};
use crate::disabled::DisabledItems;
use crate::env::Environment;
use crate::error::Error;
use crate::item::{ItemAddress, ItemId, ItemKind};
use crate::manifest::action::Action;
use crate::output::Reporter;

/// The environment variable unioned with `--skip-action`.
const SKIP_ACTIONS: &str = "BATFILES_SKIP_ACTIONS";

/// The environment variable unioned with `--skip-group`.
const SKIP_GROUPS: &str = "BATFILES_SKIP_GROUPS";

/// Warning suffix for a condition evaluation failure.
const NOT_INSTALLED: &str = "it is not installed";

/// A record's action address, group address, and condition for selection. Included addresses
/// are qualified; absent IDs and unnamed inclusions have no address. A clone-list entry is
/// addressed under its list and has no group.
#[derive(Clone, Copy)]
pub(crate) struct Subject<'a> {
    pub address: Option<&'a ItemAddress>,
    pub group_address: Option<&'a ItemAddress>,
    pub gate: Option<Gate<'a>>,
}

/// What this run does with one record or clone-list entry.
pub(crate) enum Disposition {
    /// Not requested by the command; still available for skip matching.
    NotRequested,
    /// Requested, and excluded for this reason.
    Excluded(Exclusion),
    /// Requested and not excluded.
    Allowed,
    /// An inclusion or clone list that is not excluded, opened only because the
    /// target names something inside it. Nothing else it holds is requested.
    AllowedInPart,
}

impl Disposition {
    /// `Excluded` for an exclusion, `Allowed` for none.
    pub fn from_exclusion(exclusion: Option<Exclusion>) -> Self {
        match exclusion {
            Some(exclusion) => Self::Excluded(exclusion),
            None => Self::Allowed,
        }
    }

    /// This disposition for a record opened only to reach inside it: `Allowed`
    /// becomes `AllowedInPart`, and anything else is unchanged.
    pub fn in_part(self) -> Self {
        match self {
            Self::Allowed => Self::AllowedInPart,
            other => other,
        }
    }

    /// Whether the record may run or be opened, whole or in part.
    pub fn is_allowed(&self) -> bool {
        matches!(self, Self::Allowed | Self::AllowedInPart)
    }
}

/// Why a record holding addressable items, an inclusion or a clone list, was
/// not read.
pub(crate) enum Unread<'a> {
    /// The command did not request it.
    NotRequested,
    /// An inclusion that was requested, and excluded for this reason.
    ExcludedInclusion(&'a Exclusion),
    /// An admitted inclusion, with no materialization of this remote to read.
    NotMaterialized(&'a ItemId),
    /// A clone list that was requested, and excluded for this reason.
    ExcludedList(&'a Exclusion),
}

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
    fn wants(&self, record: Subject<'_>) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(id) => record.address == Some(*id),
            Self::Group(group) => record.group_address == Some(*group),
        }
    }

    /// Whether this target could name something inside the inclusion at
    /// `address`, so its manifest must be read. An inclusion without an `id` is
    /// opened only for [`Everything`](Self::Everything).
    fn reaches_into(&self, address: Option<&ItemAddress>) -> bool {
        match self {
            Self::Everything => true,
            Self::Action(target) | Self::Group(target) => {
                address.is_some_and(|it| target.within(it))
            }
        }
    }

    /// Whether this target names an entry of the clone list at `address`
    /// without naming the list, so the list must be read to find it. Entries
    /// have no group, and [`Everything`](Self::Everything) requests the list
    /// itself.
    fn reaches_into_list(&self, address: Option<&ItemAddress>) -> bool {
        match self {
            Self::Action(target) => address.is_some_and(|it| target.within(it)),
            Self::Everything | Self::Group(_) => false,
        }
    }

    /// Build an unmatched-target error, or return `None` for [`Everything`](Self::Everything).
    /// If the target falls within an unread inclusion or clone list, report why it was not read.
    fn unmatched_target_error<'b>(
        &self,
        manifest: PathBuf,
        unread: impl IntoIterator<Item = (&'b ItemAddress, Unread<'b>)>,
    ) -> Option<Error> {
        let (kind, address) = match self {
            Self::Everything => return None,
            Self::Action(id) => (ItemKind::Action, *id),
            Self::Group(group) => (ItemKind::Group, *group),
        };
        let unread = unread
            .into_iter()
            .filter(|(container, _)| address.within(container))
            .find_map(|(container, unread)| {
                let (address, container) = (address.clone(), container.clone());
                match unread {
                    Unread::NotRequested => None,
                    Unread::ExcludedInclusion(exclusion) => {
                        Some(Error::TargetInExcludedInclusion {
                            kind,
                            address,
                            inclusion: container,
                            reason: exclusion.reason().to_owned(),
                        })
                    }
                    Unread::NotMaterialized(remote) => Some(Error::TargetInUnreadInclusion {
                        kind,
                        address,
                        inclusion: container,
                        remote: remote.clone(),
                    }),
                    Unread::ExcludedList(exclusion) => {
                        matches!(kind, ItemKind::Action).then(|| Error::TargetInExcludedList {
                            address,
                            list: container,
                            reason: exclusion.reason().to_owned(),
                        })
                    }
                }
            });
        Some(unread.unwrap_or_else(|| match self {
            Self::Group(_) => Error::UnknownGroup {
                path: manifest,
                group: address.clone(),
            },
            _ => Error::UnknownAction {
                path: manifest,
                id: address.clone(),
            },
        }))
    }

    /// Whether exclusions naming a record's own address apply. `apply-action`
    /// waives them, and with them the record's own condition.
    fn honors_action_exclusions(&self) -> bool {
        !matches!(self, Self::Action(_))
    }

    /// Whether exclusions naming `group`, a record's group, apply. `apply-group`
    /// waives the group it names and no other, so a contributed record's own
    /// group still counts; `apply-action` waives its record's group.
    fn honors_group_exclusion(&self, group: &ItemAddress) -> bool {
        match self {
            Self::Everything => true,
            Self::Group(named) => *named != group,
            Self::Action(_) => false,
        }
    }

    /// Whether the command reads the run-only group skips; only
    /// [`Everything`](Self::Everything) does.
    fn reads_group_skips(&self) -> bool {
        matches!(self, Self::Everything)
    }
}

/// Which exclusions a decision waives.
#[derive(Clone, Copy)]
enum TargetWaivers {
    /// Apply the target's exemptions to exclusions.
    Apply,
    /// Waive none.
    Ignore,
}

impl TargetWaivers {
    fn honors_action_exclusions(self, target: &Target<'_>) -> bool {
        matches!(self, Self::Ignore) || target.honors_action_exclusions()
    }

    fn honors_group_exclusion(self, target: &Target<'_>, group: &ItemAddress) -> bool {
        matches!(self, Self::Ignore) || target.honors_group_exclusion(group)
    }
}

/// Why an action is not being run.
#[derive(Debug)]
pub(crate) enum SkipReason<'a> {
    /// A `disabled.toml` entry; `kind` names its list.
    Disabled {
        kind: ItemKind,
        name: &'a ItemAddress,
    },
    /// A run-only skip, and the option or variable that supplied it.
    RunOnly {
        name: &'a ItemAddress,
        origin: &'static str,
    },
}

impl fmt::Display for SkipReason<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disabled { kind, name } => write!(f, "{kind} `{name}` is disabled"),
            Self::RunOnly { name, origin } => write!(f, "`{name}` from {origin}"),
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
                // Keep the CLI origin when the environment repeats the same skip.
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

    /// Warn once per name not in `present`. Names within an `unread` address
    /// are skipped silently: nothing read could answer them.
    fn warn_unmatched(
        &self,
        present: &[&ItemAddress],
        unread: &[&ItemAddress],
        kind: ItemKind,
        reporter: &Reporter,
    ) {
        for (name, origin) in &self.names {
            if present.contains(&name) || unread.iter().any(|it| name.within(it)) {
                continue;
            }
            reporter.warn(&format!("{origin} `{name}` matched no {kind}"));
        }
    }
}

/// Combine CLI and environment skips when `honored`; otherwise return an empty list without
/// reading either.
fn read_skip_list(
    honored: bool,
    values: &[String],
    option: &'static str,
    variable: &'static str,
    env: &Environment,
    reporter: &Reporter,
) -> SkipList {
    let mut skips = SkipList::default();
    if honored {
        skips.extend(values, option, reporter);
        skips.extend(&env.list(variable), variable, reporter);
    }
    skips
}

/// Command target, persisted disabled state, and run-only skips used to select records.
#[derive(Debug)]
pub(crate) struct Selection<'a> {
    target: Target<'a>,
    actions: SkipList,
    groups: SkipList,
    disabled: DisabledItems,
}

impl<'a> Selection<'a> {
    /// Build the run's selection, reading only the skips the target honors.
    pub fn new(
        target: Target<'a>,
        skip_actions: &[String],
        skip_groups: &[String],
        env: &Environment,
        disabled: DisabledItems,
        reporter: &Reporter,
    ) -> Self {
        Self {
            actions: read_skip_list(
                target.honors_action_exclusions(),
                skip_actions,
                "--skip-action",
                SKIP_ACTIONS,
                env,
                reporter,
            ),
            groups: read_skip_list(
                target.reads_group_skips(),
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

    /// Select all records subject to persisted disabled state, without run-only skips. Used for
    /// variable-refresh reachability.
    pub fn without_run_skips(disabled: DisabledItems) -> Self {
        Self {
            target: Target::Everything,
            actions: SkipList::default(),
            groups: SkipList::default(),
            disabled,
        }
    }

    /// Whether the target asks for `record`.
    pub fn wants(&self, record: Subject<'_>) -> bool {
        self.target.wants(record)
    }

    /// Whether the target is the one action or entry at `address`.
    pub fn names(&self, address: &ItemAddress) -> bool {
        matches!(self.target, Target::Action(target) if target == address)
    }

    /// See [`Target::reaches_into`].
    pub fn reaches_into(&self, address: Option<&ItemAddress>) -> bool {
        self.target.reaches_into(address)
    }

    /// See [`Target::reaches_into_list`].
    pub fn reaches_into_list(&self, address: Option<&ItemAddress>) -> bool {
        self.target.reaches_into_list(address)
    }

    /// See [`Target::unmatched_target_error`].
    pub fn unmatched_target_error<'b>(
        &self,
        manifest: PathBuf,
        unread: impl IntoIterator<Item = (&'b ItemAddress, Unread<'b>)>,
    ) -> Option<Error> {
        self.target.unmatched_target_error(manifest, unread)
    }

    /// Reject an `apply-action` target that names an inclusion; return `None` for other
    /// targets.
    pub fn refusal(&self, action: &Action) -> Option<Error> {
        match (&self.target, action) {
            (Target::Action(id), Action::IncludeRemote(_)) => {
                Some(Error::InclusionNotApplyable { id: (*id).clone() })
            }
            _ => None,
        }
    }

    /// Warn about every run-only skip that names nothing the run listed.
    /// `addresses` and `group_addresses` are every listed record's, contributed
    /// ones and clone-list entries included. Skips within an `unread` inclusion
    /// or list are not warned about: nothing read could answer them.
    pub fn warn_unmatched(
        &self,
        addresses: &[&ItemAddress],
        group_addresses: &[&ItemAddress],
        unread: &[&ItemAddress],
        reporter: &Reporter,
    ) {
        self.actions
            .warn_unmatched(addresses, unread, ItemKind::Action, reporter);
        self.groups
            .warn_unmatched(group_addresses, unread, ItemKind::Group, reporter);
    }

    /// Return the first exclusion, or `None` if admitted. Apply target exemptions, then check
    /// disabled state and run-only skips before evaluating the condition. Evaluation failures
    /// exclude the record.
    pub fn exclusion(&self, record: Subject<'_>, bindings: &Bindings<'_>) -> Option<Exclusion> {
        self.decide(record, bindings, TargetWaivers::Apply, Some(NOT_INSTALLED))
    }

    /// Return the first exclusion without target exemptions. Check all loaded skip lists and
    /// the record's condition. Used for inclusions reached indirectly by a target.
    pub fn exclusion_without_waivers(
        &self,
        record: Subject<'_>,
        bindings: &Bindings<'_>,
    ) -> Option<Exclusion> {
        self.decide(record, bindings, TargetWaivers::Ignore, Some(NOT_INSTALLED))
    }

    /// Decide one entry of a clone list whose disposition is `list`, which allows it whole or
    /// in part.
    ///
    /// An entry the target names waives its own exclusions and condition. Every other entry
    /// of a list allowed whole is decided without waivers, so naming the list does not waive
    /// its entries' disables; the other entries of a list allowed in part are not requested.
    /// A failed condition's reason leaves the consequence to the caller's line.
    pub fn entry_disposition(
        &self,
        entry: Subject<'_>,
        list: &Disposition,
        bindings: &Bindings<'_>,
    ) -> Disposition {
        let waivers = if self.target.wants(entry) {
            TargetWaivers::Apply
        } else if matches!(list, Disposition::Allowed) {
            TargetWaivers::Ignore
        } else {
            return Disposition::NotRequested;
        };
        Disposition::from_exclusion(self.decide(entry, bindings, waivers, None))
    }

    /// The first exclusion `waivers` leave standing. `consequence` completes a failed
    /// condition's reason.
    fn decide(
        &self,
        record: Subject<'_>,
        bindings: &Bindings<'_>,
        waivers: TargetWaivers,
        consequence: Option<&str>,
    ) -> Option<Exclusion> {
        if let Some(reason) = self.listed_reason(record, waivers) {
            return Some(Exclusion::Deliberate(reason.to_string()));
        }
        // Direct action targets waive their own conditions as well as listed exclusions.
        let gate = record
            .gate
            .filter(|_| waivers.honors_action_exclusions(&self.target))?;
        gate.exclusion(bindings, consequence)
    }

    /// The first exclusion either list names, or `None` where neither does.
    /// Persistent disables precede run-only skips; action exclusions precede
    /// group exclusions within each source. `waivers` determines which
    /// exclusions are honored.
    fn listed_reason<'b>(
        &self,
        record: Subject<'b>,
        waivers: TargetWaivers,
    ) -> Option<SkipReason<'b>> {
        let address = record
            .address
            .filter(|_| waivers.honors_action_exclusions(&self.target));
        let group_address = record
            .group_address
            .filter(|group| waivers.honors_group_exclusion(&self.target, group));

        let listed = |list: &BTreeSet<ItemAddress>, item: &ItemAddress| list.contains(item);

        if let Some(name) = address.filter(|it| listed(&self.disabled.actions, it)) {
            return Some(SkipReason::Disabled {
                kind: ItemKind::Action,
                name,
            });
        }
        if let Some(name) = group_address.filter(|it| listed(&self.disabled.groups, it)) {
            return Some(SkipReason::Disabled {
                kind: ItemKind::Group,
                name,
            });
        }
        if let Some((name, origin)) = address.and_then(|it| Some((it, self.actions.origin(it)?))) {
            return Some(SkipReason::RunOnly { name, origin });
        }
        group_address.and_then(|it| {
            Some(SkipReason::RunOnly {
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
    use crate::execute::record::RunRecord;
    use crate::inclusion::Inclusion;
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

    /// Build an included `create-dir` record with an optional inclusion ID.
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

    fn disabled(actions: &[&str], groups: &[&str]) -> DisabledItems {
        DisabledItems {
            actions: actions.iter().map(|name| address(name)).collect(),
            groups: groups.iter().map(|name| address(name)).collect(),
        }
    }

    /// A `sync`'s filter: it asks for everything, so it waives nothing.
    fn selection(
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: DisabledItems,
    ) -> Selection<'static> {
        filter(Target::Everything, skip_actions, skip_groups, env, disabled)
    }

    fn filter<'a>(
        target: Target<'a>,
        skip_actions: &[&str],
        skip_groups: &[&str],
        env: &[(&str, &str)],
        disabled: DisabledItems,
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
        decided_with(selection, record, vars, TargetWaivers::Apply)
    }

    /// Evaluate the record using `vars` and the specified target exemptions.
    fn decided_with(
        selection: &Selection,
        record: &RunRecord,
        vars: &[(&str, &str)],
        waivers: TargetWaivers,
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
        selection.decide(record.subject(), &bindings, waivers, Some(NOT_INSTALLED))
    }

    /// Return the reason for a [`Deliberate`](Exclusion::Deliberate) exclusion; panic on
    /// evaluation failure.
    fn expected_reason(
        selection: &Selection,
        record: &RunRecord,
        vars: &[(&str, &str)],
    ) -> Option<String> {
        match decided(selection, record, vars) {
            Some(Exclusion::Deliberate(reason)) => Some(reason),
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
        let selection = selection(&["other"], &["other"], &[], DisabledItems::default());
        assert_eq!(reason(&selection, &action("zshrc", "shell")), None);
    }

    #[test]
    fn either_namespace_can_name_an_action() {
        let by_action = selection(&["zshrc"], &[], &[], DisabledItems::default());
        assert_eq!(
            reason(&by_action, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );
        let by_group = selection(&[], &["shell"], &[], DisabledItems::default());
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
            DisabledItems::default(),
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
        let selection = selection(
            &["zshrc"],
            &[],
            &[(SKIP_ACTIONS, "zshrc")],
            DisabledItems::default(),
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
        let selection = selection(&["zshrc"], &["shell"], &[], DisabledItems::default());
        assert_eq!(
            reason(&selection, &action("zshrc", "shell")).as_deref(),
            Some("`zshrc` from --skip-action")
        );
    }

    #[test]
    fn asking_for_a_group_waives_that_group_and_no_other() {
        let work = address("work");
        let by_group = filter(
            Target::Group(&work),
            &[],
            &[],
            &[],
            disabled(&[], &["work", "corp.shell"]),
        );
        assert_eq!(reason(&by_group, &action("corp", "work")), None);
        assert_eq!(
            reason(&by_group, &included("zshrc", "shell", Some("corp"))).as_deref(),
            Some("group `corp.shell` is disabled")
        );
        assert_eq!(
            reason(&by_group, &included("p10k", "prompt", Some("corp"))),
            None
        );

        let corp_shell = address("corp.shell");
        let by_inner_group = filter(
            Target::Group(&corp_shell),
            &[],
            &[],
            &[],
            disabled(&[], &["work", "corp.shell"]),
        );
        assert_eq!(
            reason(&by_inner_group, &included("zshrc", "shell", Some("corp"))),
            None
        );
    }

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
    fn an_unwaived_decision_honors_every_list_the_selection_read() {
        // No-waiver decisions still use only the skip lists loaded for this command.
        let shell = address("corp.shell");
        let unwaived = |skip_actions: &[&str], skip_groups: &[&str], disabled| {
            let selection = filter(
                Target::Group(&shell),
                skip_actions,
                skip_groups,
                &[],
                disabled,
            );
            decided_with(
                &selection,
                &action("corp", "work"),
                &[],
                TargetWaivers::Ignore,
            )
            .map(|it| it.reason().to_owned())
        };
        assert_eq!(
            unwaived(&[], &[], disabled(&[], &["work"])).as_deref(),
            Some("group `work` is disabled")
        );
        assert_eq!(
            unwaived(&["corp"], &[], DisabledItems::default()).as_deref(),
            Some("`corp` from --skip-action")
        );
        assert_eq!(unwaived(&[], &["work"], DisabledItems::default()), None);

        let zshrc = address("corp.zshrc");
        let by_name = filter(
            Target::Action(&zshrc),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        let unwaived = decided_with(
            &by_name,
            &conditioned("when", "work"),
            &[("work", "false")],
            TargetWaivers::Ignore,
        );
        assert_eq!(
            unwaived.map(|it| it.reason().to_owned()).as_deref(),
            Some("when \"work\" is false")
        );
    }

    #[test]
    fn a_condition_decides_the_record_it_is_written_on() {
        let selection = selection(&[], &[], &[], DisabledItems::default());
        let vars = &[("work", "false")];

        assert_eq!(
            expected_reason(&selection, &conditioned("when", "work"), vars).as_deref(),
            Some("when \"work\" is false")
        );
        assert_eq!(
            expected_reason(&selection, &conditioned("unless", "work"), vars),
            None
        );

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
        let skipping = selection(&["zshrc"], &[], &[], DisabledItems::default());
        assert_eq!(
            expected_reason(&skipping, &conditioned("when", "nowhere"), &[]).as_deref(),
            Some("`zshrc` from --skip-action")
        );

        let plain = selection(&[], &[], &[], DisabledItems::default());
        assert!(matches!(
            decided(&plain, &conditioned("when", "nowhere"), &[]),
            Some(Exclusion::EvaluationFailed(_))
        ));
    }

    #[test]
    fn a_condition_that_cannot_be_decided_closes_the_gate_in_either_spelling() {
        // An evaluation error must exclude even under `unless`.
        let selection = selection(&[], &[], &[], DisabledItems::default());

        for spelling in ["when", "unless"] {
            let Some(Exclusion::EvaluationFailed(why)) =
                decided(&selection, &conditioned(spelling, "nowhere"), &[])
            else {
                panic!("`{spelling}` should close on a condition nothing can decide");
            };
            assert!(why.starts_with(&format!("{spelling} \"nowhere\"")), "{why}");
            assert!(why.contains("cannot be evaluated"), "{why}");
            assert!(why.contains(NOT_INSTALLED), "{why}");
            assert!(why.contains("`nowhere` is not declared"), "{why}");
            assert!(why.contains("batfiles vars set nowhere"), "{why}");
        }
    }

    #[test]
    fn asking_for_one_action_waives_its_condition_too() {
        let zshrc = address("zshrc");
        let by_name = filter(
            Target::Action(&zshrc),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert_eq!(
            decided(&by_name, &conditioned("when", "work"), &[("work", "false")]),
            None
        );
        assert_eq!(
            decided(&by_name, &conditioned("when", "nowhere"), &[]),
            None
        );

        let shell = address("shell");
        let by_group = filter(
            Target::Group(&shell),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
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
        let selection = selection(&[], &[], &[], DisabledItems::default());
        assert_eq!(decided(&selection, &action("zshrc", "shell"), &[]), None);
    }

    #[test]
    fn a_condition_is_repeated_back_as_written_and_cannot_forge_a_line() {
        let selection = selection(&[], &[], &[], DisabledItems::default());
        let record = conditioned("when", "work && vars['a\\nb']");
        let why = expected_reason(&selection, &record, &[("work", "true")])
            .expect("the condition should be false");
        assert!(!why.contains('\n'), "{why}");
        assert!(why.contains("\\n"), "{why}");
    }

    #[test]
    fn a_name_that_is_not_an_address_is_dropped_rather_than_kept() {
        let mut skips = SkipList::default();
        skips.extend(&names_of(&["a..b", "zshrc"]), "--skip-action", &quiet());
        assert_eq!(skips.origin(&address("zshrc")), Some("--skip-action"));
        assert_eq!(skips.names.len(), 1);
    }

    #[test]
    fn a_qualified_skip_is_kept_and_names_no_leaf_record() {
        let by_skip = selection(
            &["core.zshrc"],
            &["core.shell"],
            &[],
            DisabledItems::default(),
        );
        assert_eq!(reason(&by_skip, &action("zshrc", "shell")), None);

        let by_disable = selection(&[], &[], &[], disabled(&["core.zshrc"], &["core.shell"]));
        assert_eq!(reason(&by_disable, &action("zshrc", "shell")), None);
    }

    #[test]
    fn an_included_record_answers_to_its_qualified_name_alone() {
        let contributed = included("zshrc", "shell", Some("core"));

        for (skips, groups, expected) in [
            (["core.zshrc"], [""; 1], "`core.zshrc` from --skip-action"),
            ([""; 1], ["core.shell"], "`core.shell` from --skip-group"),
        ] {
            let selection = selection(&skips, &groups, &[], DisabledItems::default());
            assert_eq!(reason(&selection, &contributed).as_deref(), Some(expected));
        }

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
            DisabledItems::default(),
        );
        assert!(by_action.wants(contributed.subject()));
        assert!(!by_action.wants(leaf.subject()));

        let qualified = address("core.shell");
        let by_group = filter(
            Target::Group(&qualified),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert!(by_group.wants(contributed.subject()));
        assert!(!by_group.wants(leaf.subject()));
    }

    #[test]
    fn a_target_reaches_into_the_inclusion_its_address_is_qualified_by() {
        let core = address("core");
        let everything = selection(&[], &[], &[], DisabledItems::default());
        assert!(everything.reaches_into(Some(&core)));
        assert!(everything.reaches_into(None));

        let qualified = address("core.zshrc");
        let named = filter(
            Target::Action(&qualified),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert!(named.reaches_into(Some(&core)));
        assert!(!named.reaches_into(Some(&address("work"))));
        assert!(!named.reaches_into(None));

        // An inclusion's own address does not reach its children.
        let record = address("core");
        let inclusion = filter(
            Target::Action(&record),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert!(!inclusion.reaches_into(Some(&core)));
    }

    #[test]
    fn only_an_action_target_reaches_into_a_clone_list() {
        let list = address("core.plugins");
        let entry = address("core.plugins.p10k");
        let by_action = filter(
            Target::Action(&entry),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert!(by_action.reaches_into_list(Some(&list)));
        assert!(!by_action.reaches_into_list(Some(&address("plugins"))));
        assert!(!by_action.reaches_into_list(None));

        let by_group = filter(
            Target::Group(&entry),
            &[],
            &[],
            &[],
            DisabledItems::default(),
        );
        assert!(!by_group.reaches_into_list(Some(&list)));
        let everything = selection(&[], &[], &[], DisabledItems::default());
        assert!(!everything.reaches_into_list(Some(&list)));
    }

    /// An entry subject at `address`, under a `when = "work"` gate.
    fn entry_disposition(
        selection: &Selection,
        address: &ItemAddress,
        list: &Disposition,
    ) -> Disposition {
        let condition = crate::condition::Condition::new("work").expect("a condition");
        let entry = Subject {
            address: Some(address),
            group_address: None,
            gate: Some(Gate::When(&condition)),
        };
        let variables = Rc::new(VarSet::stack(
            BTreeMap::from([(
                VarName::try_from("work".to_owned()).expect("valid name"),
                crate::var_set::VarValue::Static("false".to_owned()),
            )]),
            BTreeMap::new(),
            BTreeMap::new(),
            &[],
        ));
        let empty = Environment::from_pairs(std::iter::empty::<(&str, &str)>());
        let host = HostNamespaces::capture(&empty);
        selection.entry_disposition(entry, list, &Bindings::new(&variables, &host))
    }

    fn reason_of(disposition: Disposition) -> Option<String> {
        match disposition {
            Disposition::Excluded(exclusion) => Some(exclusion.reason().to_owned()),
            Disposition::Allowed => None,
            Disposition::AllowedInPart => Some("allowed in part".to_owned()),
            Disposition::NotRequested => Some("not requested".to_owned()),
        }
    }

    #[test]
    fn an_entry_named_by_the_target_waives_its_own_exclusions() {
        let entry = address("plugins.p10k");
        let named = filter(
            Target::Action(&entry),
            &[],
            &[],
            &[],
            disabled(&["plugins.p10k"], &[]),
        );
        assert_eq!(
            reason_of(entry_disposition(
                &named,
                &entry,
                &Disposition::AllowedInPart
            )),
            None
        );
        let other = address("plugins.zsh-z");
        assert_eq!(
            reason_of(entry_disposition(
                &named,
                &other,
                &Disposition::AllowedInPart
            ))
            .as_deref(),
            Some("not requested")
        );
    }

    #[test]
    fn naming_a_list_does_not_waive_its_entries_exclusions() {
        let list = address("plugins");
        let named = filter(
            Target::Action(&list),
            &[],
            &[],
            &[],
            disabled(&["plugins.p10k"], &[]),
        );
        assert_eq!(
            reason_of(entry_disposition(
                &named,
                &address("plugins.p10k"),
                &Disposition::Allowed
            ))
            .as_deref(),
            Some("action `plugins.p10k` is disabled")
        );
        assert_eq!(
            reason_of(entry_disposition(
                &named,
                &address("plugins.zsh-z"),
                &Disposition::Allowed
            ))
            .as_deref(),
            Some("when \"work\" is false")
        );
    }

    #[test]
    fn a_skipped_entry_is_not_asked_its_condition() {
        let sync = selection(&["plugins.p10k"], &[], &[], DisabledItems::default());
        assert_eq!(
            reason_of(entry_disposition(
                &sync,
                &address("plugins.p10k"),
                &Disposition::Allowed
            ))
            .as_deref(),
            Some("`plugins.p10k` from --skip-action")
        );
    }
}
