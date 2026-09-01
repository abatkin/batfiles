//! Rule 15 in one place: install completely or not at all.
//!
//! A thing is built beside its destination and moved in with one call, so the
//! destination never holds half of it. Every seed-style action ends here —
//! `copy` today, `fetch-url` and archive extraction at 4.1 and 4.2 — which is
//! why this is a peer of the actions rather than a part of one.
//!
//! Producing the content is the half that is still `copy`'s. There is one
//! producer, so [`fill`] names it directly; 4.1 is the second caller and the
//! step that makes it a parameter.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::directory;
use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths;

/// What `copy` reproduces. Nothing else is installed by copying it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FileOrDirectory {
    File,
    Directory,
}

/// Install one thing where nothing is, or keep what is there, and say which.
///
/// The whole of the missing-only rule as a user sees it: `copy` reaches this
/// once for its `dest`, and `copy-dir` once per child. What occupies a
/// destination is never examined, because nothing here would replace it
/// whatever it turned out to be.
///
/// **Under [`RunMode::DryRun`] no staging node is created.** The mode is read
/// ahead of the staging path rather than around it, so everything below is
/// unreachable in that mode rather than merely unused.
pub(crate) fn seed(
    source: &Path,
    kind: FileOrDirectory,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error> {
    // Asked before the copy so the ordinary case — everything already seeded —
    // costs one call and copies nothing. The answer that decides is the one
    // taken when the copy is published.
    let installed = if paths::occupied(dest)? {
        false
    } else {
        // After that, not before: this refuses a destination the copy would
        // descend into, which is only a question when there is going to be a
        // copy. Asked first, it turns a destination that is merely *occupied*
        // — by a link of the user's own resolving into the source, say — into
        // an error, and a seed does not fail on an occupied destination.
        if let FileOrDirectory::Directory = kind {
            paths::refuse_destination_inside_source(source, dest)?;
        }
        for link in directory::create_parents(dest, mode)?.removals() {
            reporter.info(&link.removal_note(mode));
        }
        // The destination was free a moment ago, so a real run would copy into
        // it. Whether it still would be at the end is what only a real run
        // finds out.
        if mode.writes() {
            build_and_publish(source, kind, dest, reporter)?
        } else {
            true
        }
    };

    if installed {
        reporter.info(&format!(
            "{} {} from {}",
            Verb::Copy.say(mode),
            dest.display(),
            source.display()
        ));
    } else {
        reporter.detail(1, &format!("{} {}", Verb::Keep.say(mode), dest.display()));
    }
    Ok(())
}

/// Build a copy beside its destination, then move it in.
///
/// **Nothing is ever at the destination until the copy is whole.** Not a
/// partial copy, and not a placeholder standing in for one: the destination
/// stays absent, and the last thing this does is one rename that puts a
/// finished copy there.
///
/// That property has to hold with no cleanup at all, because a run that is
/// killed runs none (`guidance.md`, rule 15). So it is the arrangement that
/// buys it, not [`discard`].
///
/// Reports whether it installed: a destination taken while the copy was being
/// made is left alone, like one that was taken before it started.
fn build_and_publish(
    source: &Path,
    kind: FileOrDirectory,
    dest: &Path,
    reporter: &Reporter,
) -> Result<bool, Error> {
    let staging = staging_path(dest);
    // Created before anything else can fail, so that everything after it is
    // working on a node this run made. Cleanup that runs on a path this run did
    // not create is how a copy comes to delete somebody's data: the staging
    // path is predictable, and `remove_dir_all` on one that was already there
    // takes the tree with it.
    let staged = create_staging(kind, &staging)?;

    let installed = fill(source, staged, &staging).and_then(|()| publish(&staging, kind, dest));

    // Whatever happened, the staging path is not wanted: a successful rename
    // has already consumed it, a successful link has left a second name for it,
    // and a failure has left a copy that is not going anywhere. Best-effort,
    // and allowed to fail — what survives is beside the destination rather than
    // at it, so the next run copies again rather than mistaking it for
    // finished.
    discard(&staging, kind, reporter);
    installed
}

/// Create the node the copy is built on, and nothing more.
///
/// A staging path that is already taken belongs to somebody — most likely an
/// earlier run of batfiles, but that is a guess, and acting on it would mean
/// deleting a path this run did not create. So it is named and the action
/// stops.
fn create_staging(kind: FileOrDirectory, staging: &Path) -> Result<Staged, Error> {
    create_closed(kind, staging).map_err(|error| {
        if error.kind() == io::ErrorKind::AlreadyExists {
            Error::StagingPathTaken {
                path: staging.to_path_buf(),
            }
        } else {
            Error::Write {
                path: staging.to_path_buf(),
                source: error,
            }
        }
    })
}

/// The staging node this run created, ready to receive the copy.
enum Staged {
    File(fs::File),
    Directory,
}

/// Create a staging node no one but its owner can reach into.
///
/// Created closed and widened at the end, so a copy of a private file is never
/// briefly a public one (`guidance.md`, rule 15). The real permissions arrive
/// with [`mirror_permissions`] once the copy is whole; that ordering is what
/// keeps a source directory its owner cannot write into from locking batfiles
/// out of the copy it is still filling. One restrictive mode on the root covers
/// a whole staging tree, since everything under it is reached through it.
#[cfg(unix)]
fn create_closed(kind: FileOrDirectory, staging: &Path) -> io::Result<Staged> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    match kind {
        FileOrDirectory::File => fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(staging)
            .map(Staged::File),
        FileOrDirectory::Directory => fs::DirBuilder::new()
            .mode(0o700)
            .create(staging)
            .map(|()| Staged::Directory),
    }
}

