//! `[default-disabled]`: what a fresh machine starts with switched off.

use serde::Deserialize;

use super::Invalid;
use crate::condition::Condition;
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
    /// Check what the records themselves say, which for now is that no entry
    /// writes both conditions.
    ///
    /// The conditions are parsed and checked here and evaluated nowhere: which
    /// candidates a fresh machine adopts is the bootstrap's question, and it
    /// has none of these entries to answer it with until 8.3. Checking the pair
    /// now is what keeps a candidate that could never mean anything on the
    /// machine that writes it rather than on the one that finally bootstraps.
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
        return Err(Invalid::BothConditionsOnCandidate {
            noun,
            number: index + 1,
        });
    }
    Ok(())
}

/// One `[[default-disabled.actions]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ActionEntry {
    /// The action to start out disabled.
    #[expect(dead_code, reason = "adopted at 8.3, by the bootstrap that reads it")]
    pub id: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}

/// One `[[default-disabled.groups]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GroupEntry {
    /// The group to start out disabled.
    #[expect(dead_code, reason = "adopted at 8.3, by the bootstrap that reads it")]
    pub group: ItemAddress,
    /// The condition under which the candidate is offered at all.
    pub when: Option<Condition>,
    /// The condition under which it is not.
    pub unless: Option<Condition>,
}
