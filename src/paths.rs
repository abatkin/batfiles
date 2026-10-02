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
        let canonical = canonicalize(&anchored).unwrap_or_else(|_| anchored.clone());
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
    /// A broken symlink or one resolving inside the repository, replaceable without a backup.
    Replaceable {
        /// The target as written, for saying what a repair replaced.
        written: PathBuf,
        /// Resolved target path, or its lexical fallback for a broken link.
        points_at: PathBuf,
    },
    /// An unmanaged node requiring conflict resolution.
    Unmanaged(ExistingNode),
}

impl Occupancy {
    /// Inspect one destination.
    pub fn at(dest: &Path, repository: &RepositoryRoot) -> Result<Self, Error> {
        let Some(existing) = symlink_metadata_if_present(dest)? else {
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

/// Filesystem node details used in conflict diagnostics.
#[derive(Debug)]
pub(crate) enum ExistingNode {
    File,
    Directory,
    /// An external symlink with its stored target and resolved path.
    Link {
        written: PathBuf,
        points_at: PathBuf,
    },
    /// An unsupported node kind, such as a socket, FIFO, or device.
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
                // Show the resolved path only when it differs from the stored target.
                if written != points_at {
                    write!(f, " ({})", points_at.display())?;
                }
                write!(f, ", which is outside the repository")
            }
            Self::Other => write!(f, "neither a regular file, a directory, nor a symlink"),
        }
    }
}

