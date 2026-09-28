//! The run's list: each record's action, addresses, heading, and what this run
//! does with it, with each inclusion owning the records it contributed.

use std::rc::Rc;

use super::inclusion::Inclusion;
use crate::condition::Exclusion;
use crate::item::{ItemAddress, ItemId};
use crate::manifest::action::{Action, Contributor};
use crate::var_set::VarSet;

/// One record in the run's list: an action, its addresses, its report heading,
/// and what this run does with it.
pub(crate) struct RunRecord {
    pub action: Action,
    /// The address the record's `id` answers to. `None` for a record without an
    /// `id` or one from an inclusion without an `id`. A contributed record's
    /// address is always qualified, so an unqualified `zshrc` reaches only the
    /// leaf's record.
    pub address: Option<ItemAddress>,
    /// The address of the record's `group`, qualified the same way.
    pub group_address: Option<ItemAddress>,
    /// How reports name the record, fixed when the record is built.
    pub(super) heading: String,
    pub(super) disposition: Disposition,
}

impl RunRecord {
    /// A record the leaf manifest declared, at its one-based position in it.
    pub fn leaf(action: Action, number: usize) -> Self {
        Self::new(action, number, Contributor::Leaf)
    }

    /// A record an inclusion contributed, at its one-based position in the
    /// manifest that declared it. `by` names the inclusion.
    pub fn contributed(action: Action, number: usize, by: Contributor<'_>) -> Self {
        Self::new(action, number, by)
    }

    fn new(action: Action, number: usize, by: Contributor<'_>) -> Self {
        // A record from an inclusion without an `id` has no address: there is no
        // qualifier to match, and an unqualified address means the leaf's
        // record. Its heading names the inclusion instead. Addresses and heading
        // use the same qualifier.
        let qualifier = by.qualifier();
        let addressed = qualifier.is_some() || matches!(by, Contributor::Leaf);
        let qualify = |id: Option<&ItemId>| {
            id.filter(|_| addressed)
                .map(|id| ItemAddress::qualified(qualifier, id))
        };
        let (address, group_address) = (qualify(action.id()), qualify(action.group()));
        let heading = action.describe(number, by);
        Self {
            heading,
            action,
            address,
            group_address,
            disposition: Disposition::Unwanted,
        }
    }
}

/// What this run does with one record.
pub(super) enum Disposition {
    /// Not requested by the command. Kept in the list so a skip naming it is not
    /// reported as matching nothing.
    Unwanted,
    /// Requested, and excluded for this reason.
    Excluded(Exclusion),
    /// Requested, and executed.
    Run,
}

/// One record of the leaf manifest, in declaration order.
pub(super) enum Node {
    /// Any record but an inclusion.
    Action(RunRecord),
    /// An `include-remote`. `record`'s disposition says whether the run
    /// requested and admitted it. `opened` is `None` when the manifest was not
    /// read: the inclusion was not requested, was excluded, or was admitted with
    /// no materialization, which leaves the plan partial.
    Inclusion {
        record: RunRecord,
        inclusion: Inclusion,
        opened: Option<Opened>,
    },
}

/// What an admitted inclusion read out of its remote's manifest.
pub(super) struct Opened {
    /// The scope its records and their clone lists' entries are decided in:
    /// [derived](crate::var_set::VarSet::with_inclusion) for the inclusion, or
    /// the run's set where neither it nor its remote declared variables.
    pub scope: Rc<VarSet>,
    /// Every record the manifest declared, in its order, including those the
    /// inclusion's filters left out. Never an inclusion.
    pub records: Vec<RunRecord>,
}

/// The run's list, and what reports need to know about its assembly.
pub(crate) struct RunList {
    pub(super) nodes: Vec<Node>,
    /// Whether the target named a record in the list. An inclusion opened only
    /// to reach inside it does not count.
    pub(super) target_found: bool,
}

impl RunList {
    /// Every record the run knows of in declaration order: each leaf record,
    /// followed, for an opened inclusion, by the records it contributed.
    pub fn records(&self) -> impl Iterator<Item = &RunRecord> {
        self.nodes.iter().flat_map(|node| {
            let (record, opened) = match node {
                Node::Action(record) => (record, None),
                Node::Inclusion { record, opened, .. } => (record, opened.as_ref()),
            };
            std::iter::once(record).chain(opened.into_iter().flat_map(|it| &it.records))
        })
    }

    /// The ID of each inclusion whose manifest this run did not read, and why.
    /// Nothing inside one was listed, so no address qualified by its ID can be
    /// told to match nothing.
    pub fn unread_inclusions(&self) -> impl Iterator<Item = (&ItemId, Unread<'_>)> {
        self.nodes.iter().filter_map(|node| match node {
            Node::Inclusion {
                record,
                inclusion,
                opened: None,
            } => {
                let unread = match &record.disposition {
                    Disposition::Unwanted => Unread::NotRequested,
                    Disposition::Excluded(exclusion) => Unread::Excluded(exclusion),
                    Disposition::Run => Unread::NotMaterialized(inclusion.remote()),
                };
                Some((inclusion.id()?, unread))
            }
            _ => None,
        })
    }
}

/// Why an inclusion's manifest was not read.
pub(crate) enum Unread<'a> {
    /// The run did not request it.
    NotRequested,
    /// Requested, and excluded for this reason.
    Excluded(&'a Exclusion),
    /// Admitted, with no materialization of this remote to read.
    NotMaterialized(&'a ItemId),
}
