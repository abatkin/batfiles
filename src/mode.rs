//! Whether a run carries its work out or only says what it would do.

/// Whether an action does its work or describes it.
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

    /// Return whether action helpers may modify the filesystem.
    pub fn writes(self) -> bool {
        matches!(self, Self::Perform)
    }
}