/// Inspect `path` without following its final symlink; earlier components resolve normally.
/// Return `None` for NotFound or NotADirectory, and propagate other errors.
pub(crate) fn symlink_metadata_if_present(path: &Path) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(node) => Ok(Some(node)),
        Err(error) if reaches_nothing(&error) => Ok(None),
        Err(source) => Err(Error::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Whether anything is at `path`, as [`symlink_metadata_if_present`] judges it.
pub(crate) fn occupied(path: &Path) -> Result<bool, Error> {
    Ok(symlink_metadata_if_present(path)?.is_some())
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

/// Reject moving `dest` aside if resolving `source` visits it, including through symlinks.
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

/// Maximum number of symlinks followed during a path walk.
const MAX_LINKS: usize = 40;

/// Return whether resolving `path` visits `node`, which must use [`located`] form. Stop at
/// missing components or the symlink limit; propagate inspection and link-read errors.
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
                let Some(found) = symlink_metadata_if_present(&candidate)? else {
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

/// Predict a path's target by canonicalizing its nearest existing ancestor and appending
/// missing components. Fall back to lexical normalization if no ancestor resolves; the result
/// is only an estimate.
pub(crate) fn will_resolve_to(path: &Path) -> PathBuf {
    let mut trailing = Vec::new();
    let mut ancestor = path;
    loop {
        if let Ok(canonical) = canonicalize(ancestor) {
            let mut result = canonical;
            result.extend(trailing.iter().rev());
            return result;
        }
        let (Some(parent), Some(name)) = (ancestor.parent(), ancestor.file_name()) else {
            return normalize_lexically(path);
        };
        trailing.push(name);
        ancestor = parent;
    }
}

/// The canonical form of `path`, as [`fs::canonicalize`] resolves it. On Windows, the `\\?\`
/// prefix that gives it is dropped wherever the path means the same without it, so the path
/// reads as users write it and Git, which refuses such a path, can be handed it.
pub(crate) fn canonicalize(path: &Path) -> io::Result<PathBuf> {
    let canonical = fs::canonicalize(path)?;
    if cfg!(windows)
        && let Some(plain) = canonical.to_str().and_then(without_verbatim_prefix)
    {
        return Ok(PathBuf::from(plain));
    }
    Ok(canonical)
}

/// A verbatim Windows path, `\\?\C:\...` or `\\?\UNC\server\share\...`, as the path that means
/// the same without the prefix, or `None` where none does: one too long for the prefix-less
/// form, or with a component Windows would rewrite, such as a device name or a trailing dot.
fn without_verbatim_prefix(verbatim: &str) -> Option<String> {
    /// The longest path the prefix-less form reaches.
    const MAX_PATH: usize = 260;
    let plain = if let Some(unc) = verbatim.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        let rest = verbatim.strip_prefix(r"\\?\")?;
        let drive = rest.as_bytes();
        if drive.len() < 3 || !drive[0].is_ascii_alphabetic() || &drive[1..3] != br":\" {
            return None;
        }
        rest.to_owned()
    };
    let rewritten = |part: &str| {
        let stem = part.split('.').next().unwrap_or_default().trim_end();
        let device = ["CON", "PRN", "AUX", "NUL"]
            .iter()
            .any(|name| stem.eq_ignore_ascii_case(name))
            || (stem.len() == 4
                && stem.get(..3).is_some_and(|prefix| {
                    ["COM", "LPT"]
                        .iter()
                        .any(|name| prefix.eq_ignore_ascii_case(name))
                })
                && stem.as_bytes()[3].is_ascii_digit());
        device || part.ends_with('.') || part.ends_with(' ')
    };
    let components = plain.split('\\').skip(1).filter(|part| !part.is_empty());
    (plain.len() < MAX_PATH && !components.clone().any(rewritten)).then_some(plain)
}

/// Canonicalize a path, falling back to lexical normalization on failure. The fallback resolves
/// `..` textually and may differ from filesystem resolution through symlinks.
pub(crate) fn canonicalize_or_normalize(path: &Path) -> PathBuf {
    canonicalize(path).unwrap_or_else(|_| normalize_lexically(path))
}

/// Where an existing symlink points, as the operating system would read it.
fn target_of(link: &Path, written: &Path) -> PathBuf {
    let directory = match link.parent() {
        Some(parent) => canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf()),
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

/// Make a path absolute against the current directory and normalize it lexically.
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
                // Cancel only named components; preserve leading `..` in relative paths.
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
    fn a_verbatim_path_loses_its_prefix_where_it_means_the_same_without_it() {
        let plain = |verbatim: &str| without_verbatim_prefix(verbatim);
        assert_eq!(
            plain(r"\\?\C:\Users\me\dotfiles").as_deref(),
            Some(r"C:\Users\me\dotfiles")
        );
        assert_eq!(plain(r"\\?\D:\").as_deref(), Some(r"D:\"));
        assert_eq!(
            plain(r"\\?\UNC\server\share\dotfiles").as_deref(),
            Some(r"\\server\share\dotfiles")
        );
        for kept in [
            r"C:\Users\me",
            r"\\?\Volume{0-1}\dotfiles",
            r"\\?\C:\Users\con\dotfiles",
            r"\\?\C:\Users\nul.txt",
            r"\\?\C:\Users\COM1",
            r"\\?\C:\Users\trailing.",
            r"\\?\C:\Users\trailing ",
        ] {
            assert_eq!(plain(kept), None, "{kept}");
        }
        assert_eq!(
            plain(&format!(r"\\?\C:\{}", "a".repeat(300))),
            None,
            "too long without the prefix"
        );
        assert_eq!(
            plain(r"\\?\C:\Users\console").as_deref(),
            Some(r"C:\Users\console")
        );
    }

    #[test]
    fn normalizing_cancels_only_what_it_can() {
        assert_eq!(
            normalize_lexically(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
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
        assert_eq!(
            target_of(link, Path::new("/repo/../outside")),
            PathBuf::from("/outside")
        );
    }

    /// The node at `path`, where inspecting it is expected to succeed.
    fn node(path: &Path) -> Option<fs::Metadata> {
        symlink_metadata_if_present(path).expect("an inspectable path")
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

        assert!(set_aside(&real.join("source"), &real));
        assert!(set_aside(&alias.join("source"), &alias));
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
        // Skip the refusal assertion if this process can bypass permissions.
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
