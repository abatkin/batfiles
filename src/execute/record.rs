//! The run's list: each record's action, addresses, heading, provenance, and
//! what this run does with it.

use std::rc::Rc;

use super::inclusion::Contribution;
use crate::condition::Exclusion;
use crate::item::{ItemAddress, ItemId};
use crate::manifest::action::{Action, Contributor};
use crate::var_set::VarSet;

/// One entry in the run's list: an action, its addresses, its report heading,
/// and what this run does with it.
pub(crate) struct RunRecord {
    pub action: Action,
    /// The [`Contribution`] of the inclusion that contributed this record, shared
    /// by all of that inclusion's records; `None` for a leaf record.
    from: Option<Rc<Contribution>>,
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
        Self::new(action, number, None)
    }

    /// A record an inclusion contributed, at its one-based position in the
    /// manifest that declared it.
    pub fn contributed(action: Action, number: usize, from: &Rc<Contribution>) -> Self {
        Self::new(action, number, Some(Rc::clone(from)))
    }

    fn new(action: Action, number: usize, from: Option<Rc<Contribution>>) -> Self {
        let by = from
            .as_ref()
            .map_or(Contributor::Leaf, |it| it.contributor());
        // A record from an inclusion without an `id` has no address: there is no
        // qualifier to match, and an unqualified address means the leaf's
        // record. Its heading names the inclusion instead. Addresses and heading
        // use the same qualifier.
        let qualifier = by.qualifier();
        let addressed = qualifier.is_some() || from.is_none();
        let qualify = |id: Option<&ItemId>| {
            id.filter(|_| addressed)
                .map(|id| ItemAddress::qualified(qualifier, id))
        };
        let (address, group_address) = (qualify(action.id()), qualify(action.group()));
        let heading = action.describe(number, by);
        Self {
            heading,
            action,
            from,
            address,
            group_address,
            disposition: Disposition::Unwanted,
        }
    }

    /// The remote this record's repository paths are read from, or `None` for
    /// one the leaf repository declared.
    pub(super) fn remote(&self) -> Option<&ItemId> {
        self.from.as_ref().map(|it| it.remote())
    }

    /// The inclusion's variable scope, or `run` for a leaf record.
    pub(super) fn scope<'a>(&'a self, run: &'a Rc<VarSet>) -> &'a Rc<VarSet> {
        self.from.as_ref().map_or(run, |it| it.scope())
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

/// The run's list, and what reports need to know about its assembly.
pub(crate) struct RunList {
    pub records: Vec<RunRecord>,
    /// Whether the target named a record in the list. An inclusion opened only
    /// to reach inside it does not count.
    pub(super) target_found: bool,
    /// IDs of inclusions whose manifest this run did not read: not requested,
    /// excluded, or not materialized. A qualified skip into one of them is not
    /// reported as matching nothing.
    pub unread_inclusions: Vec<ItemId>,
}
