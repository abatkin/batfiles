//! `symlink` and `symlink-dir`: link a source node or each of its direct children.

use std::fs;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::symlink;

use super::children::{ChildInstall, for_each_child};
use crate::error::Error;
use crate::filesystem::directory;
use crate::filesystem::paths::{self, Occupancy};
use crate::filesystem::replace::ConflictDecision;
use crate::manifest::actions::{SymlinkAction, SymlinkDirAction};
use crate::manifest::check::ValueRule;
use crate::manifest::entry_filter::EntryFilter;
use crate::manifest::item::ItemId;
use crate::manifest::remote::Tree;
use crate::manifest::template::Resolver;
use crate::output::Verb;
use crate::run::context::RunContext;

#[cfg(not(unix))]
fn symlink(_target: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// Create or repair the symlink declared by one `symlink` action. Return whether it created or
/// replaced the link.
pub(super) fn link(
    action: &SymlinkAction,
    tree: Tree<&ItemId>,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    require_symlink_support("symlink")?;

    let dest = context.destination(&fields.field("dest", &action.dest, ValueRule::Destination)?);
    let target = context.source(tree, &action.source)?;
    link_one(&target, &dest, context)
}

/// Carry out one `symlink-dir` action: one link per direct child of a
/// directory, all of them in one destination directory. Return whether it created or replaced
/// a link.
pub(super) fn link_dir(
    action: &SymlinkDirAction,
    tree: Tree<&ItemId>,
    fields: &Resolver<'_, '_>,
    context: &RunContext,
) -> Result<bool, Error> {
    require_symlink_support("symlink-dir")?;

    let dest_dir = fields.field("dest-dir", &action.dest_dir, ValueRule::Destination)?;
    let dest_dir = context.destination(&dest_dir);
    let source_dir = context.source_directory(tree, &action.source_dir)?;
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    let mut filter = EntryFilter::new(action.include.as_ref(), action.exclude.as_ref());
    let children = for_each_child(
        context,
        &ChildInstall {
            source_dir: &source_dir,
            dest_dir: &dest_dir,
            dot_prefix: action.dot_prefix,
            verb: Verb::Link,
        },
        |child| {
            filter
                .as_mut()
                .is_none_or(|filter| filter.selects(Path::new(child)))
        },
        |source, dest| link_one(source, dest, context),
    )?;
    if let Some(filter) = filter.as_ref().filter(|_| children.enumerated) {
        filter.report_unmatched(&source_dir.display().to_string(), context.reporter());
    }
    Ok(children.changed)
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
/// in the way under the run's conflict policy. Return whether the link was created or
/// replaced.
fn link_one(target: &Path, dest: &Path, context: &RunContext) -> Result<bool, Error> {
    let reporter = context.reporter();
    let mode = context.mode();
    let resolver = context.conflict_resolver();
    match Occupancy::at(dest, context.repository())? {
        Occupancy::Replaceable { points_at, .. }
            if points_at == paths::canonicalize_or_normalize(target) =>
        {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
            Ok(false)
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
            Ok(true)
        }
        Occupancy::Vacant => {
            paths::refuse_destination_inside_source(target, dest)?;
            // Resolve the conflict before creating parent directories.
            if !directory::create_parents(dest, &resolver)? {
                return Ok(false);
            }
            if mode.writes() {
                create(target, dest)?;
            }
            report_link(target, dest, context);
            Ok(true)
        }
        Occupancy::Unmanaged(found) => {
            paths::refuse_destination_inside_source(target, dest)?;
            paths::refuse_setting_aside_a_source(target, dest)?;
            match resolver.resolve(dest, &found)? {
                ConflictDecision::Refuse => Err(Error::DestinationExists {
                    path: dest.to_path_buf(),
                    found,
                }),
                ConflictDecision::Skip => Ok(false),
                ConflictDecision::Replace(keep) => {
                    resolver.replace(dest, keep, || {
                        if mode.writes() {
                            create(target, dest)?;
                        }
                        Ok(())
                    })?;
                    report_link(target, dest, context);
                    Ok(true)
                }
            }
        }
    }
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
    symlink(target, dest).map_err(Error::writing(dest))
}

fn remove(dest: &Path) -> Result<(), Error> {
    fs::remove_file(dest).map_err(Error::writing(dest))
}
