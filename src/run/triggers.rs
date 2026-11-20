//! Whether a `run` action's trigger fires, and the changes and successes `runs.toml` keeps for
//! it. See [triggers](../../docs/actions/run.md#triggers).

use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;

use super::history::{ActionEntry, Generation, RecordedRuns};
use super::record::RunRecord;
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::manifest::actions::RunAction;
use crate::manifest::item::{ItemAddress, ItemId};
use crate::manifest::trigger::{AfterTarget, Trigger};
use crate::output::Reporter;
use crate::vars::dynamic::age;

/// The run's triggers, and the `runs.toml` they read.
pub(super) struct Triggers<'a> {
    history: RecordedRuns,
    /// Whether the run is `clone` or `sync --bootstrap`.
    bootstrap: bool,
    /// The address `apply-action` names, whose trigger it waives.
    waived: Option<&'a ItemAddress>,
}

/// What a trigger decided.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    /// Run the command.
    Run,
    /// Leave the command alone, for this reason.
    Skip(String),
    /// A dry run's `after` action that nothing has made due: it runs if one of these changes.
    IfChanged(String),
}

impl<'a> Triggers<'a> {
    /// Read `runs.toml` under `state_dir`. A malformed document fails.
    pub fn new(
        state_dir: &Path,
        mode: RunMode,
        bootstrap: bool,
        waived: Option<&'a ItemAddress>,
    ) -> Result<Self, Error> {
        Ok(Self {
            history: RecordedRuns::load(state_dir, mode)?,
            bootstrap,
            waived,
        })
    }

    /// Record that the remote `id` changed.
    pub fn remote_changed(&mut self, id: &ItemId) {
        self.history.record_remote_change(id);
    }

    /// Record that the action at `address` changed something.
    pub fn changed(&mut self, address: &ItemAddress) {
        self.history.record_action_change(address);
    }

    /// Decide whether `record`, the `run` action `action`, runs its command now.
    pub fn decide(&self, record: &RunRecord, action: &RunAction, mode: RunMode) -> Verdict {
        let address = record.address.as_ref();
        if self.waived.is_some() && self.waived == address {
            return Verdict::Run;
        }
        let (entry, latest_change) = match address {
            Some(address) => (
                self.history.action(address),
                self.latest_change(address, &action.trigger),
            ),
            None => (ActionEntry::default(), None),
        };
        match (
            fire(
                &action.trigger,
                &entry,
                latest_change,
                self.bootstrap,
                Timestamp::now(),
            ),
            &action.trigger,
        ) {
            (Fires::Yes, _) => Verdict::Run,
            (Fires::No(_), Trigger::After(targets)) if !mode.writes() => {
                let targets: Vec<String> = targets.iter().map(|it| format!("`{it}`")).collect();
                Verdict::IfChanged(format!("would run if {} changes", targets.join(" or ")))
            }
            (Fires::No(reason), _) => Verdict::Skip(reason),
        }
    }

    /// The latest generation in which an `after` target of the action at `address` changed. An
    /// action target is the follower's sibling: declared in the same manifest, and qualified by
    /// the same inclusion.
    fn latest_change(&self, address: &ItemAddress, trigger: &Trigger) -> Option<Generation> {
        let Trigger::After(targets) = trigger else {
            return None;
        };
        targets
            .iter()
            .filter_map(|target| match target {
                AfterTarget::Action(id) => self.history.action_changed(&address.sibling(id)),
                AfterTarget::Remote(id) => self.history.remote_changed(id),
            })
            .max()
    }

    /// Record that `record`'s command succeeded, if its trigger keeps state.
    pub fn succeeded(&mut self, record: &RunRecord, action: &RunAction) {
        if let (Some(address), true) = (record.address.as_ref(), action.trigger.keeps_state()) {
            self.history.record_success(address, Timestamp::now());
        }
    }

    /// Save what the run recorded, warning if it cannot be written.
    pub fn save(&mut self, reporter: &Reporter) {
        self.history.save(reporter);
    }
}

/// Whether a trigger fires.
#[derive(Debug, PartialEq, Eq)]
enum Fires {
    Yes,
    /// No, for this reason.
    No(String),
}

/// Whether `trigger` fires, given the action's `runs.toml` `entry`, the latest generation in
/// which an `after` target changed, whether the run is a bootstrap, and the time `now`. An
/// `after` change in the generation of the last success came before it, in the same run.
fn fire(
    trigger: &Trigger,
    entry: &ActionEntry,
    latest_change: Option<Generation>,
    bootstrap: bool,
    now: Timestamp,
) -> Fires {
    let ran = |since: Duration| Fires::No(format!("ran {}", ago(since)));
    match trigger {
        Trigger::Always => Fires::Yes,
        Trigger::Bootstrap if bootstrap => Fires::Yes,
        Trigger::Bootstrap => Fires::No("not a bootstrap run".to_owned()),
        Trigger::Once => match entry.last_success {
            None => Fires::Yes,
            Some(_) => Fires::No("already ran".to_owned()),
        },
        Trigger::Every(interval) => match entry.age(now) {
            Some(since) if since < interval.get() => ran(since),
            _ => Fires::Yes,
        },
        Trigger::After(_) => match entry.success_generation {
            Some(succeeded) if latest_change.is_none_or(|changed| changed <= succeeded) => {
                Fires::No("nothing it follows changed".to_owned())
            }
            _ => Fires::Yes,
        },
    }
}

/// `3h ago`, or `just now`.
fn ago(since: Duration) -> String {
    let rendered = age(since);
    if since.as_secs() < 1 {
        rendered
    } else {
        format!("{rendered} ago")
    }
}

#[cfg(test)]
mod tests;
