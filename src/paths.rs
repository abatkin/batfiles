//! How batfiles reasons about a path: how one is composed, and what is already
//! sitting at it.
//!
//! **Nothing here writes.** Every function answers a question, which is what
//! makes this module safe to call from either run mode without consulting one.
//! Making the directory when the answer is "nothing is there" belongs to
//! [`crate::directory`].
//!
//! **Paths are built lexically; existing nodes are classified physically.** A
//! `dest` means its components joined to the selected home, symlinked parents
//! followed as ordinary path resolution follows them. A symlink already sitting
//! at that path is the other question, and the operating system reads its target
//! from the directory the link is *physically* in — answer that one lexically
//! and a link pointing outside the repository looks like one batfiles owns.
//!
//! That distinction is rule 14, and it is what makes rule 13 — never destroy
//! what you did not create — decidable. Both are settled here, once, so that no
//! action restates either (`guidance.md`).

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// The repository an action installs from, in both forms it is needed in.
///
/// The **anchored** path is absolute and lexically clean, and is what gets
/// written into a symlink: it keeps the spelling the user chose. The
/// **canonical** path is what containment is decided against, because the paths
/// it is compared with have been resolved by the operating system. Holding both
/// in one value is what stops a caller crossing them — the mistake rule 14
/// exists to prevent, and one that is invisible on a machine whose home
/// contains no symlink.
#[derive(Debug)]
pub(crate) struct Repository {
    anchored: PathBuf,
    canonical: PathBuf,
}

impl Repository {
    /// Anchor and resolve a selected repository root.
    ///
    /// The root exists — its manifest has already been read — so resolution is
    /// expected to succeed; where the filesystem refuses anyway, containment
    /// falls back to the anchored form, which is what the tool compared against
    /// before this module existed.
    pub fn at(path: &Path) -> Result<Self, Error> {
        let anchored = anchor(path)?;
        let canonical = fs::canonicalize(&anchored).unwrap_or_else(|_| anchored.clone());
        Ok(Self {
            anchored,
            canonical,
        })
    }

    /// The root as it is written into a link.
    pub fn path(&self) -> &Path {
        &self.anchored
    }

    /// Whether a path the operating system resolved lands inside this
    /// repository.
    ///
    /// The argument must already be in resolved form — [`resolved`] or
    /// [`Occupancy::Replaceable`] produce one. Passing a lexically composed path
    /// is the mistake this module is about.
    pub fn contains(&self, resolved_path: &Path) -> bool {
        resolved_path.starts_with(&self.canonical)
    }
}

/// What a destination holds, judged the way the operating system reads it.
///
/// The final component is never followed: a symlink is a thing that is there,
/// not a window onto what it reaches. Its *target*, however, is resolved, which
/// is the whole point.
#[derive(Debug)]
pub(crate) enum Occupancy {
    /// Nothing is there. The action may create what it was asked to.
    Vacant,
    /// A symlink holding no content of its own, so replacing it destroys
    /// nothing. Either it resolves inside the repository — one batfiles would
    /// have made — or it resolves nowhere at all, in which case it is already
    /// broken and where it was meant to point decides nothing.
    Replaceable {
        /// The target as written, for saying what a repair replaced.
        written: PathBuf,
        /// Where it resolves, for deciding whether it is already right. A
        /// broken link has no such place, so this is where it *would* land.
        points_at: PathBuf,
    },
    /// Someone's data, whatever kind. Until the backup policy at 9.4 there is
    /// nothing to do with this but name it.
    Unmanaged(ExistingNode),
}

impl Occupancy {
    /// Inspect one destination.
    pub fn at(dest: &Path, repository: &Repository) -> Result<Self, Error> {
        match fs::symlink_metadata(dest) {
            Ok(existing) if existing.is_symlink() => {
                let written = fs::read_link(dest).map_err(|source| Error::Read {
                    path: dest.to_path_buf(),
                    source,
                })?;
                let points_at = target_of(dest, &written);
                // A link into the repository is batfiles' to repair, and a link
                // reaching nothing is anybody's: it holds no content and gives
                // access to none, so the data rule 13 protects is not there to
                // lose. Only a link that both leaves the repository and lands on
                // something is someone else's arrangement.
                Ok(if repository.contains(&points_at) || is_broken(dest) {
                    Self::Replaceable { written, points_at }
                } else {
                    Self::Unmanaged(ExistingNode::Link { written, points_at })
                })
            }
            Ok(existing) => Ok(Self::Unmanaged(ExistingNode::of(&existing))),
            // Vacant covers both ways of reaching nothing. A destination under
            // a component that is not a directory holds nothing either, and
            // saying so hands the refusal to whoever creates the parents, which
            // can name the offending component — where reporting the raw error
            // here would name only the path below it.
            Err(error) if reaches_nothing(&error) => Ok(Self::Vacant),
            Err(error) => Err(Error::Read {
                path: dest.to_path_buf(),
                source: error,
            }),
        }
    }
}

