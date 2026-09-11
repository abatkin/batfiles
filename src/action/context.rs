//! Action roots, source and destination resolution, run mode, and reporting.

use std::path::{Path, PathBuf};

use crate::directory::{self, DirectoryOutcome};
use crate::error::Error;
use crate::location::Roots;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{self, RepositoryRoot};

/// Anchored repository and home roots, execution mode, and reporter for one run.
pub(crate) struct RunContext<'a> {
    repository: RepositoryRoot,
    home: PathBuf,
    mode: RunMode,
    reporter: &'a Reporter,
}

impl<'a> RunContext<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(roots: &Roots, mode: RunMode, reporter: &'a Reporter) -> Result<Self, Error> {
        Ok(Self {
            repository: RepositoryRoot::at(&roots.batfiles_dir)?,
            home: paths::anchor(&roots.home)?,
            mode,
            reporter,
        })
    }

    /// Resolve a validated repository-relative source to an absolute path.
    /// The final node must exist; a broken symlink counts as present. The returned
    /// path preserves repository symlinks rather than canonicalizing them.
    pub fn source(&self, source: &str) -> Result<PathBuf, Error> {
        let resolved = paths::normalize_lexically(&self.repository.path().join(source));

        // Presence, not reachability: a source that is itself a broken symlink
        // is there, and linking at it is what the repository asked for.
        if paths::occupied(&resolved)? {
            Ok(resolved)
        } else {
            Err(Error::SourceMissing { path: resolved })
        }
    }

    /// Resolve a source directory, following its final symlink.
    /// Fails if the source is missing, unreadable, or does not resolve to a directory.
    pub fn source_directory(&self, source_dir: &str) -> Result<PathBuf, Error> {
        let resolved = self.source(source_dir)?;
        if paths::reaches_directory(&resolved)? {
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
    pub fn repository(&self) -> &RepositoryRoot {
        &self.repository
    }

    pub fn reporter(&self) -> &Reporter {
        self.reporter
    }

    /// Whether this run performs its work or only says what it would do.
    pub fn mode(&self) -> RunMode {
        self.mode
    }

    /// Ensure a destination directory exists and report creation or link removal.
    pub fn ensure_directory(&self, dir: &Path) -> Result<(), Error> {
        let outcome = directory::ensure_directory(dir, self.mode)?;
        for link in outcome.removals() {
            self.reporter.info(&link.removal_note(self.mode));
        }
        match outcome {
            DirectoryOutcome::Created { .. } => self.reporter.info(&format!(
                "{} {}",
                Verb::Create.say(self.mode),
                dir.display()
            )),
            DirectoryOutcome::AlreadyThere => self
                .reporter
                .detail(1, &format!("unchanged {}", dir.display())),
        }
        Ok(())
    }
}

/// Resolve a validated destination against the selected home.
/// Accepts `~`, `~/...`, relative paths, and absolute paths; paths may leave home.
/// The caller must reject empty values and `~other` before calling.
fn destination(home: &Path, dest: &str) -> PathBuf {
    let path = match dest.strip_prefix('~') {
        Some(rest) => home.join(rest.trim_start_matches('/')),
        None => home.join(dest),
    };
    paths::normalize_lexically(&path)
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
