//! `copy` and `copy-dir`: the same seed, made once or once per child.
//!
//! Both end at [`install::seed`], which is where the missing-only rule and rule
//! 15 live. What is left here is which node each action hands it, how that node
//! was named — a source the manifest wrote is followed through a final link, and
//! one found inside a directory being copied is not — and the reproduction
//! itself, which is what fills the staging node `install` created.

use std::fs;
use std::io;
use std::path::Path;

use super::RunContext;
use super::children::{ChildInstall, for_each_child};
use crate::error::Error;
use crate::install::{self, FileOrDirectory, Staged};
use crate::manifest::action::{CopyAction, CopyDirAction};
use crate::output::Verb;
use crate::paths;

/// Carry out one `copy` action: one file or one directory, at one destination.
///
/// A seed, so the destination decides everything: something there means the
/// action is done, and a directory source is installed whole or not at all.
pub(super) fn copy(action: &CopyAction, context: &RunContext) -> Result<(), Error> {
    let source = context.source(&action.source)?;
    let dest = context.destination(&action.dest);
    seed(&source, kind_of_source(&source)?, &dest, context)
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
        |source, dest| seed(source, kind_of_child(source)?, dest, context),
    )
}

/// Seed one node by reproducing it, whichever action asked for it.
///
/// The one place that says what a copy fills its staging node with: a file is
/// written into the handle `install` opened, and a directory is walked into the
/// staging tree. Nothing here can reach the destination — [`install`] moves the
/// finished thing there, or does not.
fn seed(
    source: &Path,
    kind: FileOrDirectory,
    dest: &Path,
    context: &RunContext,
) -> Result<(), Error> {
    install::seed(
        install::Seed {
            kind,
            verb: Verb::Copy,
            origin: source.display().to_string(),
            // Only a directory can be descended into, so only a directory
            // source is a place a destination must not be.
            not_inside: matches!(kind, FileOrDirectory::Directory).then_some(source),
            // The parameter is annotated because the closure lives in a struct
            // field: without it, inference binds one lifetime rather than the
            // any-lifetime bound `seed` asks for.
            fill: |staged, staging: &Path| match staged {
                Staged::File(into) => copy_file(source, into, staging),
                Staged::Directory => copy_children(source, staging),
            },
        },
        dest,
        context.mode(),
        context.reporter(),
    )
}

/// Classify a source the manifest named, following a final symlink.
///
/// Naming a thing and reproducing one are different questions, and every
/// action's source resolves through a link — that is how a repository points at
/// something it stores under another name.
fn kind_of_source(source: &Path) -> Result<FileOrDirectory, Error> {
    let found = fs::metadata(source).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            // Reachability, not presence: the source has already been resolved
            // and found, so this is a link whose target is gone.
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

/// Classify a node found inside a directory being copied, following nothing.
///
/// A symlink here is refused rather than followed. `copy` reproduces nodes, and
/// a symlink is not one it will reproduce: copying what it reaches silently
/// turns a link the repository chose into a detached file, and recreating it
/// re-reads a relative target from a directory it is no longer in. Refusing is
/// the answer that can be changed later without changing what a working
/// manifest does today.
fn kind_of_child(source: &Path) -> Result<FileOrDirectory, Error> {
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

/// The tail both of the above share, once each has decided what to inspect.
fn classify(found: &fs::Metadata, source: &Path) -> Result<FileOrDirectory, Error> {
    if found.is_file() {
        Ok(FileOrDirectory::File)
    } else if found.is_dir() {
        Ok(FileOrDirectory::Directory)
    } else {
        Err(Error::SourceNotCopyable {
            path: source.to_path_buf(),
        })
    }
}

/// Write a source file's contents into the file already opened for it.
///
/// Not [`fs::copy`], which opens the destination itself and would truncate
/// whatever it found. The file is handed in already created exclusively, so the
/// only thing this can write into is one that did not exist a moment ago. The
/// permissions [`fs::copy`] would have carried are set here instead, which is
/// also where a directory gets them.
///
/// `built_at` is where `into` lives, which is never the action's destination:
/// everything this writes is inside the staging tree, and reaches the
/// destination only when [`install`] moves it there whole.
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
///
/// What it walks, it reproduces, and every node is new: `built_at` is inside the
/// staging directory, which nothing else knows about, so a path already taken is
/// a failure rather than a thing to keep. Keeping one here would publish an
/// incomplete copy as a finished one, which is the opposite of what staging is
/// for.
fn copy_children(source: &Path, built_at: &Path) -> Result<(), Error> {
    for child in paths::children_of(source)? {
        let from = source.join(&child);
        let to = built_at.join(&child);
        match kind_of_child(&from)? {
            FileOrDirectory::File => {
                copy_file(&from, create_new_file(&to)?, &to)?;
            }
            FileOrDirectory::Directory => {
                fs::create_dir(&to).map_err(|error| Error::Write {
                    path: to.clone(),
                    source: error,
                })?;
                copy_children(&from, &to)?;
            }
        }
    }
    // Last, and not at creation: a source directory its owner cannot write into
    // would otherwise lock batfiles out of the copy it is still filling.
    mirror_permissions(source, built_at)
}

/// Create one of the files inside a copy being built, failing rather than
/// truncating if the path is taken.
///
/// Those are all under the staging node this run just made, so a name already
/// taken there is a failure rather than something to keep — keeping one would
/// publish an incomplete copy as a finished one.
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

/// Give a copied file or directory the permissions of what it was copied from,
/// so an executable arrives executable and a private directory arrives private.
///
/// Ownership is not copied; the copy belongs to whoever ran the command.
/// Directories batfiles creates only to *reach* a destination are not these,
/// and keep the platform default: they correspond to nothing in the repository.
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
