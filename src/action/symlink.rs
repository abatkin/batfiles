//! `symlink` and `symlink-dir`: the same link, made once or once per child.
//!
//! Both end at [`link_one`], so rule 13 is applied in one place regardless of
//! how many links an action is.

use std::fs;
use std::path::Path;

// Making a symlink is the one platform-specific call in the crate. Windows
// needs `symlink_file` against `symlink_dir` and a privilege check, with no CI
// runner and no user to prove it against, so it is not built —
// `require_symlink_support` refuses the action instead, and the stand-in below
// keeps the crate compiling there.
#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::{Context, install_children, source_directory};
use crate::error::Error;
use crate::manifest::action::{SymlinkAction, SymlinkDirAction};
use crate::paths::{self, Occupant};

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Carry out one `symlink` action: the whole of it is one link.
pub(super) fn link(action: &SymlinkAction, context: &Context) -> Result<(), Error> {
    require_symlink_support("symlink")?;

    let target = context.source(&action.source)?;
    let dest = context.destination(&action.dest);
    link_one(&target, &dest, context)
}

/// Carry out one `symlink-dir` action: one link per direct child of a
/// directory, all of them in one destination directory.
///
/// A child that is itself a directory becomes one link like any other, so what
/// is under it is reached through that link and a file added there later needs
/// no further sync.
pub(super) fn link_dir(action: &SymlinkDirAction, context: &Context) -> Result<(), Error> {
    require_symlink_support("symlink-dir")?;

    let source_dir = source_directory(context, &action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    install_children(
        context,
        &source_dir,
        &dest_dir,
        action.dot_prefix,
        "link",
        |source, dest| link_one(source, dest, context),
    )
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

/// Create one symlink, repair it, or leave it alone.
///
/// What is at the destination, and whether it is batfiles' to replace, is
/// [`Occupant::at`]'s answer — see that module for why the question cannot be
/// asked of the written path. This decides only what a `symlink` does with each
/// answer.
fn link_one(target: &Path, dest: &Path, context: &Context) -> Result<(), Error> {
    let reporter = context.reporter();
    match Occupant::at(dest, context.repository())? {
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
            paths::create_parents(dest)?;
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
