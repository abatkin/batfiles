//! Resolve paths and inspect destination occupancy without filesystem writes.
//! Keep lexical source containment separate from filesystem target resolution.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::error::Error;

/// One repository root in two forms: the anchored path written into links, and
/// the canonical path containment is judged against.
#[derive(Debug)]
pub(crate) struct RepositoryRoot {
    anchored: PathBuf,
    canonical: PathBuf,
}

impl RepositoryRoot {
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
    /// nothing: it resolves inside the repository, or it is broken.
    Replaceable {
        /// The target as written, for saying what a repair replaced.
        written: PathBuf,
        /// Where it resolves, for deciding whether it is already right. A
        /// broken link has no such place, so this is where it *would* land.
        points_at: PathBuf,
    },
    /// Someone's data, whatever kind: a conflict for the run's policy to
    /// settle, or a refusal where the destination is tool-owned.
    Unmanaged(ExistingNode),
}

impl Occupancy {
    /// Inspect one destination.
    pub fn at(dest: &Path, repository: &RepositoryRoot) -> Result<Self, Error> {
        let Some(existing) = node_at(dest)? else {
            return Ok(Self::Vacant);
        };
        if !existing.is_symlink() {
            return Ok(Self::Unmanaged(ExistingNode::of(&existing)));
        }
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

/// What a destination in the way turned out to hold: the data half of
/// [`Error::DestinationExists`], and what a conflict names.
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
    /// Classify what is at a destination, for a refusal to name.
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

/// The node at `path` itself, where one is. A final symlink is the link, and
/// where it points is not read; earlier components resolve as usual.
/// NotFound and NotADirectory mean nothing is there; other inspection errors
/// propagate.
pub(crate) fn node_at(path: &Path) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(node) => Ok(Some(node)),
        // Nothing is at a path under a component that is not a directory
        // either.
        Err(error) if reaches_nothing(&error) => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Whether anything is at `path`, as [`node_at`] judges it.
pub(crate) fn occupied(path: &Path) -> Result<bool, Error> {
    Ok(node_at(path)?.is_some())
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
    let source = canonicalize_or_normalize(source);
    if will_resolve_to(dest).starts_with(&source) {
        return Err(Error::DestinationInsideSource {
            installed: source,
            dest: dest.to_path_buf(),
        });
    }
    Ok(())
}

/// Refuse to set `dest` aside where resolving `source` passes through it,
/// which would take the source with it: `source` is at or inside `dest`, or
/// reaches its target by a link `dest` is, or is inside.
pub(crate) fn refuse_setting_aside_a_source(source: &Path, dest: &Path) -> Result<(), Error> {
    if passes_through(source, &located(dest))? {
        return Err(Error::SourceInsideDestination {
            installed: source.to_path_buf(),
            dest: dest.to_path_buf(),
        });
    }
    Ok(())
}

/// Where the node at `path` physically is: its parent resolved, and its own
/// name kept, so a final symlink is named rather than followed.
fn located(path: &Path) -> PathBuf {
    match (path.parent(), path.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            canonicalize_or_normalize(parent).join(name)
        }
        _ => normalize_lexically(path),
    }
}

/// How many symlinks a walk follows before it gives up, as the operating
/// system does for a loop.
const MAX_LINKS: usize = 40;

/// Whether resolving `path` component by component, following each symlink
/// the way the operating system does, visits the node physically at `node`,
/// as [`located`] names one. A loop, or a component nothing is at, ends the
/// walk with what it visited so far; a node that cannot be inspected or a
/// link that cannot be read is an error.
fn passes_through(path: &Path, node: &Path) -> Result<bool, Error> {
    let mut pending: Vec<PathBuf> = path
        .components()
        .rev()
        .map(|component| PathBuf::from(component.as_os_str()))
        .collect();
    let mut current = PathBuf::new();
    let mut followed = 0;
    while let Some(next) = pending.pop() {
        match next.components().next() {
            Some(Component::Prefix(_)) => current = next,
            Some(Component::RootDir) => {
                // A root keeps the prefix before it and nothing else.
                let prefix = current
                    .components()
                    .next()
                    .filter(|first| matches!(first, Component::Prefix(_)));
                current = prefix
                    .map(|first| PathBuf::from(first.as_os_str()))
                    .unwrap_or_default();
                current.push(Component::RootDir);
            }
            Some(Component::CurDir) | None => {}
            Some(Component::ParentDir) => {
                current.pop();
            }
            Some(Component::Normal(name)) => {
                let candidate = current.join(name);
                if candidate == node {
                    return Ok(true);
                }
                let Some(found) = node_at(&candidate)? else {
                    // Nothing is here, so nothing further along is either.
                    return Ok(false);
                };
                if !found.is_symlink() {
                    current = candidate;
                    continue;
                }
                followed += 1;
                if followed > MAX_LINKS {
                    return Ok(false);
                }
                let target = fs::read_link(&candidate).map_err(|source| Error::Read {
                    path: candidate.clone(),
                    source,
                })?;
                // Read from the link's directory, which `current` still is;
                // an absolute target resets it.
                pending.extend(
                    target
                        .components()
                        .rev()
                        .map(|component| PathBuf::from(component.as_os_str())),
                );
            }
        }
    }
    Ok(false)
}

/// Where a path that does not exist yet would land: the nearest existing
/// ancestor canonicalized, with the missing components appended.
///
/// Best effort: where no ancestor canonicalizes, the answer is lexical, which a
/// symlink along the path can make wrong. Use it to predict what a run will do,
/// never to report where something is.
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
            return normalize_lexically(path);
        };
        trailing.push(name);
        ancestor = parent;
    }
}

