//! `copy` and `copy-dir`: the same seed, made once or once per child.
//!
//! Both end at [`install::seed`], which is where the missing-only rule and rule
//! 15 live. What is left here is which node each action hands it and how that
//! node was named — a source the manifest wrote is followed through a final
//! link, and one found inside a directory being copied is not.

use std::path::Path;

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::error::Error;
use crate::install;
use crate::manifest::action::{CopyAction, CopyDirAction};
use crate::mode::Verb;
use crate::paths;

/// Carry out one `copy` action: one file or one directory, at one destination.
///
/// A seed, so the destination decides everything: something there means the
/// action is done, and a directory source is installed whole or not at all.
pub(super) fn copy(action: &CopyAction, context: &RunContext) -> Result<(), Error> {
    let source = context.source(&action.source)?;
    let dest = context.destination(&action.dest);
    install::seed(
        &source,
        install::kind_of_named_source(&source)?,
        &dest,
        context.mode(),
        context.reporter(),
    )
}

/// Carry out one `copy-dir` action: one copy per direct child of a directory,
/// all of them in one destination directory.
///
/// A child that is itself a directory is one thing installed, whole where
/// nothing is there and untouched where something is. Nothing decides entry by
/// entry inside a child, so a directory the user already has is never seeded
/// into.
pub(super) fn copy_dir(action: &CopyDirAction, context: &RunContext) -> Result<(), Error> {
    let source_dir = context.source_directory(&action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    // Before the destination is created, because creating it inside the source
    // is what puts it in the list of children about to be copied. Each child is
    // checked again on its own; this one names the two directories the manifest
    // wrote, which is what the author can act on.
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    for_each_child(
        context,
        &ChildInstall {
            source_dir: &source_dir,
            dest_dir: &dest_dir,
            dot_prefix: action.dot_prefix,
            verb: Verb::Copy,
        },
        |source, dest| seed_child(source, dest, context),
    )
}

/// Seed one child of a `source-dir`.
///
/// Classified without following anything, because a child is a node found
/// rather than a path the manifest wrote.
fn seed_child(source: &Path, dest: &Path, context: &RunContext) -> Result<(), Error> {
    install::seed(
        source,
        install::kind_of_found_node(source)?,
        dest,
        context.mode(),
        context.reporter(),
    )
}
