//! Select actions and clone-list entries using command targets, disabled state, run-only
//! skips, and conditions.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;

use crate::env::Environment;
use crate::error::Error;
use crate::manifest::actions::Action;
use crate::manifest::condition::{Bindings, Exclusion, Gate};
use crate::manifest::disabled::DisabledItems;
use crate::manifest::item::{ItemAddress, ItemId, ItemKind};
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
                address.is_some_and(|address| target.within(address))
            }
        }
    }

    /// Whether this target names an entry of the clone list at `address`
    /// without naming the list, so the list must be read to find it. Entries
    /// have no group, and [`Everything`](Self::Everything) requests the list
    /// itself.
    fn reaches_into_list(&self, address: Option<&ItemAddress>) -> bool {
        match self {
            Self::Action(target) => address.is_some_and(|address| target.within(address)),
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
            if present.contains(&name) || unread.iter().any(|parent| name.within(parent)) {
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
    skipped_actions: SkipList,
    skipped_groups: SkipList,
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
            skipped_actions: read_skip_list(
                target.honors_action_exclusions(),
                skip_actions,
                "--skip-action",
                SKIP_ACTIONS,
                env,
                reporter,
            ),
            skipped_groups: read_skip_list(
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
            skipped_actions: SkipList::default(),
            skipped_groups: SkipList::default(),
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
        self.skipped_actions
            .warn_unmatched(addresses, unread, ItemKind::Action, reporter);
        self.skipped_groups
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

        if let Some(name) = address.filter(|address| listed(&self.disabled.actions, address)) {
            return Some(SkipReason::Disabled {
                kind: ItemKind::Action,
                name,
            });
        }
        if let Some(name) = group_address.filter(|address| listed(&self.disabled.groups, address)) {
            return Some(SkipReason::Disabled {
                kind: ItemKind::Group,
                name,
            });
        }
        if let Some((name, origin)) =
            address.and_then(|address| Some((address, self.skipped_actions.origin(address)?)))
        {
            return Some(SkipReason::RunOnly { name, origin });
        }
        group_address.and_then(|group| {
            Some(SkipReason::RunOnly {
                name: group,
                origin: self.skipped_groups.origin(group)?,
            })
        })
    }
}

#[cfg(test)]
mod tests;
