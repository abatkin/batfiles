//! Action roots, source and destination resolution, run mode, and reporting.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::condition::Exclusion;
use crate::directory::{self, DirectoryOutcome};
use crate::error::Error;
use crate::item::ItemId;
use crate::location::Roots;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{self, RepositoryRoot};
use crate::remotes;
use crate::replace::{self, Conflicts, Policy, Resolver};
use crate::repo_path::RepoPath;

/// How a run treats what is already at its destinations.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Replacement {
    /// What to do with an unmanaged node in the way.
    pub policy: Policy,
    /// Whether seeds are installed again over what is already there.
    pub refresh_content: bool,
}

impl Default for Replacement {
    fn default() -> Self {
        Self {
            policy: Policy::Backup,
            refresh_content: false,
        }
    }
}

/// Anchored repository and home roots, execution mode, excluded remotes,
/// conflict policy, and reporter for one run.
///
/// Everything here is settled before the first action and read by every one of
/// them. Nothing an action does changes it.
pub(crate) struct RunContext<'a> {
    repository: RepositoryRoot,
    home: PathBuf,
    mode: RunMode,
    /// The declared remotes this machine's conditions close, and why, settled
    /// once for the run before any action asks.
    excluded_remotes: BTreeMap<ItemId, Exclusion>,
    conflicts: Conflicts,
    refresh_content: bool,
    reporter: &'a Reporter,
}

impl<'a> RunContext<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(
        roots: &Roots,
        mode: RunMode,
        excluded_remotes: BTreeMap<ItemId, Exclusion>,
        replacement: Replacement,
        reporter: &'a Reporter,
    ) -> Result<Self, Error> {
        Ok(Self {
            repository: RepositoryRoot::at(&roots.batfiles_dir)?,
            home: paths::anchor(&roots.home)?,
            mode,
            excluded_remotes,
            conflicts: Conflicts::new(replacement.policy, SystemTime::now()),
            refresh_content: replacement.refresh_content,
            reporter,
        })
    }

    /// The condition excluding this remote, if any, regardless of tree presence.
    pub fn excluded_remote(&self, id: &ItemId) -> Option<&Exclusion> {
        self.excluded_remotes.get(id)
    }

    /// Resolve a validated source against the tree it is read from.
    ///
    /// `remote` is the materialization an
    /// [inclusion](crate::manifest::action::IncludeRemoteAction)'s record came
    /// from, or `None` for a leaf record. It never conflicts with a remote named
    /// in `source`, which only a leaf action may write.
    ///
    /// The final node must exist; broken symlinks count as present.
    /// Returns an absolute path preserving repository symlinks.
    /// Excluded or missing remote materializations are errors.
    pub fn source(&self, remote: Option<&ItemId>, source: &RepoPath) -> Result<PathBuf, Error> {
        let root = self.tree_root(remote.or(source.remote()))?;
        // A file remote is named with no path, and is its materialization.
        let resolved = if source.path().is_empty() {
            root
        } else {
            paths::normalize_lexically(&root.join(source.path()))
        };

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
    pub fn source_directory(
        &self,
        remote: Option<&ItemId>,
        source_dir: &RepoPath,
    ) -> Result<PathBuf, Error> {
        let resolved = self.source(remote, source_dir)?;
        if paths::reaches_directory(&resolved)? {
            Ok(resolved)
        } else {
            Err(Error::SourceNotADirectory { path: resolved })
        }
    }

    /// Return the declaring repository root or an existing remote materialization.
    /// Refuse excluded remotes before checking whether their trees exist.
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

    /// Where a declared remote is materialized in this run's repository; see
    /// [`remotes::materialization`].
    pub fn materialization(&self, id: &ItemId) -> PathBuf {
        remotes::materialization(self.repository.path(), id)
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

    /// The run's conflict policy, for a destination an action installs to.
    pub fn resolver(&self) -> Resolver<'_> {
        Resolver::new(&self.conflicts, self.mode, self.reporter)
    }

    /// The policy for a destination batfiles owns, such as a remote's
    /// materialization: an unmanaged node there is refused, whatever the run
    /// was asked to do with the user's.
    pub fn tool_owned(&self) -> Resolver<'_> {
        Resolver::new(&replace::REFUSING, self.mode, self.reporter)
    }

    /// Whether `--refresh-content` asks seeds to be installed again over what
    /// is already there.
    pub fn refresh_content(&self) -> bool {
        self.refresh_content
    }

    /// Ensure a destination directory exists and report creation or link
    /// removal. Returns false where a conflict along the path was skipped, and
    /// nothing is there to install into.
    pub fn ensure_directory(&self, dir: &Path) -> Result<bool, Error> {
        let outcome = directory::ensure_directory(dir, &self.resolver())?;
        outcome.report_removals(self.mode, self.reporter);
        match outcome {
            DirectoryOutcome::Created { .. } => self.reporter.info(&format!(
                "{} {}",
                Verb::Create.say(self.mode),
                dir.display()
            )),
            DirectoryOutcome::AlreadyThere => self
                .reporter
                .detail(1, &format!("unchanged {}", dir.display())),
            DirectoryOutcome::Skipped => return Ok(false),
        }
        Ok(true)
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
