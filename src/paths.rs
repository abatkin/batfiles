//! Resolve paths and inspect destination occupancy without writing to the filesystem.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// The repository an action installs from, in both forms it is needed in.
#[derive(Debug)]
pub(crate) struct Repository {
    anchored: PathBuf,
    canonical: PathBuf,
}

impl Repository {
    /// Anchor and resolve a selected repository root.
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

    /// Test a resolved absolute path against the repository root.
    /// Callers must resolve the candidate before comparing it.
    pub fn contains(&self, resolved_path: &Path) -> bool {
        resolved_path.starts_with(&self.canonical)
    }
}

/// What a destination holds, judged the way the operating system reads it.
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
                Ok(if repository.contains(&points_at) || is_broken(dest) {
                    Self::Replaceable { written, points_at }
                } else {
                    Self::Unmanaged(ExistingNode::Link { written, points_at })
                })
            }
            Ok(existing) => Ok(Self::Unmanaged(ExistingNode::of(&existing))),
            Err(error) if reaches_nothing(&error) => Ok(Self::Vacant),
            Err(error) => Err(Error::Read {
                path: dest.to_path_buf(),
                source: error,
            }),
        }
    }
}

/// Whether a symlink reaches nothing at all.
fn is_broken(link: &Path) -> bool {
    matches!(fs::metadata(link), Err(error) if reaches_nothing(&error))
}

/// Whether an error from resolving a path means nothing is at the far end.
pub(crate) fn reaches_nothing(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
    )
}

/// What a refused destination turned out to hold, as the data half of
/// [`Error::DestinationExists`].
#[derive(Debug)]
pub(crate) enum ExistingNode {
    File,
    Directory,
    /// A symlink that leaves the repository and reaches something, named both
    /// as it is written and as it resolves: a relative target is read from the
    /// link's own directory, so the spelling alone does not say where it goes.
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

/// Test node presence without following a final symlink.
/// NotFound and NotADirectory mean vacant; other inspection errors propagate.
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
pub(crate) fn reaches_directory(path: &Path) -> Result<bool, Error> {
    fs::metadata(path)
        .map(|found| found.is_dir())
        .map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })
}

/// Refuse a destination that lands inside the source it installs from.
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

/// Canonicalize the nearest existing ancestor and append the missing components.
/// Falls back to lexical normalization if no ancestor can be canonicalized.
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

/// Canonicalize an existing path, falling back to lexical normalization on error.
pub(crate) fn resolved(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize(path))
}

/// Where an existing symlink points, as the operating system would read it.
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
