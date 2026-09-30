//! Settle an unmanaged node at a destination — back it up, discard it, skip
//! it, or refuse it — and put new content in its place without losing the old
//! node on the way.

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
pub(crate) enum Policy {
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
pub(crate) struct Conflicts {
    policy: Policy,
    stamp: String,
}

impl Conflicts {
    /// The policy for destinations an action installs to, stamping backups
    /// with `now`.
    pub fn new(policy: Policy, now: SystemTime) -> Self {
        Self {
            policy,
            stamp: utc_stamp(now),
        }
    }

    /// Whether every conflict is skipped, so work whose only use is to replace
    /// an occupied destination need not be done.
    pub fn skips(&self) -> bool {
        self.policy == Policy::Skip
    }
}

/// The policy for tool-owned destinations, which makes no backups.
pub(crate) static REFUSING: Conflicts = Conflicts {
    policy: Policy::Refuse,
    stamp: String::new(),
};

/// What to do about one conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resolution {
    /// Fail; the caller words the refusal.
    Refuse,
    /// Leave the node and install nothing. Already reported.
    Skip,
    /// Replace the node, keeping it as a backup or discarding it.
    Replace(Keep),
}

/// Whether a replaced node survives the replacement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keep {
    /// Renamed to a backup beside the destination, and left there.
    Backup,
    /// Renamed aside and removed once its replacement is in place.
    Discard,
}

/// A conflict policy with the mode and reporter that carry it out.
#[derive(Clone, Copy)]
pub(crate) struct Resolver<'a> {
    conflicts: &'a Conflicts,
    mode: RunMode,
    reporter: &'a Reporter,
}

impl<'a> Resolver<'a> {
    pub fn new(conflicts: &'a Conflicts, mode: RunMode, reporter: &'a Reporter) -> Self {
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

    pub fn conflicts(&self) -> &'a Conflicts {
        self.conflicts
    }

    /// Decide what to do about `dest`, which is `found` — a phrase completing
    /// "`dest` is …". A skip is reported here. Asking reads one line from
    /// standard input per question; a dry run never asks, and answers as the
    /// default would.
    pub fn resolve(&self, dest: &Path, found: &dyn fmt::Display) -> Result<Resolution, Error> {
        let resolution = match self.conflicts.policy {
            Policy::Refuse => Resolution::Refuse,
            Policy::Backup => Resolution::Replace(Keep::Backup),
            Policy::Skip => Resolution::Skip,
            Policy::Ask if !self.mode.writes() => Resolution::Replace(Keep::Backup),
            Policy::Ask => self.ask(dest, found)?,
        };
        if resolution == Resolution::Skip {
            self.reporter.info(&format!(
                "{} {}: it is {found}",
                Verb::Skip.say(self.mode),
                dest.display()
            ));
        }
        Ok(resolution)
    }

    /// Put one question about `dest` until it has an answer.
    fn ask(&self, dest: &Path, found: &dyn fmt::Display) -> Result<Resolution, Error> {
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
                // The question is still open on its line.
                eprintln!();
                return Err(Error::NoAnswer {
                    path: dest.to_path_buf(),
                });
            }
            match answer.trim().to_ascii_lowercase().as_str() {
                "" | "b" => return Ok(Resolution::Replace(Keep::Backup)),
                "o" => return Ok(Resolution::Replace(Keep::Discard)),
                "s" => return Ok(Resolution::Skip),
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
        keep: Keep,
        install: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        let aside = match keep {
            Keep::Backup => self.backup_path(dest)?,
            Keep::Discard => discard_path(dest)?,
        };
        let (mode, reporter) = (self.mode, self.reporter);
        if mode.writes() {
            set_aside(dest, &aside)?;
        }
        match keep {
            Keep::Backup => reporter.info(&format!(
                "{} {} to {}",
                Verb::BackUp.say(mode),
                dest.display(),
                aside.display()
            )),
            Keep::Discard => {
                reporter.info(&format!("{} {}", Verb::Discard.say(mode), dest.display()));
            }
        }
        if !mode.writes() {
            return install();
        }

        if let Err(error) = install() {
            let (error, restored) = put_back(dest, &aside, error);
            if restored {
                reporter.info(&format!("{} {}", Verb::Restore.say(mode), dest.display()));
            }
            return Err(error);
        }
        if keep == Keep::Discard {
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

/// Where a node waits to be discarded, which must be vacant: what is there is
/// left over from another run, and nobody's to remove.
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

/// After `error` failed an install over `dest`, rename the node [`set_aside`]
/// moved back from `aside` if nothing is at `dest`. Answers the error to
/// report, which names `aside` where the node could not go back, and whether
/// it went back.
pub(crate) fn put_back(dest: &Path, aside: &Path, error: Error) -> (Error, bool) {
    let vacant = matches!(paths::node_at(dest), Ok(None));
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
    // Before 1970 is a clock nobody set, and any stamp will do.
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
        let conflicts = Conflicts {
            policy: Policy::Backup,
            stamp: "STAMP".to_owned(),
        };
        let reporter = Reporter::new(false);
        let resolver = Resolver::new(&conflicts, RunMode::Perform, &reporter);
        for contents in ["first", "second", "third"] {
            fs::write(&dest, contents).expect("a node to back up");
            resolver
                .replace(&dest, Keep::Backup, || Ok(()))
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
        let conflicts = Conflicts::new(Policy::Backup, SystemTime::now());
        let reporter = Reporter::new(false);
        let resolver = Resolver::new(&conflicts, RunMode::Perform, &reporter);

        let failed = resolver.replace(&dest, Keep::Discard, || {
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
        let conflicts = Conflicts::new(Policy::Backup, SystemTime::now());
        let reporter = Reporter::new(false);
        let resolver = Resolver::new(&conflicts, RunMode::Perform, &reporter);

        let failed = resolver.replace(&dest, Keep::Backup, || {
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
