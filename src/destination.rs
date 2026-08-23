//! What is already at a destination, and whether batfiles may replace it.
//!
//! Rule 13 — never destroy what you did not create — is one decision, and every
//! action that installs something has to make it. It is made here so that
//! `symlink` and `symlink-dir` reach the same answer today and the actions that
//! arrive at 1.2 and later reach it without restating the rule.
//!
//! Rule 14 is the reason this is not a lexical comparison. A destination *path*
//! is composed lexically, which is deliberate and specified: `~/.config/nvim`
//! means those components joined to the selected home, and a parent that is a
//! symlink is followed by ordinary path resolution rather than being resolved
//! away. But a symlink already sitting at that path is a different question:
//! the operating system reads its target from the directory the link is
//! *physically* in, so composing the answer lexically classifies it against a
//! directory it is not in. That is how a link pointing outside the repository
//! comes to look like one batfiles owns.
//!
//! So: paths are built lexically, and existing nodes are classified physically.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::{Error, ExistingNode};

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
/// anchored root against a resolved path is precisely the bug this module
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
    /// A symlink resolving inside the repository — one batfiles would have
    /// made. It holds no content of its own, so replacing it destroys nothing.
    Owned {
        /// The target as written, for saying what a repair replaced.
        written: PathBuf,
        /// Where it actually resolves, for deciding whether it is already
        /// right.
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
                Ok(if repository.contains(&points_at) {
                    Self::Owned { written, points_at }
                } else {
                    Self::Unmanaged(ExistingNode::Link { written, points_at })
                })
            }
            Ok(existing) => Ok(Self::Unmanaged(kind_of(&existing))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::Vacant),
            Err(error) => Err(Error::Read {
                path: dest.to_path_buf(),
                source: error,
            }),
        }
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
/// it cannot: a link may dangle, and a dangling one still has to be classified
/// rather than error out.
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

/// Make sure a destination *directory* is one, creating it where nothing is.
///
/// A container rather than a destination, so unlike [`Occupant::at`] this
/// follows a final symlink: a home whose `~/.config` is a link onto another
/// volume is an ordinary arrangement, and the contents belong where it points.
/// What is already inside is left alone.
pub(crate) fn ensure_directory(dir: &Path) -> Result<(), Error> {
    match fs::metadata(dir) {
        Ok(existing) if existing.is_dir() => Ok(()),
        Ok(existing) => Err(Error::DestinationExists {
            path: dir.to_path_buf(),
            found: kind_of(&existing),
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            // Nothing resolves here, but something may still be here: a
            // dangling symlink. `create_dir_all` would fail on it with a bare
            // `EEXIST` naming nothing, so it is identified rather than hit.
            //
            // Reported as its own kind rather than as an unowned link. Where a
            // dangling one points decides nothing — batfiles will not create
            // the far end of a link somebody else made, inside the repository
            // or out — so the refusal says the target is missing instead of
            // claiming it is somewhere.
            if fs::symlink_metadata(dir).is_ok_and(|node| node.is_symlink()) {
                let written = fs::read_link(dir).map_err(|source| Error::Read {
                    path: dir.to_path_buf(),
                    source,
                })?;
                return Err(Error::DestinationExists {
                    path: dir.to_path_buf(),
                    found: ExistingNode::DanglingLink { written },
                });
            }
            fs::create_dir_all(dir).map_err(|source| Error::Write {
                path: dir.to_path_buf(),
                source,
            })
        }
        Err(error) => Err(Error::Read {
            path: dir.to_path_buf(),
            source: error,
        }),
    }
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
