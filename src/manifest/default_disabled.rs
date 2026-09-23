//! `[default-disabled]`: what a fresh machine starts with switched off.

use serde::Deserialize;

use super::check::{Invalid, RecordName};
use crate::condition::{Condition, Gate};
use crate::item::ItemAddress;

/// The two candidate lists.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabled {
    #[serde(default)]
    pub actions: Vec<ActionEntry>,
    #[serde(default)]
    pub groups: Vec<GroupEntry>,
}

impl DefaultDisabled {
    /// Check that no entry writes both conditions. Checked as the document is
    /// read, so the error surfaces on every machine, not only one being
    /// bootstrapped; [`crate::bootstrap`] evaluates the conditions.
    pub fn validate(&self) -> Result<(), Invalid> {
        for (index, entry) in self.actions.iter().enumerate() {
            one_condition(&entry.when, &entry.unless, "action", index)?;
        }
        for (index, entry) in self.groups.iter().enumerate() {
            one_condition(&entry.when, &entry.unless, "group", index)?;
        }
        Ok(())
    }
}

/// Refuse an entry writing both spellings, named by its one-based position
/// within its own array.
fn one_condition(
    when: &Option<Condition>,
    unless: &Option<Condition>,
    noun: &'static str,
    index: usize,
) -> Result<(), Invalid> {
    if when.is_some() && unless.is_some() {
        return Err(Invalid::BothConditions {
            record: RecordName::Candidate {
                noun,
                number: index + 1,
            },
        });
    }
    Ok(())
}

/// One `[[default-disabled.actions]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ActionEntry {
    /// The action to start out disabled.
    pub id: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}

impl ActionEntry {
    /// The condition deciding whether this machine is offered the candidate, or
    /// `None` where the entry wrote neither spelling.
    /// [`DefaultDisabled::validate`] has established that it wrote at most one.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::declared(self.when.as_ref(), self.unless.as_ref())
    }
}

/// One `[[default-disabled.groups]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GroupEntry {
    /// The group to start out disabled.
    pub group: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}

impl GroupEntry {
    /// The same, for a group candidate.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::declared(self.when.as_ref(), self.unless.as_ref())
    }
}
