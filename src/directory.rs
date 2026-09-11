//! Making the directories an action needs, and saying what that took.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::mode::RunMode;
use crate::output::Verb;
use crate::paths::{ExistingNode, reaches_nothing};

/// What [`ensure_directory`] found, for a caller that reports what it did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DirectoryOutcome {
    /// A directory was already there. Nothing was written.
    AlreadyThere,
    /// Nothing was there, and now the directory is — along with any missing
    /// parents, and minus any broken symlink that had to be cleared to make
    /// one. `replaced` is empty in the ordinary case.
    Created { replaced: Vec<BrokenLink> },
}

impl DirectoryOutcome {
    /// The links this cleared, which a caller has to say something about.
    pub fn removals(&self) -> &[BrokenLink] {
        match self {
            Self::AlreadyThere => &[],
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
/// Clear broken links along the path; refuse other occupied nodes. Dry runs
/// return intended creations and removals without changing the filesystem.
pub(crate) fn ensure_directory(dir: &Path, mode: RunMode) -> Result<DirectoryOutcome, Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(DirectoryOutcome::AlreadyThere),
        Ok(existing) => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found: ExistingNode::of(&existing),
        }),
        // Nothing resolves here — either the path is empty or something on the
        // way to it is not a directory, and only making it will say which.
        Err(error) if reaches_nothing(&error) => make_directory(dir, mode),
        Err(error) => Err(Error::Read {
            path: dir.to_path_buf(),
            source: error,
        }),
    }
}

/// Make one directory and every missing ancestor, clearing a broken symlink at
/// any level that has to become one.
fn make_directory(dir: &Path, mode: RunMode) -> Result<DirectoryOutcome, Error> {
    // The ancestors first, so this is only ever creating a directory whose
    // parent is known to be one.
    let mut replaced = match dir.parent() {
        // An empty parent is what a one-component relative path has, and it is
        // not a directory anything should try to make.
        Some(parent) if !parent.as_os_str().is_empty() => {
            ensure_directory(parent, mode)?.removals().to_vec()
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
    if mode.writes() {
        fs::create_dir(dir).map_err(|source| Error::Write {
            path: dir.to_path_buf(),
            source,
        })?;
    }
    Ok(DirectoryOutcome::Created { replaced })
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

/// Create the directories a destination sits in, if they are not there.
pub(crate) fn create_parents(dest: &Path, mode: RunMode) -> Result<DirectoryOutcome, Error> {
    match dest.parent() {
        // An empty parent is what a one-component relative path has; there is
        // no directory to make, and the destination is the working directory's.
        Some(parent) if !parent.as_os_str().is_empty() => ensure_directory(parent, mode),
        // A path with no parent is a root, which is already there.
        _ => Ok(DirectoryOutcome::AlreadyThere),
    }
}
