//! Whether a run carries its work out or only says what it would do.
//!
//! `guidance.md`, "Dry-run", is the design and names the helpers that read
//! [`RunMode`]. How a run *words* what it did is [`crate::output`]'s.

/// Whether an action does its work or describes it.
///
/// [`Self::DryRun`] promises that none of the plan is carried out. It does not
/// promise that the process writes nothing anywhere: batfiles' own bookkeeping
/// runs in both modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RunMode {
    Perform,
    DryRun,
}

impl RunMode {
    /// The mode `--dry-run` selects.
    pub fn new(dry_run: bool) -> Self {
        if dry_run { Self::DryRun } else { Self::Perform }
    }

    /// Whether this run may write. Every helper that writes asks this, and
    /// nothing else asks it.
    pub fn writes(self) -> bool {
        matches!(self, Self::Perform)
    }
}
