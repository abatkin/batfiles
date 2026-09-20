//! `symlink` and `symlink-dir`: the same link, made once or once per child.

use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::directory;
use crate::error::Error;
use crate::item::ItemId;
use crate::manifest::action::{SymlinkAction, SymlinkDirAction};
use crate::output::Verb;
use crate::paths::{self, Occupancy};

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Carry out one `symlink` action: the whole of it is one link.
pub(super) fn link(
    action: &SymlinkAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    require_symlink_support("symlink")?;

    let target = context.source(remote, &action.source)?;
    let dest = context.destination(&action.dest);
    link_one(&target, &dest, context)
}

/// Carry out one `symlink-dir` action: one link per direct child of a
/// directory, all of them in one destination directory.
pub(super) fn link_dir(
    action: &SymlinkDirAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    require_symlink_support("symlink-dir")?;

    let source_dir = context.source_directory(remote, &action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
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
fn require_symlink_support(action_type: &'static str) -> Result<(), Error> {
    if cfg!(unix) {
        Ok(())
    } else {
        Err(Error::UnsupportedOnPlatform { action_type })
    }
}

/// Create one symlink, repair it, or leave it alone.
fn link_one(target: &Path, dest: &Path, context: &RunContext) -> Result<(), Error> {
    let reporter = context.reporter();
    let mode = context.mode();
    match Occupancy::at(dest, context.repository())? {
        Occupancy::Replaceable { points_at, .. }
            if points_at == paths::canonicalize_or_normalize(target) =>
        {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
        }
        Occupancy::Replaceable { written, .. } => {
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
            directory::create_parents(dest, mode)?.report_removals(mode, reporter);
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
