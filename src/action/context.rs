//! Action roots, source and destination resolution, run mode, and reporting.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::condition::Exclusion;
use crate::directory::{self, DirectoryOutcome};
use crate::error::Error;
use crate::item::ItemId;
use crate::location::Roots;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{self, RepositoryRoot};
use crate::remotes;
use crate::repo_path::RepoPath;

/// Anchored repository and home roots, execution mode, excluded remotes, and
/// reporter for one run.
pub(crate) struct RunContext<'a> {
    repository: RepositoryRoot,
    home: PathBuf,
    mode: RunMode,
    /// The declared remotes this machine's conditions close, and why, settled
    /// once for the run before any action asks.
    excluded_remotes: BTreeMap<ItemId, Exclusion>,
    reporter: &'a Reporter,
}

impl<'a> RunContext<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(
        roots: &Roots,
        mode: RunMode,
        excluded_remotes: BTreeMap<ItemId, Exclusion>,
        reporter: &'a Reporter,
    ) -> Result<Self, Error> {
        Ok(Self {
            repository: RepositoryRoot::at(&roots.batfiles_dir)?,
            home: paths::anchor(&roots.home)?,
            mode,
            excluded_remotes,
            reporter,
        })
    }

    /// Why this machine does not have the remote `id` names, or `None` where it
    /// is one this run materializes and reads.
    pub fn excluded_remote(&self, id: &ItemId) -> Option<&Exclusion> {
        self.excluded_remotes.get(id)
    }

    /// Resolve a validated repository path to an absolute one, against the
    /// declaring repository or against a declared remote's materialization.
    ///
    /// This is where the two trees stop being two: which one a path is read
    /// from is settled here, and every action downstream has an absolute path
    /// and no further questions.
    ///
    /// The final node must exist; a broken symlink counts as present. The
    /// returned path preserves repository symlinks rather than canonicalizing
    /// them.
    pub fn source(&self, source: &RepoPath) -> Result<PathBuf, Error> {
        let resolved =
            paths::normalize_lexically(&self.tree_root(source.remote())?.join(source.path()));

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
    pub fn source_directory(&self, source_dir: &RepoPath) -> Result<PathBuf, Error> {
        let resolved = self.source(source_dir)?;
        if paths::reaches_directory(&resolved)? {
            Ok(resolved)
        } else {
            Err(Error::SourceNotADirectory { path: resolved })
        }
    }

    /// The root a repository path is read from.
    ///
    /// A remote's is its materialization, which has to be on the machine
    /// already: nothing clones one on demand, so a path into a remote that
    /// `sync` has not brought down is reported as that rather than as a missing
    /// file under a directory the user never made.
    ///
    /// A remote this machine's conditions exclude is refused ahead of that, and
    /// whether or not a tree is there: a materialization an earlier run left
    /// behind is not content this one may install from.
    fn tree_root(&self, remote: Option<&ItemId>) -> Result<PathBuf, Error> {
        let Some(id) = remote else {
            return Ok(self.repository.path().to_path_buf());
        };
        if let Some(exclusion) = self.excluded_remote(id) {
            return Err(Error::RemoteExcluded {
                remote: id.clone(),
                reason: exclusion.reason().to_owned(),
            });
        }
        let path = self.materialization(id);
        if paths::occupied(&path)? {
            Ok(path)
        } else {
            Err(Error::RemoteNotMaterialized {
                remote: id.clone(),
                path,
            })
        }
    }

    /// An action's `dest`, resolved against this run's selected home.
    pub fn destination(&self, dest: &str) -> PathBuf {
        destination(&self.home, dest)
    }

    /// Where one declared remote is materialized: `remotes/<id>` inside the leaf
    /// repository, keyed by the ID the remote was declared under rather than by
    /// anything that reaches it.
    ///
    /// A repository-owned path, so it is resolved here with the rest of them.
    /// An ID is a path segment the manifest already validated, so nothing here
    /// can leave the tree.
    pub fn materialization(&self, id: &ItemId) -> PathBuf {
        self.repository
            .path()
            .join(remotes::DIRECTORY)
            .join(id.as_str())
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
