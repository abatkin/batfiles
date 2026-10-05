//! Run records with their addresses, report headings, dispositions, and included children.

use std::path::PathBuf;
use std::rc::Rc;

use crate::error::Error;
use crate::inclusion::Inclusion;
use crate::item::{ItemAddress, ItemId};
use crate::manifest::action::{Action, Contributor};
use crate::output::Reporter;
use crate::selection::{Disposition, Selection, Subject, Unread};
use crate::var_set::VarSet;

/// An action's declaration, addresses, report heading, and disposition for this run.
pub(crate) struct RunRecord {
    pub action: Action,
    /// The action address, qualified for included records. `None` if the record or its
    /// inclusion has no ID.
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
        // Unnamed inclusions have no qualifier; assigning unqualified addresses would collide
        // with leaf records.
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
            disposition: Disposition::NotRequested,
        }
    }

    /// The record as selection reads it.
    pub fn subject(&self) -> Subject<'_> {
        Subject {
            address: self.address.as_ref(),
            group_address: self.group_address.as_ref(),
            gate: self.action.gate(),
        }
    }
}

/// One record of the leaf manifest, in declaration order.
pub(super) enum LeafEntry {
    /// Any record but an inclusion.
    Action(RunRecord),
    /// An inclusion and its contributed records. `opened` is `None` if it was unrequested,
    /// excluded, or lacked a materialization.
    Inclusion {
        record: RunRecord,
        inclusion: Inclusion,
        opened: Option<OpenedInclusion>,
    },
}

/// What an admitted inclusion read out of its remote's manifest.
pub(super) struct OpenedInclusion {
    /// Variable scope for included records and their clone-list entries. Derived for the
    /// inclusion, or shared with the run when neither inclusion nor remote declares variables.
    pub scope: Rc<VarSet>,
    /// Every record the manifest declared, in its order, including those the
    /// inclusion's filters left out. Never an inclusion.
    pub records: Vec<RunRecord>,
}

/// The run's list, and what reports need to know about its assembly.
pub(crate) struct RunList {
    pub(super) entries: Vec<LeafEntry>,
    /// Whether the target named a record in the list. An inclusion opened only
    /// to reach inside it does not count.
    pub(super) target_found: bool,
}

impl RunList {
    /// Every record the run knows of in declaration order: each leaf record,
    /// followed, for an opened inclusion, by the records it contributed.
    pub fn records(&self) -> impl Iterator<Item = &RunRecord> {
        self.entries.iter().flat_map(|entry| {
            let (record, opened) = match entry {
                LeafEntry::Action(record) => (record, None),
                LeafEntry::Inclusion { record, opened, .. } => (record, opened.as_ref()),
            };
            std::iter::once(record).chain(opened.into_iter().flat_map(|it| &it.records))
        })
    }

    /// Iterate the addresses of unread inclusions and clone lists, with the reason each went
    /// unread. Addresses inside them cannot be classified as unmatched.
    pub fn unread(&self) -> impl Iterator<Item = (&ItemAddress, Unread<'_>)> {
        let inclusions = self.entries.iter().filter_map(|entry| match entry {
            LeafEntry::Inclusion {
                record,
                inclusion,
                opened: None,
            } => {
                let unread = match &record.disposition {
                    Disposition::NotRequested => Unread::NotRequested,
                    Disposition::Excluded(exclusion) => Unread::ExcludedInclusion(exclusion),
                    Disposition::Allowed | Disposition::AllowedInPart => {
                        Unread::NotMaterialized(inclusion.remote())
                    }
                };
                Some((record.address.as_ref()?, unread))
            }
            _ => None,
        });
        let lists = self.records().filter_map(|record| {
            if !matches!(record.action, Action::GitCloneList(_)) {
                return None;
            }
            let unread = match &record.disposition {
                Disposition::NotRequested => Unread::NotRequested,
                Disposition::Excluded(exclusion) => Unread::ExcludedList(exclusion),
                Disposition::Allowed | Disposition::AllowedInPart => return None,
            };
            Some((record.address.as_ref()?, unread))
        });
        inclusions.chain(lists)
    }

    /// Warn about every run-only skip in `selection` that names nothing listed
    /// here or among `entries`, the addresses of every prepared clone-list
    /// entry. See [`Selection::warn_unmatched`].
    pub fn warn_unmatched(
        &self,
        selection: &Selection<'_>,
        entries: &[&ItemAddress],
        reporter: &Reporter,
    ) {
        let names = |of: fn(&RunRecord) -> &Option<ItemAddress>| -> Vec<&ItemAddress> {
            self.records().filter_map(|it| of(it).as_ref()).collect()
        };
        let mut addresses = names(|it| &it.address);
        addresses.extend_from_slice(entries);
        let unread: Vec<&ItemAddress> = self.unread().map(|(address, _)| address).collect();
        selection.warn_unmatched(
            &addresses,
            &names(|it| &it.group_address),
            &unread,
            reporter,
        );
    }

    /// Return an error if the selection's target matched no record and no clone-list entry
    /// (`entry_found`), or `None` if it matched or selected everything. Diagnostics identify
    /// `manifest` or the unread inclusion or list the target is inside.
    pub fn unmatched_target_error(
        &self,
        selection: &Selection<'_>,
        manifest: PathBuf,
        entry_found: bool,
    ) -> Option<Error> {
        if self.target_found || entry_found {
            return None;
        }
        selection.unmatched_target_error(manifest, self.unread())
    }
}
