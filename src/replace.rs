//! Resolve unmanaged destination conflicts by backing up, discarding, skipping, or refusing
//! existing nodes. Restore a moved node after installation failure when possible.

use std::fmt;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths;

/// What a run does with an unmanaged node where it installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictPolicy {
    /// Back it up beside itself and replace it: the default.
    Backup,
    /// Leave it and install nothing there: `--no-overwrite`.
    Skip,
    /// Ask, one conflict at a time: `--interactive`.
    Ask,
    /// Fail naming it. Only tool-owned destinations use this.
    Refuse,
}

/// A run's conflict policy, and the timestamp every backup it makes is named
/// with.
#[derive(Debug)]
pub(crate) struct ConflictSettings {
    policy: ConflictPolicy,
    stamp: String,
}

impl ConflictSettings {
    /// The policy for destinations an action installs to, stamping backups
    /// with `now`.
    pub fn new(policy: ConflictPolicy, now: SystemTime) -> Self {
        Self {
            policy,
            stamp: utc_stamp(now),
        }
    }

    /// Return whether the policy skips every occupied destination.
    pub fn skips(&self) -> bool {
        self.policy == ConflictPolicy::Skip
    }
}

/// The policy for tool-owned destinations, which makes no backups.
pub(crate) static REFUSE_CONFLICTS: ConflictSettings = ConflictSettings {
    policy: ConflictPolicy::Refuse,
    stamp: String::new(),
};

/// What to do about one conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConflictDecision {
    /// Fail; the caller words the refusal.
    Refuse,
    /// Leave the node and install nothing. Already reported.
    Skip,
    /// Replace the node, keeping it as a backup or discarding it.
    Replace(ExistingContent),
}

/// Whether a replaced node survives the replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExistingContent {
    /// Renamed to a backup beside the destination, and left there.
    BackUp,
    /// Renamed aside and removed once its replacement is in place.
    Discard,
}

/// A conflict policy with the mode and reporter that carry it out.
#[derive(Clone, Copy)]
pub(crate) struct ConflictResolver<'a> {
    conflicts: &'a ConflictSettings,
    mode: RunMode,
    reporter: &'a Reporter,
}

impl<'a> ConflictResolver<'a> {
    pub fn new(conflicts: &'a ConflictSettings, mode: RunMode, reporter: &'a Reporter) -> Self {
        Self {
            conflicts,
            mode,
            reporter,
        }
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    pub fn reporter(&self) -> &'a Reporter {
        self.reporter
    }

