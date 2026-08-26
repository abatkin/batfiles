//! How batfiles reasons about a path: how one is composed, and what is already
//! sitting at it.
//!
//! Two rules meet here, which is why they share a module.
//!
//! Rule 13 — never destroy what you did not create — is one decision, and every
//! action that installs something has to make it. It is made here so that
//! `symlink` and `symlink-dir` reach the same answer today and the actions that
//! arrive at 1.2 and later reach it without restating the rule.
//!
//! Rule 14 is the reason that decision is not a lexical comparison. A *path* is
//! composed lexically, which is deliberate and specified: `~/.config/nvim` means
//! those components joined to the selected home, and a parent that is a symlink
//! is followed by ordinary path resolution rather than being resolved away. But
//! a symlink already sitting at that path is a different question: the operating
//! system reads its target from the directory the link is *physically* in, so
//! composing the answer lexically classifies it against a directory it is not
//! in. That is how a link pointing outside the repository comes to look like one
//! batfiles owns.
//!
//! So: paths are built lexically, and existing nodes are classified physically.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// The repository an action installs from, in both forms it is needed in.
///
/// Two forms because they answer different questions and are not
/// interchangeable. The **anchored** path is absolute and lexically clean, and
/// is what gets written into a symlink: it is the spelling the user chose, and
/// rewriting `~/dotfiles` as `/usr/home/you/dotfiles` because `/home` happens
/// to be a symlink would be a surprise in every `ls -l` from then on. The
/// **canonical** path is what containment is decided against, because the paths
/// it is compared with have been resolved by the operating system.
///
/// Holding both in one value is what stops the two being crossed. Comparing an
/// anchored root against a resolved path is precisely the mistake rule 14
/// exists to prevent, and it is invisible on any machine whose home contains no
/// symlink.
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
    /// [`Occupant::points_at`] produce one. Passing a lexically composed path
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
pub(crate) enum Occupant {
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

impl Occupant {
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
            Ok(existing) => Ok(Self::Unmanaged(kind_of(&existing))),
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
fn reaches_nothing(error: &io::Error) -> bool {
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
    /// see [`Occupant::Replaceable`].
    Link {
        written: PathBuf,
        points_at: PathBuf,
    },
    /// A socket, a fifo, a device — something batfiles has no idea how to give
    /// back, which is exactly why it will not take it.
    Other,
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
/// replaces anything does not need to know what it found — a file, a directory,
/// or a link, broken or not, all mean the same thing to it, and mean it
/// whoever put them there. Telling them apart is [`Occupant::at`]'s job, and
/// that exists because an action which *replaces* has to decide whether it may.
pub(crate) fn occupied(path: &Path) -> Result<bool, Error> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        // Both ways of reaching nothing, as in [`Occupant::at`]: nothing is at a
        // path under a component that is not a directory either.
        Err(error) if reaches_nothing(&error) => Ok(false),
        Err(error) => Err(Error::Read {
            path: path.to_path_buf(),
            source: error,
        }),
    }
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
    if intended(dest).starts_with(&source) {
        return Err(Error::DestinationInsideSource {
            installed: source,
            dest: dest.to_path_buf(),
        });
    }
    Ok(())
}

/// Where a path *will* be, resolved as far as the filesystem can say.
///
/// [`resolved`] answers this for something that is there. A destination need
/// not be there yet, so the deepest ancestor that does exist is resolved and
/// the rest is joined back on. That is enough to compare a destination against
/// a path the operating system resolved, without pretending the leaf exists.
pub(crate) fn intended(path: &Path) -> PathBuf {
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

/// A path in the form [`Repository::contains`] and [`Occupant::Owned`] compare
/// against.
///
/// Used on an action's intended target so that "is this link already right?" is
/// asked in one space rather than across two.
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

/// What [`ensure_directory`] found, for a caller that reports what it did.
///
/// The distinction is the whole output of a `create-dir` action, and it is the
/// difference between a run that changed the home and one that agreed with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Directory {
    /// A directory was already there. Nothing was written.
    AlreadyThere,
    /// Nothing was there, and now the directory is — along with any missing
    /// parents, and minus any broken symlink that had to be cleared to make
    /// one. `replaced` is empty in the ordinary case.
    Created { replaced: Vec<BrokenLink> },
}

impl Directory {
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
    /// The one sentence for a cleared link, wherever it was cleared.
    pub fn removal_note(&self) -> String {
        format!(
            "removed a broken symlink to {} to make {}",
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
/// A container rather than a destination, so unlike [`Occupant::at`] this
/// follows a final symlink: a home whose `~/.config` is a link onto another
/// volume is an ordinary arrangement, and the contents belong where it points.
/// Nothing is replaced either way, which is what makes following it safe.
/// What is already inside is left alone.
pub(crate) fn ensure_directory(dir: &Path) -> Result<Directory, Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(Directory::AlreadyThere),
        Ok(existing) => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found: kind_of(&existing),
        }),
        // Nothing resolves here — either the path is empty or something on the
        // way to it is not a directory, and only making it will say which.
        Err(error) if reaches_nothing(&error) => make_directory(dir),
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
fn make_directory(dir: &Path) -> Result<Directory, Error> {
    // The ancestors first, so this is only ever creating a directory whose
    // parent is known to be one.
    let mut replaced = match dir.parent() {
        // An empty parent is what a one-component relative path has, and it is
        // not a directory anything should try to make.
        Some(parent) if !parent.as_os_str().is_empty() => {
            ensure_directory(parent)?.removals().to_vec()
        }
        _ => Vec::new(),
    };

    if let Some(written) = broken_link_at(dir)? {
        replaced.push(BrokenLink {
            path: dir.to_path_buf(),
            written,
        });
    }
    fs::create_dir(dir).map_err(|source| Error::Write {
        path: dir.to_path_buf(),
        source,
    })?;
    Ok(Directory::Created { replaced })
}

/// Clear a broken symlink out of a path nothing resolves at, naming what it
/// held.
///
/// Only ever called where [`fs::metadata`] has already reported that nothing is
/// reachable, so a symlink found here is broken by construction and needs no
/// second question asked of it. `None` where the path is simply empty, which is
/// the ordinary case.
fn broken_link_at(path: &Path) -> Result<Option<PathBuf>, Error> {
    if !fs::symlink_metadata(path).is_ok_and(|node| node.is_symlink()) {
        return Ok(None);
    }
    let written = fs::read_link(path).map_err(|source| Error::Read {
        path: path.to_path_buf(),
        source,
    })?;
    fs::remove_file(path).map_err(|source| Error::Write {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(written))
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
/// reads as vacant ([`Occupant::at`]), and this is the step that can say which
/// component is the problem rather than reporting the path below it.
///
/// The outcome is returned rather than discarded because clearing a link removed
/// something, and a caller that says nothing about it would be destroying a node
/// silently — the one thing rule 13 is unwilling to do even for a node it is
/// willing to destroy.
pub(crate) fn create_parents(dest: &Path) -> Result<Directory, Error> {
    match dest.parent() {
        // An empty parent is what a one-component relative path has; there is
        // no directory to make, and the destination is the working directory's.
        Some(parent) if !parent.as_os_str().is_empty() => ensure_directory(parent),
        // A path with no parent is a root, which is already there.
        _ => Ok(Directory::AlreadyThere),
    }
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

/// Name what is sitting at a destination, so a refusal can say which kind it
/// found rather than only that it found one.
///
/// The symlink cases are not here: they need the link's target, which the
/// caller has already read.
fn kind_of(existing: &fs::Metadata) -> ExistingNode {
    if existing.is_file() {
        ExistingNode::File
    } else if existing.is_dir() {
        ExistingNode::Directory
    } else {
        ExistingNode::Other
    }
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
