//! Create action destination directories and report changes.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{ExistingNode, reaches_nothing};
use crate::replace::{ConflictDecision, ConflictResolver};

/// What [`ensure_directory`] found, for a caller that reports what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectoryOutcome {
    /// A directory was already there. Nothing was written.
    AlreadyThere,
    /// The directory and any missing parents were created, or would be in a dry run.
    /// `removed_links` lists broken links cleared along the path; other conflicts have already
    /// been reported.
    Created { removed_links: Vec<BrokenLink> },
    /// The conflict policy skipped a non-directory node; the skip has been reported.
    Skipped,
}

impl DirectoryOutcome {
    /// Report what this cleared, in order, as ordinary progress. Touches no
    /// filesystem; `mode` sets the tense.
    pub fn report_removals(&self, mode: RunMode, reporter: &Reporter) {
        for link in self.removals() {
            reporter.info(&link.removal_note(mode));
        }
    }

    /// The links this cleared, for merging into an ancestor's outcome.
    fn removals(&self) -> &[BrokenLink] {
        match self {
            Self::AlreadyThere | Self::Skipped => &[],
            Self::Created { removed_links } => removed_links,
        }
    }
}

/// A broken symlink that was cleared so a directory could be made where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrokenLink {
    /// Path of the removed link, possibly at an ancestor of the requested directory.
    pub path: PathBuf,
    /// The removed link's target.
    pub target: PathBuf,
}

impl BrokenLink {
    /// Format a link removal message in the tense selected by `mode`.
    pub fn removal_note(&self, mode: RunMode) -> String {
        format!(
            "{} a broken symlink to {} to make {}",
            Verb::Remove.for_mode(mode),
            self.target.display(),
            self.path.display()
        )
    }
}

/// Ensure a directory and its parents exist, following symlinks to directories. Clear broken
/// links and resolve other conflicts using `resolver`. Dry runs return planned changes without
/// writing.
pub(crate) fn ensure_directory(
    dir: &Path,
    resolver: &ConflictResolver<'_>,
) -> Result<DirectoryOutcome, Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(DirectoryOutcome::AlreadyThere),
        Ok(existing) => replace_with_directory(dir, ExistingNode::of(&existing), resolver),
        Err(error) if reaches_nothing(&error) => make_directory(dir, resolver),
        Err(error) => Err(Error::Read {
            path: dir.to_path_buf(),
            source: error,
        }),
    }
}

/// Replace a non-directory node according to the conflict policy, unless skipped. Its parent
/// must already resolve to a directory.
fn replace_with_directory(
    dir: &Path,
    found: ExistingNode,
    resolver: &ConflictResolver<'_>,
) -> Result<DirectoryOutcome, Error> {
    match resolver.resolve(dir, &found)? {
        ConflictDecision::Refuse => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found,
        }),
        ConflictDecision::Skip => Ok(DirectoryOutcome::Skipped),
        ConflictDecision::Replace(keep) => {
            resolver.replace(dir, keep, || create(dir, resolver.mode()))?;
            Ok(DirectoryOutcome::Created {
                removed_links: Vec::new(),
            })
        }
    }
}

/// Make one directory and every missing ancestor, clearing a broken symlink at
/// any level that has to become one.
fn make_directory(dir: &Path, resolver: &ConflictResolver<'_>) -> Result<DirectoryOutcome, Error> {
    let mode = resolver.mode();
    let mut removed_links = match dir.parent() {
        // A single-component relative path has an empty parent; do not try to create it.
        Some(parent) if !parent.as_os_str().is_empty() => {
            match ensure_directory(parent, resolver)? {
                DirectoryOutcome::Skipped => return Ok(DirectoryOutcome::Skipped),
                outcome => outcome.removals().to_vec(),
            }
        }
        _ => Vec::new(),
    };

    if let Some(target) = broken_link_at(dir)? {
        removed_links.push(BrokenLink {
            path: dir.to_path_buf(),
            target,
        });
        if mode.writes() {
            remove_link(dir)?;
        }
    }
    create(dir, mode)?;
    Ok(DirectoryOutcome::Created { removed_links })
}

/// Create one directory whose parent is there, in perform mode.
fn create(dir: &Path, mode: RunMode) -> Result<(), Error> {
    if mode.writes() {
        fs::create_dir(dir).map_err(|source| Error::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

/// Read a final symlink target. The caller must first establish that following
/// the path fails with NotFound or NotADirectory.
fn broken_link_at(path: &Path) -> Result<Option<PathBuf>, Error> {
    if !fs::symlink_metadata(path).is_ok_and(|node| node.is_symlink()) {
        return Ok(None);
    }
    let written = fs::read_link(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(written))
}

/// Clear a link [`broken_link_at`] found, so the directory can be made where it
/// was.
fn remove_link(path: &Path) -> Result<(), Error> {
    fs::remove_file(path).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Ensure the destination's parent directories exist and report removed broken links. Return
/// `false` if a conflict was skipped.
pub(crate) fn create_parents(dest: &Path, resolver: &ConflictResolver<'_>) -> Result<bool, Error> {
    let outcome = parents(dest, resolver)?;
    outcome.report_removals(resolver.mode(), resolver.reporter());
    Ok(!matches!(outcome, DirectoryOutcome::Skipped))
}

/// Ensure the destination's parent directory exists.
fn parents(dest: &Path, resolver: &ConflictResolver<'_>) -> Result<DirectoryOutcome, Error> {
    match dest.parent() {
        // An empty parent means the destination is in the working directory.
        Some(parent) if !parent.as_os_str().is_empty() => ensure_directory(parent, resolver),
        // A path with no parent is a root, which is already there.
        _ => Ok(DirectoryOutcome::AlreadyThere),
    }
}
