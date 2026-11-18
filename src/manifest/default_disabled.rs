//! Bootstrap candidates declared in `[default-disabled]`.

use serde::Deserialize;

use super::DeclarationName;
use super::check::{ManifestError, check_conditions};
use crate::manifest::condition::{Condition, Gate};
use crate::manifest::item::{ItemAddress, ItemKind};

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
    /// Reject entries declaring both `when` and `unless`, or a condition that does not parse.
    pub fn validate(&self) -> Result<(), ManifestError> {
        for (index, entry) in self.actions.iter().enumerate() {
            let record = candidate(ItemKind::Action, index);
            check_conditions(entry.when.as_ref(), entry.unless.as_ref(), &record)?;
        }
        for (index, entry) in self.groups.iter().enumerate() {
            let record = candidate(ItemKind::Group, index);
            check_conditions(entry.when.as_ref(), entry.unless.as_ref(), &record)?;
        }
        Ok(())
    }
}

/// The name of the zero-based `index`th candidate of `kind`, which diagnostics number from one.
fn candidate(kind: ItemKind, index: usize) -> DeclarationName {
    DeclarationName::Candidate {
        kind,
        number: index + 1,
    }
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
