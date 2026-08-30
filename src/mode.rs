//! Whether a run carries its work out or only says what it would do, and the
//! verbs it says it in.
//!
//! `guidance.md`, "Dry-run", is the design and names the helpers that read
//! [`RunMode`].

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

/// One act an action reports, in whichever tense the mode calls for.
///
/// Not `unchanged`: that is a state a destination is already in rather than an
/// act, so it reads the same in both modes and takes no "would".
#[derive(Debug, Clone, Copy)]
pub(crate) enum Verb {
    Link,
    Relink,
    Copy,
    Create,
    Remove,
    Keep,
}

impl Verb {
    /// The bare verb, for a sentence that names an act rather than reporting
    /// one: "no children to link in …".
    pub fn infinitive(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Relink => "relink",
            Self::Copy => "copy",
            Self::Create => "create",
            Self::Remove => "remove",
            Self::Keep => "keep",
        }
    }

    /// What an action did, or — under [`RunMode::DryRun`] — would do.
    pub fn say(self, mode: RunMode) -> String {
        match mode {
            RunMode::Perform => self.past().to_string(),
            RunMode::DryRun => format!("would {}", self.infinitive()),
        }
    }

    fn past(self) -> &'static str {
        match self {
            Self::Link => "linked",
            Self::Relink => "relinked",
            Self::Copy => "copied",
            Self::Create => "created",
            Self::Remove => "removed",
            Self::Keep => "kept",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verb_is_reported_in_the_tense_the_mode_calls_for() {
        assert_eq!(Verb::Link.say(RunMode::Perform), "linked");
        assert_eq!(Verb::Link.say(RunMode::DryRun), "would link");
        // The irregular ones, which is why `past` is a table rather than a
        // suffix.
        assert_eq!(Verb::Copy.say(RunMode::Perform), "copied");
        assert_eq!(Verb::Keep.say(RunMode::Perform), "kept");
    }
}
