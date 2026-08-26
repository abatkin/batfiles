//! What an action is carried out against, and how it reads the two paths a
//! manifest record gives it.
//!
//! Resolving what a `source` and a `dest` mean happens here rather than inside
//! the records, so there is one place that decides. What a path means, and what
//! is already *at* one, is [`crate::paths`]': every action asks it the same
//! questions and they are not asked twice.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::location::Roots;
use crate::output::Reporter;
use crate::paths::{self, Directory, Repository};

/// What every action is carried out against: the two roots it can reach, and
/// somewhere to say what it did.
///
/// The roots are anchored once, when this is built, because a symlink stores
/// the target it is given: a relative one is read back relative to the link's
/// own directory rather than to wherever batfiles happened to be run, so a
/// relative `--batfiles-dir` would otherwise produce a link that points nowhere
/// and a next run that calls it correct.
///
/// Everything an action needs that is not in its own record reaches it through
/// here, which is what keeps a run's settings from being threaded past every
/// action individually: 2.1's dry-run flag and 9.4's `--refresh-content` are
/// both fields on this value rather than parameters on nine signatures.
pub(crate) struct Context<'a> {
    repository: Repository,
    home: PathBuf,
    reporter: &'a Reporter,
}

impl<'a> Context<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(roots: &Roots, reporter: &'a Reporter) -> Result<Self, Error> {
        Ok(Self {
            repository: Repository::at(&roots.batfiles_dir)?,
            home: paths::anchor(&roots.home)?,
            reporter,
        })
    }

    /// An action's `source`, resolved against the repository that declared it.
    ///
    /// The manifest has already settled what a source may say, so this expects
    /// one that is relative and lands strictly inside the repository, and
    /// checks only the rule needing a filesystem: the path has to exist. The
    /// repository is already anchored, so the result is too — this is a path
    /// batfiles writes into a link, not one it classifies, so it keeps the
    /// spelling the user chose.
    ///
    /// The one place a repository path is resolved, which 6.3 widens to take
    /// `@remote/path` (`guidance.md`, "Seams the late slices need").
    pub fn source(&self, source: &str) -> Result<PathBuf, Error> {
        let resolved = paths::normalize(&self.repository.path().join(source));

        // Presence, not reachability: a source that is itself a broken symlink
        // is there, and linking at it is what the repository asked for.
        match fs::symlink_metadata(&resolved) {
            Ok(_) => Ok(resolved),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(Error::SourceMissing { path: resolved })
            }
            Err(error) => Err(Error::Read {
                path: resolved,
                source: error,
            }),
        }
    }

    /// An action's `source-dir`, resolved and confirmed to be one.
    ///
    /// Followed, unlike a destination: a `source-dir` that is a symlink to a
    /// directory inside the repository is something the repository put there
    /// deliberately, and its children are what the action is asking for.
    pub fn source_directory(&self, source_dir: &str) -> Result<PathBuf, Error> {
        let resolved = self.source(source_dir)?;
        if fs::metadata(&resolved)
            .map_err(|source| Error::Read {
                path: resolved.clone(),
                source,
            })?
            .is_dir()
        {
            Ok(resolved)
        } else {
            Err(Error::SourceNotADirectory { path: resolved })
        }
    }

    /// An action's `dest`, resolved against this run's selected home.
    pub fn destination(&self, dest: &str) -> PathBuf {
        destination(&self.home, dest)
    }

    /// The repository an action installs from, for the one question that needs
    /// it: whether a symlink already at a destination points into it.
    pub fn repository(&self) -> &Repository {
        &self.repository
    }

    pub fn reporter(&self) -> &Reporter {
        self.reporter
    }

    /// [`paths::ensure_directory`], plus the report it has no reporter to make.
    ///
    /// Every caller says the same thing about the same directory, because they
    /// are all the same operation: `create-dir` asks for one outright, and the
    /// two directory-wide actions need one to install into. A directory that
    /// appeared in the home is worth a line either way.
    pub fn ensure_directory(&self, dir: &Path) -> Result<(), Error> {
        let outcome = paths::ensure_directory(dir)?;
        // Ahead of the line about the directory, and at normal verbosity rather
        // than at `-v`: each of these is a removal, and a broken link is still
        // one the user may have been meaning to fix.
        for link in outcome.removals() {
            self.reporter.info(&link.removal_note());
        }
        match outcome {
            Directory::Created { .. } => self.reporter.info(&format!("created {}", dir.display())),
            Directory::AlreadyThere => self
                .reporter
                .detail(1, &format!("unchanged {}", dir.display())),
        }
        Ok(())
    }
}

/// A `dest` resolved against a selected home.
///
/// `~` and a relative path both resolve from the selected home rather than from
/// an independently discovered one, and an absolute path is used as written.
/// None of this makes the home a boundary: a destination may deliberately point
/// outside it, and only `--home-dir` decides what "home" means.
///
/// Free rather than a method so that the rule can be tested against a home that
/// no filesystem has to have.
///
/// Infallible: the manifest has already refused an empty `dest` and a `~other`,
/// so what reaches this is `~`, `~/…`, or an ordinary path.
fn destination(home: &Path, dest: &str) -> PathBuf {
    let path = match dest.strip_prefix('~') {
        // `~` leaves nothing and `~/…` leaves a separator, so trimming covers
        // both without a second arm.
        Some(rest) => home.join(rest.trim_start_matches('/')),
        // `join` returns an absolute `dest` unchanged, which is the rule for
        // one, so the relative and absolute cases are the same line.
        None => home.join(dest),
    };
    paths::normalize(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    fn dest_of(dest: &str) -> PathBuf {
        destination(&home(), dest)
    }

    // What a `dest` may say is checked by `manifest`, and tested there. These
    // cover the other half: what an accepted one resolves to.

    #[test]
    fn a_destination_resolves_against_the_selected_home() {
        assert_eq!(dest_of("~/.zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of(".zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of("~"), home());
        assert_eq!(dest_of("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn a_destination_may_deliberately_leave_the_home() {
        // The home is a base, not a boundary. Someone linking into a sibling
        // directory is expressing intent, not making a mistake.
        assert_eq!(dest_of("~/../shared/rc"), PathBuf::from("/home/shared/rc"));
    }
}