/// The path as the filesystem reads it, or its lexical form where it cannot be
/// canonicalized — a path that is not there, or one whose links cannot be read.
///
/// The result does not say which form it is, and the lexical form resolves
/// `..` textually, unlike the filesystem through a symlink. Sound for comparing
/// two existing paths; best effort otherwise.
pub(crate) fn canonicalize_or_normalize(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| normalize_lexically(path))
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
    canonicalize_or_normalize(&joined)
}

/// A sibling of `dest` named for it: its file name with `suffix` appended.
pub(crate) fn beside(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    dest.with_file_name(name)
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
    Ok(normalize_lexically(&absolute))
}

/// Resolve `.` and `..` textually, without consulting the filesystem.
pub(crate) fn normalize_lexically(path: &Path) -> PathBuf {
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
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        // Nothing to cancel: the root has no parent, and a leading `..` in a
        // relative path is part of where it points.
        assert_eq!(normalize_lexically(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(
            normalize_lexically(Path::new("../../a")),
            PathBuf::from("../../a")
        );
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

    /// The node at `path`, where inspecting it is expected to succeed.
    fn node(path: &Path) -> Option<fs::Metadata> {
        node_at(path).expect("an inspectable path")
    }

    #[test]
    fn nothing_is_at_a_missing_path_or_one_under_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("file");
        fs::write(&file, "").expect("a file");

        assert!(node(&dir.path().join("missing")).is_none());
        assert!(node(&file.join("below")).is_none());
        assert!(node(&file).is_some_and(|found| found.is_file()));
    }

    #[cfg(unix)]
    #[test]
    fn a_source_inside_a_destination_is_judged_as_written_and_as_resolved() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).expect("a canonical root");
        let real = root.join("real");
        fs::create_dir_all(real.join("source")).expect("a source");
        let alias = root.join("alias");
        std::os::unix::fs::symlink(&real, &alias).expect("a link to it");
        let set_aside = |source: &Path, dest: &Path| {
            matches!(
                refuse_setting_aside_a_source(source, dest),
                Err(Error::SourceInsideDestination { .. })
            )
        };

        // As written: through the directory, or through the link.
        assert!(set_aside(&real.join("source"), &real));
        assert!(set_aside(&alias.join("source"), &alias));
        // As resolved: the directory holds it whatever spelling reached it.
        assert!(set_aside(&alias.join("source"), &real));
        // Moving the link aside leaves what it reached where it is.
        assert!(!set_aside(&real.join("source"), &alias));
        assert!(!set_aside(&real.join("source"), &root.join("elsewhere")));

        // A link inside the source's path, reached through another alias:
        // `alias/nested/source` walks through `real/nested`.
        std::os::unix::fs::symlink(".", real.join("nested")).expect("a nested link");
        assert!(set_aside(
            &alias.join("nested/source"),
            &real.join("nested")
        ));
        // A loop ends the walk rather than the process.
        std::os::unix::fs::symlink("loop", root.join("loop")).expect("a loop");
        assert!(!set_aside(&root.join("loop/source"), &real));
    }

    #[cfg(unix)]
    #[test]
    fn a_walk_that_cannot_inspect_a_component_is_an_error_rather_than_a_pass() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().expect("temp dir");
        let root = fs::canonicalize(dir.path()).expect("a canonical root");
        let locked = root.join("locked");
        fs::create_dir(&locked).expect("a directory");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).expect("locked");
        // Someone the mode does not bind, such as root, can inspect it anyway.
        let binds = fs::symlink_metadata(locked.join("source"))
            .is_err_and(|error| error.kind() == io::ErrorKind::PermissionDenied);

        let checked = refuse_setting_aside_a_source(&locked.join("source"), &root.join("dest"));

        fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).expect("unlocked");
        if binds {
            assert!(matches!(checked, Err(Error::Read { .. })), "{checked:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_broken_final_symlink_is_a_node_of_its_own() {
        let dir = tempfile::tempdir().expect("temp dir");
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path().join("nowhere"), &link).expect("a broken link");

        assert!(node(&link).is_some_and(|found| found.is_symlink()));
    }
}
