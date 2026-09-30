//! Making the directories an action needs, and saying what that took.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{ExistingNode, reaches_nothing};
use crate::replace::{Resolution, Resolver};

/// What [`ensure_directory`] found, for a caller that reports what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectoryOutcome {
    /// A directory was already there. Nothing was written.
    AlreadyThere,
    /// Nothing was there, and now the directory is — along with any missing
    /// parents, and minus any broken symlink that had to be cleared to make
    /// one. `replaced` is empty in the ordinary case. A node that was in the
    /// way and backed up or discarded has been reported already.
    Created { replaced: Vec<BrokenLink> },
    /// Something other than a directory is in the way, and the conflict policy
    /// skipped it, which has been reported. Nothing was written.
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
            Self::Created { replaced } => replaced,
        }
    }
}

/// A broken symlink that was cleared so a directory could be made where it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BrokenLink {
    /// Where the link was, which is not always a path the caller named: making
    /// a directory makes its missing ancestors too, and any of them can be the
    /// one that was in the way.
    pub path: PathBuf,
    /// The target it named, reported so the removal can be recognized rather
    /// than merely announced.
    pub target: PathBuf,
}

impl BrokenLink {
    /// The one sentence for a cleared link, wherever it was cleared, in the
    /// tense the mode calls for.
    pub fn removal_note(&self, mode: RunMode) -> String {
        format!(
            "{} a broken symlink to {} to make {}",
            Verb::Remove.say(mode),
            self.target.display(),
            self.path.display()
        )
    }
}

/// Ensure a directory and its parents exist, following links to directories.
/// Clear broken links along the path; settle any other node in the way under
/// `resolver`'s policy, whose refusal names it. Dry runs return intended
/// creations and removals without changing the filesystem.
pub(crate) fn ensure_directory(
    dir: &Path,
    resolver: &Resolver<'_>,
) -> Result<DirectoryOutcome, Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(DirectoryOutcome::AlreadyThere),
        Ok(existing) => replace_with_directory(dir, ExistingNode::of(&existing), resolver),
        // Nothing resolves here — either the path is empty or something on the
        // way to it is not a directory, and only making it will say which.
        Err(error) if reaches_nothing(&error) => make_directory(dir, resolver),
        Err(error) => Err(Error::Read {
            path: dir.to_path_buf(),
            source: error,
        }),
    }
}

/// Settle a node that is not a directory where one has to be, and make the
/// directory in its place unless that was skipped. Its parent is known to be a
/// directory, since the node resolved.
fn replace_with_directory(
    dir: &Path,
    found: ExistingNode,
    resolver: &Resolver<'_>,
) -> Result<DirectoryOutcome, Error> {
    match resolver.resolve(dir, &found)? {
        Resolution::Refuse => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found,
        }),
        Resolution::Skip => Ok(DirectoryOutcome::Skipped),
        Resolution::Replace(keep) => {
            resolver.replace(dir, keep, || create(dir, resolver.mode()))?;
            Ok(DirectoryOutcome::Created {
                replaced: Vec::new(),
            })
        }
    }
}

/// Make one directory and every missing ancestor, clearing a broken symlink at
/// any level that has to become one.
fn make_directory(dir: &Path, resolver: &Resolver<'_>) -> Result<DirectoryOutcome, Error> {
    let mode = resolver.mode();
    // The ancestors first, so this is only ever creating a directory whose
    // parent is known to be one.
    let mut replaced = match dir.parent() {
        // An empty parent is what a one-component relative path has, and it is
        // not a directory anything should try to make.
        Some(parent) if !parent.as_os_str().is_empty() => {
            match ensure_directory(parent, resolver)? {
                DirectoryOutcome::Skipped => return Ok(DirectoryOutcome::Skipped),
                outcome => outcome.removals().to_vec(),
            }
        }
        _ => Vec::new(),
    };

    if let Some(target) = broken_link_at(dir)? {
        replaced.push(BrokenLink {
            path: dir.to_path_buf(),
            target,
        });
        if mode.writes() {
            remove_link(dir)?;
        }
    }
    create(dir, mode)?;
    Ok(DirectoryOutcome::Created { replaced })
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

/// Create the directories a destination sits in, if they are not there, and
/// report any broken links that took clearing. Returns false where a conflict
/// along the way was skipped, and the destination cannot be installed.
pub(crate) fn create_parents(dest: &Path, resolver: &Resolver<'_>) -> Result<bool, Error> {
    let outcome = parents(dest, resolver)?;
    outcome.report_removals(resolver.mode(), resolver.reporter());
    Ok(!matches!(outcome, DirectoryOutcome::Skipped))
}

/// The directories a destination sits in, made as [`ensure_directory`] makes
/// one.
fn parents(dest: &Path, resolver: &Resolver<'_>) -> Result<DirectoryOutcome, Error> {
    match dest.parent() {
        // An empty parent is what a one-component relative path has; there is
        // no directory to make, and the destination is the working directory's.
        Some(parent) if !parent.as_os_str().is_empty() => ensure_directory(parent, resolver),
        // A path with no parent is a root, which is already there.
        _ => Ok(DirectoryOutcome::AlreadyThere),
    }
}