    pub fn conflicts(&self) -> &'a ConflictSettings {
        self.conflicts
    }

    /// Resolve a conflict at `dest`. `found` describes its occupant, completing "`dest` is
    /// ...". Report skipped conflicts. Interactive mode reads stdin; dry runs use the default
    /// backup decision without prompting.
    pub fn resolve(
        &self,
        dest: &Path,
        found: &dyn fmt::Display,
    ) -> Result<ConflictDecision, Error> {
        let resolution = match self.conflicts.policy {
            ConflictPolicy::Refuse => ConflictDecision::Refuse,
            ConflictPolicy::Backup => ConflictDecision::Replace(ExistingContent::BackUp),
            ConflictPolicy::Skip => ConflictDecision::Skip,
            ConflictPolicy::Ask if !self.mode.writes() => {
                ConflictDecision::Replace(ExistingContent::BackUp)
            }
            ConflictPolicy::Ask => self.ask(dest, found)?,
        };
        if resolution == ConflictDecision::Skip {
            self.reporter.info(&format!(
                "{} {}: it is {found}",
                Verb::Skip.for_mode(self.mode),
                dest.display()
            ));
        }
        Ok(resolution)
    }

    /// Prompt for a conflict decision until a valid answer is read.
    fn ask(&self, dest: &Path, found: &dyn fmt::Display) -> Result<ConflictDecision, Error> {
        let stdin = io::stdin();
        let mut input = stdin.lock();
        let mut again = "";
        loop {
            self.reporter.prompt(&format!(
                "{again}{} is {found}: back up and replace (b), overwrite (o), or skip (s)? [b]",
                dest.display()
            ));
            let mut answer = String::new();
            let read = input
                .read_line(&mut answer)
                .map_err(|source| Error::Prompt {
                    path: dest.to_path_buf(),
                    source,
                })?;
            if read == 0 {
                // End the prompt line before printing the error.
                eprintln!();
                return Err(Error::NoAnswer {
                    path: dest.to_path_buf(),
                });
            }
            match answer.trim().to_ascii_lowercase().as_str() {
                "" | "b" => return Ok(ConflictDecision::Replace(ExistingContent::BackUp)),
                "o" => return Ok(ConflictDecision::Replace(ExistingContent::Discard)),
                "s" => return Ok(ConflictDecision::Skip),
                _ => again = "answer b, o, or s: ",
            }
        }
    }

    /// Replace the node at `dest` with whatever `install` puts there, keeping
    /// the old node as `keep` says.
    ///
    /// The node is renamed aside before `install` runs, and the rename is
    /// reported: a backup at `<dest>.batfiles-backup-<stamp>`, with `-2`, `-3`, …
    /// appended where that is taken; a discard at `<dest>.batfiles-old`, which
    /// must be vacant. If `install` fails and `dest` is vacant again, the node is
    /// renamed back and that is reported; otherwise the error names where it is.
    /// A discarded node is removed only after `install` succeeds.
    ///
    /// `install` runs in both modes and must gate its own writes; a dry run
    /// only reports the rename.
    pub fn replace(
        &self,
        dest: &Path,
        keep: ExistingContent,
        install: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let aside = match keep {
            ExistingContent::BackUp => self.backup_path(dest)?,
            ExistingContent::Discard => discard_path(dest)?,
        };
        let (mode, reporter) = (self.mode, self.reporter);
        if mode.writes() {
            set_aside(dest, &aside)?;
        }
        match keep {
            ExistingContent::BackUp => reporter.info(&format!(
                "{} {} to {}",
                Verb::BackUp.for_mode(mode),
                dest.display(),
                aside.display()
            )),
            ExistingContent::Discard => {
                reporter.info(&format!(
                    "{} {}",
                    Verb::Discard.for_mode(mode),
                    dest.display()
                ));
            }
        }
        if !mode.writes() {
            return install();
        }

        if let Err(error) = install() {
            let (error, restored) = put_back(dest, &aside, error);
            if restored {
                reporter.info(&format!(
                    "{} {}",
                    Verb::Restore.for_mode(mode),
                    dest.display()
                ));
            }
            return Err(error);
        }
        if keep == ExistingContent::Discard {
            remove_aside(&aside, reporter);
        }
        Ok(())
    }

    /// The first vacant backup path for `dest` under this run's stamp.
    fn backup_path(&self, dest: &Path) -> Result<PathBuf, Error> {
        let base = format!("{BACKUP_SUFFIX}{}", self.conflicts.stamp);
        let mut candidate = paths::beside(dest, &base);
        let mut attempt = 1;
        while paths::occupied(&candidate)? {
            attempt += 1;
            candidate = paths::beside(dest, &format!("{base}-{attempt}"));
        }
        Ok(candidate)
    }
}

/// What names a backup, ahead of its stamp.
const BACKUP_SUFFIX: &str = ".batfiles-backup-";

/// Return `<dest>.batfiles-old`, failing if it is already occupied.
fn discard_path(dest: &Path) -> Result<PathBuf, Error> {
    let aside = paths::beside(dest, ".batfiles-old");
    if paths::occupied(&aside)? {
        return Err(Error::StagingPathTaken { path: aside });
    }
    Ok(aside)
}

