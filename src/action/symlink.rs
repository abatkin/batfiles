//! `symlink` and `symlink-dir`: link a source node or each of its direct children.

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
use crate::replace::ConflictDecision;

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Create or repair the symlink declared by one `symlink` action.
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

/// Create one symlink, repair it, or leave it alone; settle an unmanaged node
/// in the way under the run's conflict policy.
fn link_one(target: &Path, dest: &Path, context: &RunContext) -> Result<(), Error> {
    let reporter = context.reporter();
    let mode = context.mode();
    let resolver = context.resolver();
    match Occupancy::at(dest, context.repository())? {
        Occupancy::Replaceable { points_at, .. }
            if points_at == paths::canonicalize_or_normalize(target) =>
        {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
        }
        Occupancy::Replaceable { written, .. } => {
            paths::refuse_destination_inside_source(target, dest)?;
            paths::refuse_setting_aside_a_source(target, dest)?;
            if mode.writes() {
                remove(dest)?;
                create(target, dest)?;
            }
            reporter.info(&format!(
                "{} {} -> {} (was {})",
                Verb::Relink.for_mode(mode),
                dest.display(),
                target.display(),
                written.display()
            ));
        }
        Occupancy::Vacant => {
            paths::refuse_destination_inside_source(target, dest)?;
            // Resolve the conflict before creating parent directories.
            if !directory::create_parents(dest, &resolver)? {
                return Ok(());
            }
            if mode.writes() {
                create(target, dest)?;
            }
            report_link(target, dest, context);
        }
        Occupancy::Unmanaged(found) => {
            paths::refuse_destination_inside_source(target, dest)?;
            paths::refuse_setting_aside_a_source(target, dest)?;
            match resolver.resolve(dest, &found)? {
                ConflictDecision::Refuse => {
                    return Err(Error::DestinationExists {
                        path: dest.to_path_buf(),
                        found,
                    });
                }
                ConflictDecision::Skip => {}
                ConflictDecision::Replace(keep) => {
                    resolver.replace(dest, keep, || {
                        if mode.writes() {
                            create(target, dest)?;
                        }
                        Ok(())
                    })?;
                    report_link(target, dest, context);
                }
            }
        }
    }
    Ok(())
}

/// Report a symlink creation or planned creation.
fn report_link(target: &Path, dest: &Path, context: &RunContext) {
    context.reporter().info(&format!(
        "{} {} -> {}",
        Verb::Link.for_mode(context.mode()),
        dest.display(),
        target.display()
    ));
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