/// Whether a symlink reaches nothing at all.
///
/// Asked of the link rather than of the target that was read off it, so a chain
/// ending nowhere is broken too and not merely its last hop.
fn is_broken(link: &Path) -> bool {
    matches!(fs::metadata(link), Err(error) if reaches_nothing(&error))
}

/// Whether an error from resolving a path means nothing is at the far end.
///
/// `NotFound` is the ordinary answer. `NotADirectory` is the same answer
/// arrived at differently: a path resolving through a component that is not a
/// directory — a link to `<some-file>/child` — reaches nothing just as surely
/// as one naming something that was never there, and the kernel distinguishes
/// them only by which step it gave up on.
///
/// A symlink loop is deliberately not here. `FilesystemLoop` says resolution
/// never terminated, not that it ended nowhere, so the link is left classified
/// by where it points and refused: removing what it cannot explain is the thing
/// rule 13 exists to stop batfiles doing.
pub(crate) fn reaches_nothing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// What a refused destination turned out to hold, as the data half of
/// [`Error::DestinationExists`].
///
/// Which one it is decides nothing — all four are refused — but it is the
/// difference between a diagnostic someone can act on and one that only says
/// no. This is the crate's one hand-written `Display`: the symlink case renders
/// conditionally, which no `#[error]` attribute can express.
#[derive(Debug)]
pub(crate) enum ExistingNode {
    File,
    Directory,
    /// A symlink that leaves the repository and reaches something, named both
    /// as it is written and as it resolves: a relative target is read from the
    /// link's own directory, so the spelling alone does not say where it goes.
    ///
    /// A link reaching *nothing* is not here, because it is not refused —
    /// see [`Occupancy::Replaceable`].
    Link {
        written: PathBuf,
        points_at: PathBuf,
    },
    /// A socket, a fifo, a device — something batfiles has no idea how to give
    /// back, which is exactly why it will not take it.
    Other,
}

impl ExistingNode {
    /// Name what is sitting at a destination, so a refusal can say which kind
    /// it found rather than only that it found one.
    ///
    /// The symlink cases are not here: they need the link's target, which the
    /// caller has already read.
    pub fn of(existing: &fs::Metadata) -> Self {
        if existing.is_file() {
            Self::File
        } else if existing.is_dir() {
            Self::Directory
        } else {
            Self::Other
        }
    }
}

impl fmt::Display for ExistingNode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File => write!(f, "a regular file"),
            Self::Directory => write!(f, "a directory"),
            Self::Link { written, points_at } => {
                write!(f, "a symlink to {}", written.display())?;
                // An absolute target resolves to itself, and printing it twice
                // reads as though two paths were involved.
                if written != points_at {
                    write!(f, " ({})", points_at.display())?;
                }
                write!(f, ", which is outside the repository")
            }
            Self::Other => write!(f, "neither a regular file, a directory, nor a symlink"),
        }
    }
}

/// Whether anything at all is at a path, without following a final symlink.
///
/// The question a seed asks, and the only one it asks: an action that never
/// replaces anything does not need to know what it found. Telling the kinds
/// apart is [`Occupancy::at`]'s, for the actions that do replace.
pub(crate) fn occupied(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        // Both ways of reaching nothing, as in [`Occupancy::at`]: nothing is at a
        // path under a component that is not a directory either.
        Err(error) if reaches_nothing(&error) => Ok(false),
        Err(error) => Err(Error::Read {
            path: path.to_path_buf(),
            source: error,
        }),
    }
}

/// Whether a path reaches a directory, following a final symlink.
///
/// What a path *reaches*, where [`Occupancy::at`] asks what one *holds*.
pub(crate) fn reaches_directory(path: &Path) -> Result<bool, Error> {
    fs::metadata(path)
        .map(|found| found.is_dir())
        .map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })
}

/// Refuse a destination that lands inside the source it installs from.
///
/// An action that installs *into* the thing it installs *from* has no reading
/// worth honoring, and for a directory it does not simply fail: the destination
/// becomes a child of the source, enumerating the source finds it, and the
/// action works on what it is writing. `copy` descends until the filesystem
/// refuses a longer path, having written a deep tree into the repository on the
/// way; `symlink-dir` links the destination it just created into itself. Either
/// way the tool has written into the repository, which it otherwise never does.
///
/// Judged by where the two resolve rather than how they are spelled, since a
/// destination can reach the source by a route that does not look like it
/// (`guidance.md`, rule 14). Naturally a no-op for a file source: nothing can
/// land inside one.
pub(crate) fn refuse_destination_inside_source(source: &Path, dest: &Path) -> Result<(), Error> {
    let source = resolved(source);
    if will_resolve_to(dest).starts_with(&source) {
        return Err(Error::DestinationInsideSource {
            installed: source,
            dest: dest.to_path_buf(),
        });
    }
    Ok(())
}

