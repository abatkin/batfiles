//! `[default-disabled]`: what a fresh machine starts with switched off.

use serde::Deserialize;

use crate::item::ItemAddress;

/// The two candidate lists.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct DefaultDisabled {
    #[expect(
        dead_code,
        reason = "walked at 5.6, to reject an entry setting both conditions"
    )]
    #[serde(default)]
    pub actions: Vec<ActionEntry>,
    #[expect(
        dead_code,
        reason = "walked at 5.6, to reject an entry setting both conditions"
    )]
    #[serde(default)]
    pub groups: Vec<GroupEntry>,
}

/// One `[[default-disabled.actions]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ActionEntry {
    /// The action to start out disabled.
    #[expect(dead_code, reason = "adopted at 8.3, by the bootstrap that reads it")]
    pub id: ItemAddress,
}

/// One `[[default-disabled.groups]]` entry.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct GroupEntry {
    /// The group to start out disabled.
    #[expect(dead_code, reason = "adopted at 8.3, by the bootstrap that reads it")]
    pub group: ItemAddress,
}
