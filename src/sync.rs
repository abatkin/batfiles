//! `sync`: bring the home directory to the state a repository's actions
//! describe.
//!
//! One loop over one ordered list, each action inspecting the filesystem as the
//! previous one left it. Resolving what a `source` and a `dest` mean happens
//! here rather than inside the records, so there is one place that decides.
//!
//! What a path means, and what is already *at* one, is [`crate::paths`]':
//! every action asks it the same questions and they are not asked twice.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// Making a symlink is the one platform-specific call here. Windows needs
// `symlink_file` against `symlink_dir` and a privilege check, with no CI runner
// and no user to prove it against, so it is not built —
// `require_symlink_support` refuses the action instead, and the stand-in below
// keeps the crate compiling there.
#[cfg(unix)]
use std::os::unix::fs::symlink;

use crate::error::Error;
use crate::location::Roots;
use crate::manifest::Manifest;
use crate::manifest::action::{Action, SymlinkAction, SymlinkDirAction};
use crate::output::Reporter;
use crate::paths::{self, Occupant, Repository};

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// Execute every action in declaration order, stopping at the first failure.
///
/// The two roots an action can reach are anchored once, here, because a symlink
/// stores the target it is given: a relative one is read back relative to the
/// link's own directory rather than to wherever batfiles happened to be run,
/// so a relative `--batfiles-dir` would otherwise produce a link that points
/// nowhere and a next run that calls it correct.
pub(crate) fn sync(roots: &Roots, manifest: &Manifest, reporter: &Reporter) -> Result<(), Error> {
    let repository = Repository::at(&roots.batfiles_dir)?;
    let home = paths::anchor(&roots.home)?;
    for action in &manifest.actions {
        match action {
            Action::Symlink(action) => link(action, &repository, &home, reporter)?,
            Action::SymlinkDir(action) => link_dir(action, &repository, &home, reporter)?,
        }
    }
    Ok(())
}

/// Refuse an action type that makes symlinks where batfiles cannot make one.
///
/// Called on sight, before anything is inspected or removed. Repairing a link
/// deletes the old one first, so a platform check made at the moment of writing
/// would fail with the destination already gone.
fn require_symlink_support(action_type: &'static str) -> Result<(), Error> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(Error::Unsupported { action_type })
    }
}

/// Carry out one `symlink` action: the whole of it is one link.
fn link(
    action: &SymlinkAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    require_symlink_support("symlink")?;

    let target = resolve_source(repository, &action.source)?;
    let dest = resolve_destination(home, &action.dest);
    link_one(&target, &dest, repository, reporter)
}

/// Carry out one `symlink-dir` action: one link per direct child of a
/// directory, all of them in one destination directory.
///
/// Not recursive. A child that is itself a directory becomes one link like any
/// other, so what is under it is reached through that link and a file added
/// there later needs no further sync.
fn link_dir(
    action: &SymlinkDirAction,
    repository: &Repository,
    home: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    require_symlink_support("symlink-dir")?;

    let source_dir = resolve_source(repository, &action.source_dir)?;
    // Followed, unlike a destination: a `source-dir` that is a symlink to a
    // directory inside the repository is something the repository put there
    // deliberately, and its children are what the action is asking for.
    if !fs::metadata(&source_dir)
        .map_err(|source| Error::Read {
            path: source_dir.clone(),
            source,
        })?
        .is_dir()
    {
        return Err(Error::SourceNotADirectory { path: source_dir });
    }

    let dest_dir = resolve_destination(home, &action.dest_dir);
    paths::ensure_directory(&dest_dir)?;

    let children = children_of(&source_dir)?;
    if children.is_empty() {
        reporter.detail(
            1,
            &format!("no children to link in {}", source_dir.display()),
        );
    }
    for child in children {
        // Lossy only where a name is not UTF-8, and only for the refusal's
        // message; the paths themselves are joined from the original `OsString`.
        let name = child.to_string_lossy();
        if action.dot_prefix && name.starts_with('.') {
            return Err(Error::DotPrefixOnDotfile {
                child: name.into_owned(),
            });
        }
        let installed = if action.dot_prefix {
            let mut dotted = OsString::from(".");
            dotted.push(&child);
            dotted
        } else {
            child.clone()
        };
        link_one(
            &source_dir.join(&child),
            &dest_dir.join(installed),
            repository,
            reporter,
        )?;
    }
    Ok(())
}

