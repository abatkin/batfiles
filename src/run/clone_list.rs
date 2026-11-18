//! A clone list read for one run, with each entry decided against the run's selection.

use std::path::Path;

use crate::error::Error;
use crate::manifest::actions::GitCloneListAction;
use crate::manifest::clone_list::{self, CloneListEntry};
use crate::manifest::item::ItemAddress;
use crate::manifest::template::TemplateString;
use crate::run::selection::{Disposition, Subject};

/// A validated clone list with each entry decided. An empty list declares no repositories.
pub(crate) struct PreparedList<'a> {
    /// The action that declares the list and its destination directory.
    action: &'a GitCloneListAction,
    entries: Vec<PreparedEntry>,
}

/// A parsed clone-list entry, its address, and what the run does with it.
pub(crate) struct PreparedEntry {
    /// The parsed entry.
    pub declared: CloneListEntry,
    /// The list's address followed by the entry's `id`. `None` if either is absent.
    pub address: Option<ItemAddress>,
    /// Whether the entry is cloned, excluded, or not requested.
    pub disposition: Disposition,
}

impl<'a> PreparedList<'a> {
    /// Read and validate the clone list at `path` for `action`, whose record is at `list`,
    /// and decide each entry in list order with `decide`. Missing or malformed lists return
    /// an error.
    pub fn prepare(
        action: &'a GitCloneListAction,
        path: &Path,
        list: Option<&ItemAddress>,
        mut decide: impl FnMut(Subject<'_>) -> Disposition,
    ) -> Result<Self, Error> {
        let entries = clone_list::read(path)?
            .into_iter()
            .map(|declared| {
                let address = list
                    .zip(declared.id.as_ref())
                    .map(|(list, id)| list.child(id));
                let disposition = decide(Subject {
                    address: address.as_ref(),
                    group_address: None,
                    gate: declared.gate(),
                });
                PreparedEntry {
                    declared,
                    address,
                    disposition,
                }
            })
            .collect();
        Ok(Self { action, entries })
    }

    /// The directory the clones are made in, as the record wrote it.
    pub fn dest_dir(&self) -> &TemplateString {
        &self.action.dest_dir
    }

    /// The list source as declared, including any remote reference.
    pub fn name(&self) -> String {
        self.action.source.to_string()
    }

    /// Whether the list declares no repositories at all.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Every entry, in list order.
    pub fn entries(&self) -> &[PreparedEntry] {
        &self.entries
    }
}
