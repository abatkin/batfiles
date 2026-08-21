//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Resolving what a `source` and a `dest` mean happens
//! here rather than inside the records, so there is one place that decides.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

// Making a symlink is the one platform-specific call here. Windows needs
// `symlink_file` against `symlink_dir` and a privilege check, with no CI runner
// and no user to prove it against, so it is not built — `link` refuses the
// action instead, and this stands in so the crate still compiles there.
#[cfg(unix)]
use std::os::unix::fs::symlink;

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

use crate::error::{Error, ExistingNode};
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::{Action, SymlinkAction};
use crate::output::Reporter;

/// Execute every action in declaration order, stopping at the first failure.
///
/// The two roots an action can reach are anchored once, here, because a symlink
/// stores the target it is given: a relative one is read back relative to the
/// link's own directory rather than to wherever batfiles happened to be run,
/// so a relative `--batfiles-dir` would otherwise produce a link that points
/// nowhere and a next run that calls it correct.
pub(crate) fn sync(roots: &Roots, manifest: &Manifest, reporter: &Reporter) -> Result<(), Error> {
    let repository = anchor(&roots.batfiles_dir)?;
    let home = anchor(&roots.home)?;
    for action in &manifest.actions {
        match action {
            Action::Symlink(action) => link(action, &repository, &home, reporter)?,
        }
    }
    Ok(())
}

/// Create one symlink, repair it, or leave it alone.
///
/// The destination is inspected without following a final symlink, so a link is
/// judged by where it points rather than by what it reaches. A link into the
/// repository is one batfiles would have made and holds no data of its own, so
/// repointing it loses nothing; anything else is someone's, and this refuses it
/// (`guidance.md`, rule 13).
fn link(
    action: &SymlinkAction,
    repository: &Path,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Refused on sight, before anything is inspected or removed. Repairing a
    // link deletes the old one first, so a platform check made at the moment of
    // writing would fail with the destination already gone.
    if !cfg!(unix) {
        return Err(Error::Unsupported { action: "symlink" });
    }

    let target = source(repository, &action.source)?;
    let dest = destination(home, &action.dest)?;

    match fs::symlink_metadata(&dest) {
        Ok(existing) if existing.is_symlink() => {
            let current = fs::read_link(&dest).map_err(|source| Error::Read {
                path: dest.clone(),
                source,
            })?;
            // Where the link points, not how it was spelled. A target is read
            // back exactly as it was written, and a relative one means "from
            // the link's own directory" — so `../dotfiles/zshrc` may well point
            // into the repository, and `repo/../elsewhere` may well point out
            // of it. Judging the spelling gets both backwards.
            let points_at = pointed_at(&dest, &current);
            if points_at == target {
                reporter.detail(1, &format!("unchanged {}", dest.display()));
            } else if points_at.starts_with(repository) {
                remove(&dest)?;
                create(&target, &dest)?;
                reporter.info(&format!(
                    "relinked {} -> {} (was {})",
                    dest.display(),
                    target.display(),
                    current.display()
                ));
            } else {
                return Err(Error::DestinationExists {
                    path: dest,
                    found: ExistingNode::Link {
                        written: current,
                        points_at,
                    },
                });
            }
        }
        Ok(existing) => {
            return Err(Error::DestinationExists {
                path: dest,
                found: existing_node(&existing),
            });
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|source| Error::Write {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            create(&target, &dest)?;
            reporter.info(&format!(
                "linked {} -> {}",
                dest.display(),
                target.display()
            ));
        }
        Err(source) => return Err(Error::Read { path: dest, source }),
    }
    Ok(())
}

/// Name what is sitting at a destination, so the refusal can say which of rule
/// 13's cases it hit rather than only that it hit one.
///
/// The symlink case is not here: it needs the link's target, which the caller
/// has already read in order to decide the link is not repairable.
fn existing_node(existing: &fs::Metadata) -> ExistingNode {
    if existing.is_file() {
        ExistingNode::File
    } else if existing.is_dir() {
        ExistingNode::Directory
    } else {
        ExistingNode::Other
    }
}

/// Where an existing symlink points, as the operating system would read it.
///
/// Lexical, like every other path decision here: intermediate components that
/// are themselves symlinks are not resolved, because a parent link is something
/// the user put there deliberately and following it is ordinary path
/// resolution, not a fact batfiles needs to establish.
fn pointed_at(link: &Path, target: &Path) -> PathBuf {
    match link.parent() {
        Some(parent) if target.is_relative() => normalize(&parent.join(target)),
        _ => normalize(target),
    }
}

/// Make a path absolute and lexically clean, so that what is compared, reported,
/// and written into a link does not depend on the working directory.
fn anchor(path: &Path) -> Result<PathBuf, Error> {
    let absolute =
        std::path::absolute(path).map_err(|source| Error::WorkingDirectory { source })?;
    Ok(normalize(&absolute))
}

/// An action's `source`, resolved against the repository that declared it.
///
/// A source names a path within its repository, so an absolute one and one that
/// climbs out are both refused. `repository` is already anchored, so the result
/// is too. The containment check is lexical: a symlink deliberately stored
/// inside the repository may point anywhere, and is followed like any other.
fn source(repository: &Path, source: &str) -> Result<PathBuf, Error> {
    if Path::new(source).is_absolute() {
        return Err(Error::SourceAbsolute {
            value: source.to_owned(),
        });
    }
    let resolved = normalize(&repository.join(source));
    if !resolved.starts_with(repository) {
        return Err(Error::SourceOutsideRepository {
            value: source.to_owned(),
        });
    }

    // Presence, not reachability: a source that is itself a dangling symlink is
    // there, and linking to it is what the repository asked for.
    match fs::symlink_metadata(&resolved) {
        Ok(_) => Ok(resolved),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(Error::SourceMissing { path: resolved })
        }
        Err(source) => Err(Error::Read {
            path: resolved,
            source,
        }),
    }
}

