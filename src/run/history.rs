//! `runs.toml`: the generation of each writing run, when each action and remote last changed,
//! and when each stateful `run` action last succeeded.
//! See [`docs/state.md`](../../docs/state.md#runstoml-run-action-history).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::filesystem::tomlfile;
use crate::manifest::item::{ItemAddress, ItemId};
use crate::output::Reporter;

/// A run's place in the order of writing runs. Each writing run takes one more than the last
/// one saved.
pub(crate) type Generation = u64;

/// The parsed document. Entries whose address or ID matches nothing current are kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct RunHistory {
    /// The generation of the last run that saved the document.
    generation: Generation,
    /// Actions by address.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    actions: BTreeMap<ItemAddress, ActionEntry>,
    /// Remotes by ID.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    remotes: BTreeMap<ItemId, RemoteEntry>,
}

/// One action's record. Every field is optional, and an unknown one is invalid.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub(crate) struct ActionEntry {
    /// When the command last exited 0. `None` means never.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_success: Option<Timestamp>,
    /// The generation of the command's last success. `None` means `after` treats it as never
    /// having succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success_generation: Option<Generation>,
    /// The generation of the last run in which the action changed something.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_changed: Option<Generation>,
}

/// One remote's record.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct RemoteEntry {
    /// The generation of the last run in which the remote was materialized anew.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    last_changed: Option<Generation>,
}

impl ActionEntry {
    /// How long ago the command last succeeded, as of `now`, or `None` if it never has. A success
    /// recorded in the future is age zero.
    pub fn age(&self, now: Timestamp) -> Option<Duration> {
        self.last_success
            .map(|at| Duration::try_from(now.duration_since(at)).unwrap_or(Duration::ZERO))
    }
}

/// The document as read: the current layout, or the earlier one, which keyed `run` actions
/// directly by address.
#[derive(Default, Deserialize)]
#[serde(try_from = "toml::Table")]
struct Stored(RunHistory);

/// One action in the earlier layout.
#[derive(Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
struct EarlierEntry {
    #[serde(default)]
    last_success: Option<Timestamp>,
    #[serde(default)]
    pending: bool,
}

impl TryFrom<toml::Table> for Stored {
    type Error = toml::de::Error;

    /// Read the current layout when `generation` is a top-level value. The earlier layout holds
    /// only tables, so a document without one is read as that.
    fn try_from(table: toml::Table) -> Result<Self, Self::Error> {
        let current = table
            .get("generation")
            .is_some_and(|value| !value.is_table());
        let document = toml::Value::Table(table);
        if current {
            return document.try_into().map(Self);
        }
        let earlier: BTreeMap<ItemAddress, EarlierEntry> = document.try_into()?;
        Ok(Self(RunHistory {
            generation: 0,
            actions: earlier
                .into_iter()
                .map(|(address, entry)| (address, entry.into()))
                .collect(),
            remotes: BTreeMap::new(),
        }))
    }
}

impl From<EarlierEntry> for ActionEntry {
    /// An earlier success counts as generation 0, before any change this layout records. An
    /// earlier pending firing keeps no generation, so `after` runs the command once more.
    fn from(earlier: EarlierEntry) -> Self {
        Self {
            last_success: earlier.last_success,
            success_generation: earlier.last_success.filter(|_| !earlier.pending).map(|_| 0),
            last_changed: None,
        }
    }
}

impl RunHistory {
    /// The document's file name; the state root decides its directory.
    pub const FILE_NAME: &'static str = "runs.toml";

    /// Load the document in either layout, treating a missing file as an empty one. A malformed
    /// one fails and is left alone.
    pub fn load(path: &Path) -> Result<Self, Error> {
        tomlfile::read_or_default::<Stored>(path).map(|stored| stored.0)
    }

    /// Rewrite the document in the current layout.
    pub fn save(&self, path: &Path) -> Result<(), Error> {
        tomlfile::write(path, self)
    }

