//! Bootstrap candidates declared in `[default-disabled]`.

use serde::Deserialize;

use super::check::{ManifestError, RecordName};
use crate::condition::{Condition, Gate};
use crate::item::{ItemAddress, ItemKind};

/// Action and group candidates for bootstrap disabled state.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabled {
    #[serde(default)]
    pub actions: Vec<DisabledActionCandidate>,
    #[serde(default)]
    pub groups: Vec<DisabledGroupCandidate>,
}

impl DefaultDisabled {
    /// Reject entries declaring both `when` and `unless`.
    pub fn validate(&self) -> Result<(), ManifestError> {
        for (index, entry) in self.actions.iter().enumerate() {
            one_condition(&entry.when, &entry.unless, ItemKind::Action, index)?;
        }
        for (index, entry) in self.groups.iter().enumerate() {
            one_condition(&entry.when, &entry.unless, ItemKind::Group, index)?;
        }
        Ok(())
    }
}

/// Reject simultaneous conditions. `index` is zero-based; diagnostics use one-based positions.
fn one_condition(
    when: &Option<Condition>,
    unless: &Option<Condition>,
    kind: ItemKind,
    index: usize,
) -> Result<(), ManifestError> {
    if when.is_some() && unless.is_some() {
        return Err(ManifestError::BothConditions {
            record: RecordName::Candidate {
                kind,
                number: index + 1,
            },
        });
    }
    Ok(())
}

/// One `[[default-disabled.actions]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DisabledActionCandidate {
    /// The action to start out disabled.
    pub id: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}

impl DisabledActionCandidate {
    /// Return the candidate's condition gate. Call after [`DefaultDisabled::validate`] rejects
    /// simultaneous conditions.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::from_fields(self.when.as_ref(), self.unless.as_ref())
    }
}

/// One `[[default-disabled.groups]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DisabledGroupCandidate {
    /// The group to start out disabled.
    pub group: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}

impl DisabledGroupCandidate {
    /// Return the group candidate's condition gate. Call after [`DefaultDisabled::validate`]
    /// rejects simultaneous conditions.
    pub fn gate(&self) -> Option<Gate<'_>> {
        Gate::from_fields(self.when.as_ref(), self.unless.as_ref())
    }
}
