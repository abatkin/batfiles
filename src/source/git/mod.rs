//! Clone repositories, update existing clones conservatively, and read the origin a checkout
//! records. Dry runs report clones and updates without running Git; reading an origin is a
//! local query they make. Git configuration and credentials are inherited; repository
//! redirects are cleared.

mod clone;
mod origin;
mod process;
mod update;

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error as ThisError;

pub(crate) use clone::{clone_or_update, clone_repository, ref_suffix};
pub(crate) use origin::{Origin, recorded_origin, same_origin};
use process::{launch, stderr_summary};

/// Failures when cloning or updating a repository.
#[derive(Debug, ThisError)]
pub(crate) enum GitError {
    /// `git` could not be run at all, most often because it is not on `PATH`.
    #[error(
        "could not run git: {source}. batfiles runs the `git` on your PATH so that your \
         gitconfig, credential helpers, and SSH agent apply"
    )]
    Unavailable { source: io::Error },

    /// A `git` command ran and failed, carrying git's own diagnostic.
    #[error("git {command} failed in {}: {message}", .path.display())]
    Failed {
        command: &'static str,
        path: PathBuf,
        message: String,
    },

    /// A clone destination holding a directory with no `.git` in it.
    #[error(
        "cannot update {}: it is a directory, and not a git clone; \
         move it aside and run sync again",
        .path.display()
    )]
    NotAClone { path: PathBuf },

    /// The destination uses an indirect `.git` or a worktree rooted elsewhere.
    #[error(
        "cannot update {}: its .git belongs to a checkout somewhere else, so updating it \
         would change that one; move it aside and run sync again",
        .path.display()
    )]
    CloneElsewhere { path: PathBuf },

    /// The destination's `.git` has no resolvable `HEAD` commit.
    #[error(
        "cannot update {}: it has a .git but nothing checked out, so it is an incomplete \
         or damaged clone ({message}); move it aside and run sync again",
        .path.display()
    )]
    CloneIncomplete { path: PathBuf, message: String },

    /// The declared ref resolves to no branch, tag, or commit in the clone.
    #[error(
        "cannot follow `{git_ref}` in {}: no branch, tag, or commit of that name is in the clone",
        .path.display()
    )]
    RefUnresolvable { path: PathBuf, git_ref: String },
}

impl GitError {
    /// Describe an unusable clone for conflict handling, or return `None` for unrelated
    /// failures. The description completes "`dest` is ...".
    fn unusable_clone(&self) -> Option<String> {
        match self {
            Self::NotAClone { .. } => Some("a directory, and not a git clone".to_owned()),
            Self::CloneElsewhere { .. } => {
                Some("a directory whose .git belongs to a checkout somewhere else".to_owned())
            }
            Self::CloneIncomplete { message, .. } => {
                Some(format!("an incomplete or damaged clone ({message})"))
            }
            Self::Unavailable { .. } | Self::Failed { .. } | Self::RefUnresolvable { .. } => None,
        }
    }
}

/// Initialize a repository in `dir` unless it is already inside a worktree. Return whether a
/// repository was created. This function always performs writes when needed.
pub(crate) fn init_repository(dir: &Path) -> Result<bool, GitError> {
    if inside_work_tree(dir)? {
        return Ok(false);
    }

    let output = launch(Some(dir), &["init"])?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(GitError::Failed {
            command: "init",
            path: dir.to_path_buf(),
            message: stderr_summary(&output),
        })
    }
}

/// Return whether `dir` is inside a worktree, including a parent repository. Bare repositories
/// and paths outside a repository return `false`.
fn inside_work_tree(dir: &Path) -> Result<bool, GitError> {
    let output = launch(Some(dir), &["rev-parse", "--is-inside-work-tree"])?;
    Ok(output.status.success() && String::from_utf8_lossy(&output.stdout).trim() == "true")
}
