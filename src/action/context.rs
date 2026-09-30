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
use crate::replace::{self, ConflictPolicy, ConflictResolver, ConflictSettings};
use crate::repo_path::RepoPath;

/// How a run treats what is already at its destinations.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DestinationOptions {
    /// What to do with an unmanaged node in the way.
    pub policy: ConflictPolicy,
    /// Whether to reinstall existing seed content.
    pub refresh_content: bool,
}

impl Default for DestinationOptions {
    fn default() -> Self {
        Self {
            policy: ConflictPolicy::Backup,
            refresh_content: false,
        }
    }
}

/// Anchored roots, execution mode, remote exclusions, conflict settings, and reporter for a
/// run.
pub(crate) struct RunContext<'a> {
    repository: RepositoryRoot,
    home: PathBuf,
    mode: RunMode,
    /// Remotes excluded by conditions, with the reason for each exclusion.
    excluded_remotes: BTreeMap<ItemId, Exclusion>,
    conflicts: ConflictSettings,
    refresh_content: bool,
    reporter: &'a Reporter,
}

impl<'a> RunContext<'a> {
    /// Anchor the resolved roots, once, for every action in a run.
    pub fn new(
        roots: &Roots,
        mode: RunMode,
        excluded_remotes: BTreeMap<ItemId, Exclusion>,
        destination_options: DestinationOptions,
        reporter: &'a Reporter,
    ) -> Result<Self, Error> {
        Ok(Self {
            repository: RepositoryRoot::at(&roots.batfiles_repo)?,
            home: paths::anchor(&roots.home)?,
            mode,
            excluded_remotes,
            conflicts: ConflictSettings::new(destination_options.policy, SystemTime::now()),
            refresh_content: destination_options.refresh_content,
            reporter,
        })
    }

    /// The condition excluding this remote, if any, regardless of tree presence.
    pub fn excluded_remote(&self, id: &ItemId) -> Option<&Exclusion> {
        self.excluded_remotes.get(id)
    }

    /// Resolve a validated source to an absolute path, preserving repository symlinks. `remote`
    /// identifies an included record's materialization, or is `None` for a leaf record. Only
    /// leaf sources may name their own remote.
    ///
    /// The final node must exist; broken symlinks count as present. Excluded or missing remote
    /// materializations are errors.
    pub fn source(&self, remote: Option<&ItemId>, source: &RepoPath) -> Result<PathBuf, Error> {
        let root = self.tree_root(remote.or(source.remote()))?;
        let resolved = if source.path().is_empty() {
            root
        } else {
            paths::normalize_lexically(&root.join(source.path()))
        };

        // Broken symlinks are valid link sources, so check the node without following it.
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

    /// The anchored leaf repository root, used to recognize managed symlinks.
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
    pub fn resolver(&self) -> ConflictResolver<'_> {
        ConflictResolver::new(&self.conflicts, self.mode, self.reporter)
    }

    /// Return a resolver that refuses unmanaged nodes at tool-owned destinations.
    pub fn tool_owned(&self) -> ConflictResolver<'_> {
        ConflictResolver::new(&replace::REFUSE_CONFLICTS, self.mode, self.reporter)
    }

    /// Whether `--refresh-content` requests reinstalling existing seed content.
    pub fn refresh_content(&self) -> bool {
        self.refresh_content
    }

    /// Ensure a destination directory exists and report changes. Return `false` if a conflict
    /// along the path was skipped.
    pub fn ensure_directory(&self, dir: &Path) -> Result<bool, Error> {
        let outcome = directory::ensure_directory(dir, &self.resolver())?;
        outcome.report_removals(self.mode, self.reporter);
        match outcome {
            DirectoryOutcome::Created { .. } => self.reporter.info(&format!(
                "{} {}",
                Verb::Create.for_mode(self.mode),
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

    #[test]
    fn a_destination_resolves_against_the_selected_home() {
        assert_eq!(dest_of("~/.zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of(".zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of("~"), home());
        assert_eq!(dest_of("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn a_destination_may_deliberately_leave_the_home() {
        assert_eq!(dest_of("~/../shared/rc"), PathBuf::from("/home/shared/rc"));
    }
}