/// Where a path *will* be once it exists, resolved as far as the filesystem can
/// say today.
///
/// The tense is the whole difference from [`resolved`], which answers for
/// something that is there now. A destination need not be there yet, so the
/// deepest ancestor that does exist is resolved and the rest is joined back on.
/// That is enough to compare a destination against a path the operating system
/// resolved, without pretending the leaf exists.
pub(crate) fn will_resolve_to(path: &Path) -> PathBuf {
    let mut trailing = Vec::new();
    let mut ancestor = path;
    loop {
        if let Ok(canonical) = fs::canonicalize(ancestor) {
            let mut result = canonical;
            result.extend(trailing.iter().rev());
            return result;
        }
        // Nothing on this path exists, so there is nothing to resolve against
        // and the lexical answer is the only one available.
        let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) else {
            return normalize(path);
        };
        trailing.push(name);
        ancestor = parent;
    }
}

/// Where a path *is*, in the form [`Repository::contains`] and
/// [`Occupancy::Replaceable`] compare against.
///
/// Used on an action's intended target so that "is this link already right?" is
/// asked in one space rather than across two. [`will_resolve_to`] is the same
/// question for a path that does not exist yet.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize(path))
}

/// Where an existing symlink points, as the operating system would read it.
///
/// The link's *own directory* is resolved first, because that is the directory
/// a relative target is read from — not the path by which the link was reached.
/// Where `~/bin` is a symlink to `~/.local/bin`, a link at `~/bin/tool` spelled
/// `../dotfiles/bin/tool` points into `~/.local/`, not into `~/`.
///
/// The target itself is resolved where it can be and cancelled textually where
/// it cannot: a link may reach nothing, and a broken one still has to be
/// classified rather than error out.
fn target_of(link: &Path, written: &Path) -> PathBuf {
    let directory = match link.parent() {
        Some(parent) => fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf()),
        None => PathBuf::new(),
    };
    let joined = if written.is_relative() {
        directory.join(written)
    } else {
        written.to_path_buf()
    };
    resolved(&joined)
}

/// The direct children of a directory, sorted by name.
///
/// Sorted because `read_dir` yields whatever order the filesystem holds, and an
/// action that reports its work in a different order on every machine is one
/// nobody can diff.
pub(crate) fn children_of(dir: &Path) -> Result<Vec<OsString>, Error> {
    let read = fs::read_dir(dir).map_err(|source| Error::Read {
        path: dir.to_path_buf(),
        source,
    })?;
    let mut names = Vec::new();
    for entry in read {
        let entry = entry.map_err(|source| Error::Read {
            path: dir.to_path_buf(),
            source,
        })?;
        names.push(entry.file_name());
    }
    names.sort();
    Ok(names)
}

/// Make a path absolute and lexically clean, so that what is compared,
/// reported, and written into a link does not depend on the working directory.
pub(crate) fn anchor(path: &Path) -> Result<PathBuf, Error> {
    let absolute =
        std::path::absolute(path).map_err(|source| Error::WorkingDirectory { source })?;
    Ok(normalize(&absolute))
}

/// Resolve `.` and `..` textually, without consulting the filesystem.
///
/// This is how a *path* is composed, and it stays that way: batfiles does not
/// canonicalize every component of a destination to prove where it ends up,
/// because a parent that is a symlink is something the user put there
/// deliberately and following it is ordinary path resolution. Classifying what
/// is already at that path is the separate question this module's other
/// functions answer.
pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match result.components().next_back() {
                // Only a named component can be cancelled. Above a root there is
                // nowhere to go, and after another `..` the pair is meaningful.
                Some(Component::Normal(_)) => {
                    result.pop();
                }
                Some(Component::RootDir | Component::Prefix(_)) => {}
                _ => result.push(".."),
            },
            named => result.push(named),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizing_cancels_only_what_it_can() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        // Nothing to cancel: the root has no parent, and a leading `..` in a
        // relative path is part of where it points.
        assert_eq!(normalize(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(normalize(Path::new("../../a")), PathBuf::from("../../a"));
    }

    #[test]
    fn a_target_is_read_from_the_directory_the_link_is_physically_in() {
        // Neither path exists, so this exercises the fallback: with nothing to
        // resolve, the answer is the lexical one, which is what the tool did
        // everywhere before this module.
        let link = Path::new("/home/user/.zshrc");
        assert_eq!(
            target_of(link, Path::new("../dotfiles/zshrc")),
            PathBuf::from("/home/dotfiles/zshrc")
        );
        // Textually inside `/repo`, and nowhere near it once resolved.
        assert_eq!(
            target_of(link, Path::new("/repo/../outside")),
            PathBuf::from("/outside")
        );
    }
}