    /// The record for `address`; an absent one never succeeded or changed.
    pub fn action(&self, address: &ItemAddress) -> ActionEntry {
        self.actions.get(address).cloned().unwrap_or_default()
    }

    /// The generation in which the remote `id` last changed, if it ever did.
    pub fn remote_changed(&self, id: &ItemId) -> Option<Generation> {
        self.remotes.get(id).and_then(|entry| entry.last_changed)
    }

    /// Record that `address` succeeded at `now`, in `generation`.
    pub fn record_success(
        &mut self,
        address: &ItemAddress,
        generation: Generation,
        now: Timestamp,
    ) {
        let entry = self.actions.entry(address.clone()).or_default();
        entry.last_success = Some(now);
        entry.success_generation = Some(generation);
    }

    /// Record that the action at `address` changed something in `generation`.
    pub fn record_action_change(&mut self, address: &ItemAddress, generation: Generation) {
        self.actions
            .entry(address.clone())
            .or_default()
            .last_changed = Some(generation);
    }

    /// Record that the remote `id` changed in `generation`.
    pub fn record_remote_change(&mut self, id: &ItemId, generation: Generation) {
        self.remotes.entry(id.clone()).or_default().last_changed = Some(generation);
    }
}

/// A run's `runs.toml`: read once before remotes are materialized, updated in memory as each
/// remote and action completes, and saved once when the run ends. A dry run records nothing and
/// writes nothing.
pub(crate) struct RecordedRuns {
    path: PathBuf,
    history: RunHistory,
    /// The generation this run records under.
    generation: Generation,
    /// Whether the run records anything: `false` for a dry run.
    records: bool,
    /// Whether the run recorded anything not yet saved.
    dirty: bool,
}

impl RecordedRuns {
    /// Read `runs.toml` under `state_dir` and take the next generation. A malformed document
    /// fails.
    pub fn load(state_dir: &Path, mode: RunMode) -> Result<Self, Error> {
        let path = state_dir.join(RunHistory::FILE_NAME);
        let history = RunHistory::load(&path)?;
        Ok(Self {
            path,
            generation: history.generation.saturating_add(1),
            history,
            records: mode.writes(),
            dirty: false,
        })
    }

    /// The record for `address`, as of the last change this run recorded.
    pub fn action(&self, address: &ItemAddress) -> ActionEntry {
        self.history.action(address)
    }

    /// The generation in which the action at `address` last changed, if it ever did.
    pub fn action_changed(&self, address: &ItemAddress) -> Option<Generation> {
        self.history.action(address).last_changed
    }

    /// The generation in which the remote `id` last changed, if it ever did.
    pub fn remote_changed(&self, id: &ItemId) -> Option<Generation> {
        self.history.remote_changed(id)
    }

    /// Record that `address` succeeded at `now`.
    pub fn record_success(&mut self, address: &ItemAddress, now: Timestamp) {
        if self.records {
            self.history.record_success(address, self.generation, now);
            self.dirty = true;
        }
    }

    /// Record that the action at `address` changed something.
    pub fn record_action_change(&mut self, address: &ItemAddress) {
        if self.records {
            self.history.record_action_change(address, self.generation);
            self.dirty = true;
        }
    }

    /// Record that the remote `id` changed.
    pub fn record_remote_change(&mut self, id: &ItemId) {
        if self.records {
            self.history.record_remote_change(id, self.generation);
            self.dirty = true;
        }
    }

    /// Save what the run recorded, if anything, warning if it cannot be written.
    pub fn save(&mut self, reporter: &Reporter) {
        if !self.dirty {
            return;
        }
        self.history.generation = self.generation;
        match self.history.save(&self.path) {
            Ok(()) => self.dirty = false,
            Err(error) => reporter.warn(&format!(
                "{error}; this run's changes and successes are not recorded, so commands \
                 waiting on them may not run, and others may run again"
            )),
        }
    }
}

#[cfg(test)]
mod tests;
