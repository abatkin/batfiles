//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Resolving what a `source` and a `dest` mean happens
//! here rather than inside the records, so there is one place that decides.

use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Component, Path, PathBuf};

use crate::config::Roots;
use crate::error::Error;
use crate::output::Reporter;
use crate::repo::BatfilesConfig;
use crate::repo::action::{Action, SymlinkAction};

/// Execute every action in declaration order, stopping at the first failure.
pub(crate) fn sync(
    roots: &Roots,
    config: &BatfilesConfig,
    reporter: &Reporter,
) -> Result<(), Error> {
    for action in &config.actions {
        match action {
            Action::Symlink(action) => link(action, roots, reporter)?,
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
fn link(action: &SymlinkAction, roots: &Roots, reporter: &Reporter) -> Result<(), Error> {
    let target = source(&roots.batfiles_dir, &action.source)?;
    let dest = destination(&roots.home, &action.dest)?;

    match fs::symlink_metadata(&dest) {
        Ok(existing) if existing.is_symlink() => {
            let current = fs::read_link(&dest).map_err(|source| Error::Read {
                path: dest.clone(),
                source,
            })?;
            if current == target {
                reporter.detail(1, &format!("unchanged {}", dest.display()));
            } else if current.starts_with(&roots.batfiles_dir) {
                remove(&dest)?;
                create(&target, &dest)?;
                reporter.info(&format!(
                    "relinked {} -> {} (was {})",
                    dest.display(),
                    target.display(),
                    current.display()
                ));
            } else {
                return Err(Error::Occupied { path: dest });
            }
        }
        Ok(_) => return Err(Error::Occupied { path: dest }),
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

/// An action's `source`, resolved against the repository that declared it.
///
/// A source names a path within its repository, so an absolute one and one that
/// climbs out are both refused. The containment check is lexical: a symlink
/// deliberately stored inside the repository may point anywhere, and is followed
/// like any other.
fn source(repository: &Path, source: &str) -> Result<PathBuf, Error> {
    let invalid = |message| {
        Err(Error::Path {
            field: "source",
            value: source.to_owned(),
            message,
        })
    };
    if Path::new(source).is_absolute() {
        return invalid("is absolute; a source names a path within the repository");
    }
    let resolved = normalize(&repository.join(source));
    if !resolved.starts_with(repository) {
        return invalid("resolves outside the repository");
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
            return Err(Error::Path {
                field: "destination",
                value: dest.to_owned(),
                message: "names another user's home; `~` expands only to the selected home",
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
    fn normalizing_cancels_only_what_it_can() {
        assert_eq!(normalize(Path::new("/a/./b/../c")), PathBuf::from("/a/c"));
        // Nothing to cancel: the root has no parent, and a leading `..` in a
        // relative path is part of where it points.
        assert_eq!(normalize(Path::new("/..")), PathBuf::from("/"));
        assert_eq!(normalize(Path::new("../../a")), PathBuf::from("../../a"));
    }
}
