//! `copy` and `copy-dir`: seed destinations from a source node or its direct children.

use std::fs;
use std::io;
use std::path::Path;

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::error::Error;
use crate::install::{self, ContentKind};
use crate::item::ItemId;
use crate::manifest::action::{CopyAction, CopyDirAction};
use crate::output::Verb;
use crate::paths;

/// Carry out one `copy` action: one file or one directory, at one destination.
pub(super) fn copy(
    action: &CopyAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source = context.source(remote, &action.source)?;
    let dest = context.destination(&action.dest);
    seed(&source, kind_of_source(&source)?, &dest, context)
}

/// Carry out one `copy-dir` action: one copy per direct child of a directory,
/// all of them in one destination directory.
pub(super) fn copy_dir(
    action: &CopyDirAction,
    remote: Option<&ItemId>,
    context: &RunContext,
) -> Result<(), Error> {
    let source_dir = context.source_directory(remote, &action.source_dir)?;
    let dest_dir = context.destination(&action.dest_dir);
    paths::refuse_destination_inside_source(&source_dir, &dest_dir)?;

    for_each_child(
        context,
        &ChildInstall {
            source_dir: &source_dir,
            dest_dir: &dest_dir,
            dot_prefix: action.dot_prefix,
            verb: Verb::Copy,
        },
        |source, dest| seed(source, kind_of_child(source)?, dest, context),
    )
}

/// Seed `dest` with a copy of `source`, using its classified content kind.
fn seed(source: &Path, kind: ContentKind, dest: &Path, context: &RunContext) -> Result<(), Error> {
    let seed = install::SeedDescription {
        verb: Verb::Copy,
        origin: source.display().to_string(),
        source: Some(source),
    };
    match kind {
        ContentKind::File => install::seed_file(seed, dest, context, |into, staging| {
            copy_file(source, into, staging)
        }),
        ContentKind::Directory => install::seed_directory(seed, dest, context, |staging| {
            copy_children(source, staging)
        }),
    }
}

/// Classify a source the manifest named, following a final symlink.
fn kind_of_source(source: &Path) -> Result<ContentKind, Error> {
    let found = fs::metadata(source).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            Error::SourceMissing {
                path: source.to_path_buf(),
            }
        } else {
            Error::Read {
                path: source.to_path_buf(),
                source: error,
            }
        }
    })?;
    classify(&found, source)
}

/// Classify a copied directory child without following symlinks.
fn kind_of_child(source: &Path) -> Result<ContentKind, Error> {
    let found = fs::symlink_metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    if found.is_symlink() {
        return Err(Error::SourceIsSymlink {
            path: source.to_path_buf(),
        });
    }
    classify(&found, source)
}

/// Classify metadata as a file or directory; reject other node types.
fn classify(found: &fs::Metadata, source: &Path) -> Result<ContentKind, Error> {
    if found.is_file() {
        Ok(ContentKind::File)
    } else if found.is_dir() {
        Ok(ContentKind::Directory)
    } else {
        Err(Error::SourceNotCopyable {
            path: source.to_path_buf(),
        })
    }
}

/// Write a source file's contents into the file already opened for it.
fn copy_file(source: &Path, mut into: fs::File, built_at: &Path) -> Result<(), Error> {
    let mut from = fs::File::open(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    io::copy(&mut from, &mut into).map_err(|error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    })?;
    mirror_permissions(source, built_at)
}

/// Copy everything under a source directory into a directory being built.
fn copy_children(source: &Path, built_at: &Path) -> Result<(), Error> {
    for child in paths::children_of(source)? {
        let from = source.join(&child);
        let to = built_at.join(&child);
        match kind_of_child(&from)? {
            ContentKind::File => {
                copy_file(&from, create_new_file(&to)?, &to)?;
            }
            ContentKind::Directory => {
                fs::create_dir(&to).map_err(|error| Error::Write {
                    path: to.clone(),
                    source: error,
                })?;
                copy_children(&from, &to)?;
            }
        }
    }
    // Apply directory permissions after filling it; the source may be read-only.
    mirror_permissions(source, built_at)
}

/// Create one of the files inside a copy being built, failing rather than
/// truncating if the path is taken.
fn create_new_file(path: &Path) -> Result<fs::File, Error> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| Error::Write {
            path: path.to_path_buf(),
            source: error,
        })
}

/// Apply the source file or directory permissions to the copy.
fn mirror_permissions(source: &Path, built_at: &Path) -> Result<(), Error> {
    let found = fs::metadata(source).map_err(|error| Error::Read {
        path: source.to_path_buf(),
        source: error,
    })?;
    fs::set_permissions(built_at, found.permissions()).map_err(|error| Error::Write {
        path: built_at.to_path_buf(),
        source: error,
    })
}