/// The direct children of a directory, sorted by name.
///
/// Sorted because `read_dir` yields whatever order the filesystem holds, and an
/// action that reports its work in a different order on every machine is one
/// nobody can diff.
fn children_of(dir: &Path) -> Result<Vec<OsString>, Error> {
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

/// Create one symlink, repair it, or leave it alone.
///
/// What is at the destination, and whether it is batfiles' to replace, is
/// [`Occupant::at`]'s answer — see that module for why the question cannot be
/// asked of the written path. This decides only what a `symlink` does with each
/// answer.
///
/// Both action types end here, one call per link either of them installs, so
/// rule 13 is applied in one place regardless of how many links an action is.
fn link_one(
    target: &Path,
    dest: &Path,
    repository: &Repository,
    reporter: &Reporter,
) -> Result<(), Error> {
    match Occupant::at(dest, repository)? {
        // Compared in resolved form on both sides. A link written by an earlier
        // run holds the anchored spelling, which is the same place by a
        // different name wherever a root contains a symlink — and calling that
        // stale would relink it, and every link like it, on every run.
        Occupant::Owned { points_at, .. } if points_at == paths::resolved(target) => {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
        }
        Occupant::Owned { written, .. } => {
            remove(dest)?;
            create(target, dest)?;
            reporter.info(&format!(
                "relinked {} -> {} (was {})",
                dest.display(),
                target.display(),
                written.display()
            ));
        }
        Occupant::Vacant => {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|source| Error::Write {
                    path: parent.to_path_buf(),
                    source,
                })?;
            }
            create(target, dest)?;
            reporter.info(&format!(
                "linked {} -> {}",
                dest.display(),
                target.display()
            ));
        }
        Occupant::Unmanaged(found) => {
            return Err(Error::DestinationExists {
                path: dest.to_path_buf(),
                found,
            });
        }
    }
    Ok(())
}

/// An action's `source`, resolved against the repository that declared it.
///
/// The manifest has already settled what a source may say, so this expects one
/// that is relative and lands strictly inside the repository, and checks only
/// the rule needing a filesystem: the path has to exist. The repository is
/// already anchored, so the result is too — this is a path batfiles writes into
/// a link, not one it classifies, so it keeps the spelling the user chose.
///
/// The one place a repository path is resolved, which 6.3 widens to take
/// `@remote/path` (`guidance.md`, "Seams the late slices need").
fn resolve_source(repository: &Repository, source: &str) -> Result<PathBuf, Error> {
    let resolved = paths::normalize(&repository.path().join(source));

    // Presence, not reachability: a source that is itself a dangling symlink is
    // there, and linking to it is what the repository asked for.
    match fs::symlink_metadata(&resolved) {
        Ok(_) => Ok(resolved),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(Error::SourceMissing { path: resolved })
        }
        Err(error) => Err(Error::Read {
            path: resolved,
            source: error,
        }),
    }
}

/// An action's `dest`, resolved against the selected home.
///
/// `~` and a relative path both resolve from the selected home rather than from
/// an independently discovered one, and an absolute path is used as written.
/// None of this makes the home a boundary: a destination may deliberately point
/// outside it, and only `--home-dir` decides what "home" means.
///
/// Infallible: the manifest has already refused an empty `dest` and a `~other`,
/// so what reaches this is `~`, `~/…`, or an ordinary path.
fn resolve_destination(home: &Path, dest: &str) -> PathBuf {
    let path = match dest.strip_prefix('~') {
        // `~` leaves nothing and `~/…` leaves a separator, so trimming covers
        // both without a second arm.
        Some(rest) => home.join(rest.trim_start_matches('/')),
        // `join` returns an absolute `dest` unchanged, which is the rule for
        // one, so the relative and absolute cases are the same line.
        None => home.join(dest),
    };
    paths::normalize(&path)
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

    fn dest_of(dest: &str) -> PathBuf {
        resolve_destination(&home(), dest)
    }

    // What a `dest` may say is checked by `manifest`, and tested there. These
    // cover the other half: what an accepted one resolves to.

    #[test]
    fn a_destination_resolves_against_the_selected_home() {
        assert_eq!(dest_of("~/.zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of(".zshrc"), home().join(".zshrc"));
        assert_eq!(dest_of("~"), home());
        assert_eq!(dest_of("/etc/hosts"), PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn a_destination_may_deliberately_leave_the_home() {
        // The home is a base, not a boundary. Someone linking into a sibling
        // directory is expressing intent, not making a mistake.
        assert_eq!(dest_of("~/../shared/rc"), PathBuf::from("/home/shared/rc"));
    }

    // Composing a path, and classifying what is already at one, moved to
    // `paths`, and their tests went with them.
}