/// Rename the node at `dest` to `aside`, which the caller has found vacant.
pub(crate) fn set_aside(dest: &Path, aside: &Path) -> Result<(), Error> {
    fs::rename(dest, aside).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

/// Restore `aside` after an installation failure if `dest` is vacant. Return the error to
/// report and whether restoration succeeded; failed restoration adds the aside path to the
/// error.
pub(crate) fn put_back(dest: &Path, aside: &Path, error: Error) -> (Error, bool) {
    let vacant = matches!(paths::symlink_metadata_if_present(dest), Ok(None));
    if vacant && fs::rename(aside, dest).is_ok() {
        return (error, true);
    }
    let error = Error::SetAside {
        path: dest.to_path_buf(),
        aside: aside.to_path_buf(),
        source: Box::new(error),
    };
    (error, false)
}

/// Remove a node renamed aside to be discarded, warning if it cannot be.
pub(crate) fn remove_aside(aside: &Path, reporter: &Reporter) {
    let removed = match fs::symlink_metadata(aside) {
        Ok(found) if found.is_dir() => fs::remove_dir_all(aside),
        Ok(_) => fs::remove_file(aside),
        Err(error) => Err(error),
    };
    if let Err(error) = removed {
        reporter.warn(&format!(
            "could not remove the replaced content at {}: {error}",
            aside.display()
        ));
    }
}

/// `now` in UTC as `YYYYMMDDTHHMMSSZ`, to the second.
fn utc_stamp(now: SystemTime) -> String {
    // Clamp pre-epoch timestamps to the epoch.
    let seconds = now
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let (days, of_day) = (seconds / 86_400, seconds % 86_400);
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        of_day / 3600,
        of_day % 3600 / 60,
        of_day % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01, by Howard Hinnant's
/// `civil_from_days`.
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let shifted = days + 719_468;
    let era = shifted / 146_097;
    let of_era = shifted % 146_097;
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let day_of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn stamp_at(seconds: u64) -> String {
        utc_stamp(UNIX_EPOCH + Duration::from_secs(seconds))
    }

    #[test]
    fn a_stamp_is_the_utc_time_to_the_second() {
        assert_eq!(stamp_at(0), "19700101T000000Z");
        assert_eq!(stamp_at(951_782_400), "20000229T000000Z");
        assert_eq!(stamp_at(4_102_444_799), "20991231T235959Z");
    }

    #[test]
    fn a_backup_never_takes_an_earlier_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dest = dir.path().join("rc");
        let conflicts = ConflictSettings {
            policy: ConflictPolicy::Backup,
            stamp: "STAMP".to_owned(),
        };
        let reporter = Reporter::new(false);
        let resolver = ConflictResolver::new(&conflicts, RunMode::Perform, &reporter);
        for contents in ["first", "second", "third"] {
            fs::write(&dest, contents).expect("a node to back up");
            resolver
                .replace(&dest, ExistingContent::BackUp, || Ok(()))
                .expect("backed up");
        }
        let backup = |suffix: &str| fs::read_to_string(dir.path().join(format!("rc{suffix}")));
        assert_eq!(backup(".batfiles-backup-STAMP").expect("first"), "first");
        assert_eq!(
            backup(".batfiles-backup-STAMP-2").expect("second"),
            "second"
        );
        assert_eq!(backup(".batfiles-backup-STAMP-3").expect("third"), "third");
    }

    #[test]
    fn a_failed_install_puts_the_node_back() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dest = dir.path().join("rc");
        fs::write(&dest, "mine").expect("a node");
        let conflicts = ConflictSettings::new(ConflictPolicy::Backup, SystemTime::now());
        let reporter = Reporter::new(false);
        let resolver = ConflictResolver::new(&conflicts, RunMode::Perform, &reporter);

        let failed = resolver.replace(&dest, ExistingContent::Discard, || {
            Err(Error::Write {
                path: dest.clone(),
                source: io::ErrorKind::PermissionDenied.into(),
            })
        });
        assert!(matches!(failed, Err(Error::Write { .. })), "{failed:?}");
        assert_eq!(fs::read_to_string(&dest).expect("restored"), "mine");
        assert!(!paths::beside(&dest, ".batfiles-old").exists());
    }

    #[test]
    fn a_failed_install_that_left_something_names_where_the_node_went() {
        let dir = tempfile::tempdir().expect("temp dir");
        let dest = dir.path().join("rc");
        fs::write(&dest, "mine").expect("a node");
        let conflicts = ConflictSettings::new(ConflictPolicy::Backup, SystemTime::now());
        let reporter = Reporter::new(false);
        let resolver = ConflictResolver::new(&conflicts, RunMode::Perform, &reporter);

        let failed = resolver.replace(&dest, ExistingContent::BackUp, || {
            fs::write(&dest, "partial").expect("a partial install");
            Err(Error::Write {
                path: dest.clone(),
                source: io::ErrorKind::Other.into(),
            })
        });
        let Err(Error::SetAside { aside, .. }) = failed else {
            panic!("expected the error to name the backup: {failed:?}");
        };
        assert_eq!(fs::read_to_string(aside).expect("the backup"), "mine");
    }
}
