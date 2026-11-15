//! Clone into a destination or validate its existing checkout before updating it.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use super::GitError;
use super::process::{is_repository_directory, launch, line_as_path, run_git, stderr_summary};
use super::update::{follow_ref, update_clone};
use crate::error::Error;
use crate::filesystem::directory;
use crate::filesystem::paths::{self, ExistingNode, Occupancy, RepositoryRoot};
use crate::filesystem::replace::{ConflictDecision, ConflictResolver};
use crate::output::{Reporter, Verb};

/// Clone into a vacant or replaceable destination, or update an existing clone. Resolve other
/// occupants, including unusable clones, using `resolver`. Existing clones retain their
/// configured remotes regardless of `url`. Dry runs inspect and report without invoking Git.
pub(crate) fn clone_or_update(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    repository: &RepositoryRoot,
    resolver: &ConflictResolver<'_>,
) -> Result<(), Error> {
    let (mode, reporter) = (resolver.mode(), resolver.reporter());
    let clone_here = || clone(url, dest, git_ref, resolver);
    match Occupancy::at(dest, repository)? {
        // Validating a clone invokes Git, which dry runs must not do.
        Occupancy::Unmanaged(ExistingNode::Directory) => {
            if !mode.writes() {
                reporter.info(&format!(
                    "{} {}",
                    Verb::Update.for_mode(mode),
                    dest.display()
                ));
                return Ok(());
            }
            let failure = match validate_clone(dest) {
                Ok(()) => return update_clone(dest, git_ref, reporter),
                Err(Error::Git(failure)) => failure,
                Err(error) => return Err(error),
            };
            let Some(found) = failure.unusable_clone() else {
                return Err(failure.into());
            };
            match resolver.resolve(dest, &found)? {
                ConflictDecision::Refuse => Err(failure.into()),
                ConflictDecision::Skip => Ok(()),
                ConflictDecision::Replace(keep) => resolver.replace(dest, keep, clone_here),
            }
        }
        Occupancy::Unmanaged(found) => match resolver.resolve(dest, &found)? {
            ConflictDecision::Refuse => Err(Error::DestinationExists {
                path: dest.to_path_buf(),
                found,
            }),
            ConflictDecision::Skip => Ok(()),
            ConflictDecision::Replace(keep) => resolver.replace(dest, keep, clone_here),
        },
        Occupancy::Replaceable { written, .. } => {
            if mode.writes() {
                remove_replaceable_link(dest)?;
            }
            reporter.info(&format!(
                "{} the symlink at {} to {} to make room for the clone",
                Verb::Remove.for_mode(mode),
                dest.display(),
                written.display()
            ));
            clone_here()
        }
        Occupancy::Vacant => clone_here(),
    }
}

/// Clone into a vacant destination, creating parents and following the declared ref. Report the
/// operation or dry-run intent.
fn clone(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    resolver: &ConflictResolver<'_>,
) -> Result<(), Error> {
    let (mode, reporter) = (resolver.mode(), resolver.reporter());
    if !directory::create_parents(dest, resolver)? {
        return Ok(());
    }
    if mode.writes() {
        clone_repository(url, dest, git_ref, reporter)?;
    }
    reporter.info(&format!(
        "{} {} from {url}{}",
        Verb::Clone.for_mode(mode),
        dest.display(),
        ref_suffix(git_ref)
    ));
    Ok(())
}

/// Clone into a vacant destination and follow `git_ref` where one is given, without reporting
/// the clone. Git creates missing parents. A ref that cannot be followed fails and leaves the
/// clone in place. This function always runs Git and does not check `RunMode`.
pub(crate) fn clone_repository(
    url: &str,
    dest: &Path,
    git_ref: Option<&str>,
    reporter: &Reporter,
) -> Result<(), Error> {
    run_git(
        None,
        "clone",
        &[
            OsStr::new("clone"),
            // Keep a URL beginning with `-` from being read as an option.
            OsStr::new("--"),
            OsStr::new(url),
            dest.as_os_str(),
        ],
        dest,
    )?;
    if let Some(git_ref) = git_ref {
        follow_ref(dest, git_ref, reporter)?;
    }
    Ok(())
}

/// ` at <ref>`, or nothing where none was declared.
pub(crate) fn ref_suffix(git_ref: Option<&str>) -> String {
    git_ref.map(|it| format!(" at {it}")).unwrap_or_default()
}

/// Require a real `.git` directory, a worktree rooted at `dest`, and a resolvable
/// HEAD. Refuse indirect, misplaced, incomplete, or damaged clones.
fn validate_clone(dest: &Path) -> Result<(), Error> {
    match own_git_directory(dest)? {
        GitDirectory::Missing => {
            return Err(GitError::NotAClone {
                path: dest.to_path_buf(),
            }
            .into());
        }
        GitDirectory::Indirect => {
            return Err(GitError::CloneElsewhere {
                path: dest.to_path_buf(),
            }
            .into());
        }
        GitDirectory::Damaged => {
            return Err(GitError::CloneIncomplete {
                path: dest.to_path_buf(),
                message: "git does not read its .git as a repository".to_owned(),
            }
            .into());
        }
        GitDirectory::Own => {}
    }

    // After layout validation, propagate Git failures instead of classifying them as
    // destination conflicts.
    let toplevel = run_git(
        Some(dest),
        "rev-parse",
        &["rev-parse", "--show-toplevel"],
        dest,
    )?;
    match line_as_path(&toplevel.stdout) {
        Some(root)
            if paths::canonicalize_or_normalize(&root)
                == paths::canonicalize_or_normalize(dest) => {}
        _ => {
            return Err(GitError::CloneElsewhere {
                path: dest.to_path_buf(),
            }
            .into());
        }
    }

    let output = launch(Some(dest), &["rev-parse", "--verify", "HEAD"])?;
    if !output.status.success() {
        return Err(GitError::CloneIncomplete {
            path: dest.to_path_buf(),
            message: stderr_summary(&output),
        }
        .into());
    }
    Ok(())
}

/// What sits at `<dest>/.git`, judged without following it.
enum GitDirectory {
    /// Nothing: a directory that is not a clone at all.
    Missing,
    /// A real directory, which is what `git clone` makes.
    Own,
    /// A real directory git does not read as a repository.
    Damaged,
    /// A symlink or file in place of the clone's own `.git` directory.
    Indirect,
}

fn own_git_directory(dest: &Path) -> Result<GitDirectory, Error> {
    let git_dir = dest.join(".git");
    Ok(match paths::symlink_metadata_if_present(&git_dir)? {
        None => GitDirectory::Missing,
        Some(found) if found.is_dir() => {
            if is_repository_directory(&git_dir, dest)? {
                GitDirectory::Own
            } else {
                GitDirectory::Damaged
            }
        }
        Some(_) => GitDirectory::Indirect,
    })
}

/// Remove a symlink [`Occupancy`] classified as holding no content of its own.
fn remove_replaceable_link(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(Error::writing(dest))
}
