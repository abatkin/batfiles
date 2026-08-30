//! Making the directories an action needs, and saying what that took.
//!
//! The only place batfiles creates a directory. What a path *means*, and what is
//! already at one, is [`crate::paths`]'; this module is what happens when the
//! answer is "nothing, and something has to be".
//!
//! One of the helpers that reads [`RunMode`], so a dry run reports the
//! directories and the cleared links without making or clearing either.

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::Error;
use crate::mode::{RunMode, Verb};
use crate::paths::{kind_of, reaches_nothing};

/// What [`ensure_directory`] found, for a caller that reports what it did.
///
/// The distinction is the whole output of a `create-dir` action, and it is the
/// difference between a run that changed the home and one that agreed with it.
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
    ///
    /// Making a directory is unremarkable; removing something is not, at any
    /// verbosity. Empty for an outcome that removed nothing, which is nearly
    /// all of them.
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
    pub written: PathBuf,
}

impl BrokenLink {
    /// The one sentence for a cleared link, wherever it was cleared, in the
    /// tense the mode calls for.
    pub fn removal_note(&self, mode: RunMode) -> String {
        format!(
            "{} a broken symlink to {} to make {}",
            Verb::Remove.say(mode),
            self.written.display(),
            self.path.display()
        )
    }
}

/// Make sure a destination *directory* is one, creating it where nothing is.
///
/// `mkdir -p`, and deliberately: an existing directory satisfies it, a
/// non-directory refuses it, and missing parents come with it. The one
/// departure is a broken symlink in the way, where `mkdir -p` reports a bare
/// `EEXIST` naming nothing: that link reaches nothing, so it is removed and the
/// directory made in its place, and the caller is told what went.
///
/// A container rather than a destination, so unlike [`crate::paths::Occupancy::at`] this
/// follows a final symlink: a home whose `~/.config` is a link onto another
/// volume is an ordinary arrangement, and the contents belong where it points.
/// Nothing is replaced either way, which is what makes following it safe.
/// What is already inside is left alone.
///
/// Under [`RunMode::DryRun`] a [`DirectoryOutcome::Created`] describes the
/// directory that *would* have been made, and nothing is written.
pub(crate) fn ensure_directory(dir: &Path, mode: RunMode) -> Result<DirectoryOutcome, Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(DirectoryOutcome::AlreadyThere),
        Ok(existing) => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found: kind_of(&existing),
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
///
/// A level at a time, rather than one [`fs::create_dir_all`]. The bulk call
/// cannot be told about the broken links this is here to clear, and where one
/// is partway up it fails with an `EEXIST` reported against the path that was
/// asked for — a path which, being the one that does not exist, is the least
/// informative name the failure could carry. Walking down means each level is
/// asked the same question the named directory was, so a link is cleared
/// wherever on the way it turns up and a *file* in the way is named for what it
/// is rather than surfacing as a raw write failure.
///
/// Under [`RunMode::DryRun`] the walk still names the links it found in the way
/// and leaves them there, so a later inspection of the same path finds one
/// again.
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

    if let Some(written) = broken_link_at(dir)? {
        replaced.push(BrokenLink {
            path: dir.to_path_buf(),
            written,
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

/// Name the broken symlink sitting where a directory has to go.
///
/// Only ever called where [`fs::metadata`] has already reported that nothing is
/// reachable, so a symlink found here is broken by construction and needs no
/// second question asked of it. `None` where the path is simply empty, which is
/// the ordinary case.
///
/// Finding it is separate from [`remove_link`]: a dry run reports the link and
/// does not clear it.
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
///
/// These correspond to nothing in the repository — they exist only so the
/// destination can, so they take the platform default rather than any source's
/// permissions.
///
/// A parent is a container in exactly the sense [`ensure_directory`] means, so
/// it is one. Two things change by saying so. A broken symlink in the way is
/// cleared and reported, where a bare [`fs::create_dir_all`] fails on it with an
/// `EEXIST` that names nothing. And an ordinary *file* in the way is named for
/// what it is — the caller reached here because the destination under that file
/// reads as vacant ([`crate::paths::Occupancy::at`]), and this is the step that can say which
/// component is the problem rather than reporting the path below it.
///
/// The outcome is returned rather than discarded because clearing a link removed
/// something, and a caller that says nothing about it would be destroying a node
/// silently — the one thing rule 13 is unwilling to do even for a node it is
/// willing to destroy.
pub(crate) fn create_parents(dest: &Path, mode: RunMode) -> Result<DirectoryOutcome, Error> {
    match dest.parent() {
        // An empty parent is what a one-component relative path has; there is
        // no directory to make, and the destination is the working directory's.
        Some(parent) if !parent.as_os_str().is_empty() => ensure_directory(parent, mode),
        // A path with no parent is a root, which is already there.
        _ => Ok(DirectoryOutcome::AlreadyThere),
    }
}