/// An action's `dest`, resolved against the selected home.
///
/// `~` and a relative path both resolve from the selected home rather than from
/// an independently discovered one, and an absolute path is used as written.
/// None of this makes the home a boundary: a destination may deliberately point
/// outside it, and only `--home-dir` decides what "home" means.
fn destination(home: &Path, dest: &str) -> Result<PathBuf, Error> {
    let path = match dest.strip_prefix('~') {
        Some("") => home.to_path_buf(),
        Some(rest) if rest.starts_with('/') => home.join(rest.trim_start_matches('/')),
        Some(_) => {
            return Err(Error::DestinationOtherHome {
                value: dest.to_owned(),
            });
        }
        // `join` returns an absolute `dest` unchanged, which is the rule for
        // one, so the relative and absolute cases are the same line.
        None => home.join(dest),
    };
    Ok(normalize(&path))
}

/// Resolve `.` and `..` textually, without consulting the filesystem.
///
/// Deliberately not canonicalization: batfiles does not prove that a path stays
/// beneath a root by resolving every component, because a parent that is a
/// symlink is followed by ordinary path resolution and should be here too.
fn normalize(path: &Path) -> PathBuf {
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

fn create(target: &Path, dest: &Path) -> Result<(), Error> {
    symlink(target, dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

fn remove(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(|source| Error::Write {
        path: dest.to_path_buf(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    fn dest_of(dest: &str) -> Result<PathBuf, Error> {
        destination(&home(), dest)
    }

    #[test]
    fn a_destination_resolves_against_the_selected_home() {
        assert_eq!(dest_of("~/.zshrc").unwrap(), home().join(".zshrc"));
        assert_eq!(dest_of(".zshrc").unwrap(), home().join(".zshrc"));
        assert_eq!(dest_of("~").unwrap(), home());
        assert_eq!(dest_of("/etc/hosts").unwrap(), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn only_the_selected_home_is_spelled_with_a_tilde() {
        assert!(dest_of("~other/.zshrc").is_err());
    }

    #[test]
    fn a_destination_may_deliberately_leave_the_home() {
        // The home is a base, not a boundary. Someone linking into a sibling
        // directory is expressing intent, not making a mistake.
        assert_eq!(
            dest_of("~/../shared/rc").unwrap(),
            PathBuf::from("/home/shared/rc")
        );
    }

    #[test]
    fn a_source_stays_within_its_repository() {
        let repository = Path::new("/repo");
        for escaping in ["/etc/passwd", "../secrets", "files/../../secrets"] {
            assert!(
                source(repository, escaping).is_err(),
                "`{escaping}` was accepted"
            );
        }
    }

    #[test]
    fn a_link_is_judged_by_where_it_points_not_how_it_is_spelled() {
        let link = Path::new("/home/user/.zshrc");
        // Relative to the link's own directory, which is what the OS does.
        assert_eq!(
            pointed_at(link, Path::new("../dotfiles/zshrc")),
            PathBuf::from("/home/dotfiles/zshrc")
        );
        // Textually inside `/repo`, and nowhere near it once resolved.
        assert_eq!(
            pointed_at(link, Path::new("/repo/../outside")),
            PathBuf::from("/outside")
        );
    }

    #[test]
    fn normalizing_cancels_only_what_it_can() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        // Nothing to cancel: the root has no parent, and a leading `..` in a
        // relative path is part of where it points.
        assert_eq!(normalize(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(normalize(Path::new("../../a")), PathBuf::from("../../a"));
    }
}
