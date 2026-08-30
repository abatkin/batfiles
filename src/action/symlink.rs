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

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::directory;
use crate::error::Error;
use crate::manifest::action::{SymlinkAction, SymlinkDirAction};
use crate::mode::Verb;
use crate::paths::{self, Occupancy};

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Carry out one `symlink` action: the whole of it is one link.
pub(super) fn link(action: &SymlinkAction, context: &RunContext) -> Result<(), Error> {
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
pub(super) fn link_dir(action: &SymlinkDirAction, context: &RunContext) -> Result<(), Error> {
    require_symlink_support("symlink-dir")?;

    let source_dir = context.source_directory(&action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    // Before the destination is created, for the same reason `copy-dir` checks
    // first: creating it inside the source is what puts it in the list of
    // children about to be linked, and it would then be linked into itself.
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    for_each_child(
        context,
        &ChildInstall {
            source_dir: &source_dir,
            dest_dir: &dest_dir,
            dot_prefix: action.dot_prefix,
            verb: Verb::Link,
        },
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
        Err(Error::UnsupportedOnPlatform { action_type })
    }
}

/// Create one symlink, repair it, or leave it alone.
///
/// What is at the destination, and whether it is batfiles' to replace, is
/// [`Occupancy::at`]'s answer — see that module for why the question cannot be
/// asked of the written path. This decides only what a `symlink` does with each
/// answer.
///
/// Both arms that write a link first refuse a destination inside the target, and
/// neither can ask that before the destination has been inspected: a link that
/// is already correct *resolves into* its own target, so the converged case is
/// indistinguishable from the offending one until it has been told apart. That
/// is why the check sits in two arms rather than above the match — the third
/// arm, which writes nothing, is exactly the one it would misjudge.
///
/// Those two arms are also where [`crate::mode::RunMode`] is read: under a dry run the link
/// is withheld and everything else happens as it would.
fn link_one(target: &Path, dest: &Path, context: &RunContext) -> Result<(), Error> {
    let reporter = context.reporter();
    let mode = context.mode();
    match Occupancy::at(dest, context.repository())? {
        // Compared in resolved form on both sides. A link written by an earlier
        // run holds the anchored spelling, which is the same place by a
        // different name wherever a root contains a symlink — and calling that
        // stale would relink it, and every link like it, on every run.
        Occupancy::Replaceable { points_at, .. } if points_at == paths::resolved(target) => {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
        }
        Occupancy::Replaceable { written, .. } => {
            // Before the removal, not after: this arm destroys the link that is
            // there, and a refusal arriving afterwards has already done the
            // damage it was raised to prevent.
            paths::refuse_destination_inside_source(target, dest)?;
            if mode.writes() {
                remove(dest)?;
                create(target, dest)?;
            }
            reporter.info(&format!(
                "{} {} -> {} (was {})",
                Verb::Relink.say(mode),
                dest.display(),
                target.display(),
                written.display()
            ));
        }
        Occupancy::Vacant => {
            paths::refuse_destination_inside_source(target, dest)?;
            // After that, so a doomed action makes no directories on its way.
            for link in directory::create_parents(dest, mode)?.removals() {
                reporter.info(&link.removal_note(mode));
            }
            if mode.writes() {
                create(target, dest)?;
            }
            reporter.info(&format!(
                "{} {} -> {}",
                Verb::Link.say(mode),
                dest.display(),
                target.display()
            ));
        }
        Occupancy::Unmanaged(found) => {
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