/// Where a mode means something other than it does on unix, this is ordinary
/// exclusive creation: the permissions batfiles carries across are the unix
/// ones, and there is nothing here to narrow.
#[cfg(not(unix))]
fn create_closed(kind: FileOrDirectory, staging: &Path) -> io::Result<Staged> {
    match kind {
        FileOrDirectory::File => create_new(staging).map(Staged::File),
        FileOrDirectory::Directory => fs::create_dir(staging).map(|()| Staged::Directory),
    }
}

/// Make the copy itself, at the path it is built on the way to its destination.
fn fill(source: &Path, staged: Staged, staging: &Path) -> Result<(), Error> {
    match staged {
        Staged::File(into) => copy_file(source, into, staging),
        Staged::Directory => copy_children(source, staging),
    }
}

/// Move a finished copy to its destination, or report that the destination was
/// taken while it was being made.
///
/// A file is published by linking it, the one operation the standard library
/// offers that *refuses* to replace. Rename makes no such promise, so a
/// destination that appeared during a long copy would be overwritten by one,
/// and rule 13 does not stop applying because another process was quick.
///
/// A directory has neither a linkable equivalent nor a portable no-replace
/// rename, so it is checked again immediately before the rename. That leaves a
/// window of two adjacent calls, and a rename replaces only an *empty*
/// directory, so no content is at risk either way. Step 9.5 owns closing it.
fn publish(staging: &Path, kind: FileOrDirectory, dest: &Path) -> Result<bool, Error> {
    if let FileOrDirectory::File = kind {
        match fs::hard_link(staging, dest) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            // Not every filesystem has links. Where there are none, the rename
            // below is what is left.
            Err(_) => {}
        }
    }
    if paths::occupied(dest)? {
        return Ok(false);
    }
    fs::rename(staging, dest).map_err(|error| Error::Write {
        path: dest.to_path_buf(),
        source: error,
    })?;
    Ok(true)
}

/// Remove a staging node this run created, saying so if it cannot.
///
/// Only ever called on a path [`create_staging`] made, which is what makes it
/// safe. A path that is already gone is the ordinary case after a rename.
fn discard(staging: &Path, kind: FileOrDirectory, reporter: &Reporter) {
    let removed = match kind {
        FileOrDirectory::File => fs::remove_file(staging),
        FileOrDirectory::Directory => fs::remove_dir_all(staging),
    };
    match removed {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => reporter.warn(&format!(
            "could not remove the incomplete copy at {}: {error}",
            staging.display()
        )),
    }
}

/// Create a file, failing rather than truncating if the path is taken.
fn create_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

/// [`create_new`], for the paths inside a copy being built.
///
/// Those are all under the staging node this run just made, so a name already
/// taken there is a failure rather than something to keep — keeping one would
/// publish an incomplete copy as a finished one.
fn create_new_file(path: &Path) -> Result<fs::File, Error> {
    create_new(path).map_err(|error| Error::Write {
        path: path.to_path_buf(),
        source: error,
    })
}

/// Where a copy is built while it is still incomplete.
///
/// Beside the destination, so the move into place stays within one filesystem.
/// The name is fixed rather than carrying a process id: batfiles will not
/// remove what it did not create, so a copy left behind by an interrupted run
/// stops the next one with a diagnostic naming the path. That is the point — a
/// name that varied would quietly accumulate leftovers instead, and clearing
/// one to get out of the way is the user's call, not batfiles'.
fn staging_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".batfiles-incomplete");
    dest.with_file_name(name)
}

/// Classify a source the manifest named, following a final symlink.
///
/// Naming a thing and reproducing one are different questions, and every
/// action's source resolves through a link — that is how a repository points at
/// something it stores under another name.
pub(crate) fn kind_of_source(source: &Path) -> Result<FileOrDirectory, Error> {
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
pub(crate) fn kind_of_child(source: &Path) -> Result<FileOrDirectory, Error> {
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
/// destination only when [`publish`] moves it there whole.
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
