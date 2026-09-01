//! Rule 15 in one place: install completely or not at all.
//!
//! A thing is built beside its destination and moved in with one call, so the
//! destination never holds half of it. Every seed-style action ends here —
//! `copy` and `fetch-file` today, `fetch-archive` at 4.2 — which is why this
//! is a peer of the actions rather than a part of one.
//!
//! Producing the content is the caller's half: what fills the staging node this
//! module created arrives as a closure, so a copy and a download reach the
//! destination by the same route and neither is named here.

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

/// One thing to install where nothing is, described by the action installing it.
///
/// Everything that differs between a copy and a download, and nothing that does
/// not: how it is built, what it is called in the report, and the one check that
/// only applies when the content comes from a directory on this machine. The
/// route to the destination is the same for both and is not described here.
pub(crate) struct Seed<'a, F> {
    /// Whether a file or a directory is being installed, which decides how the
    /// staging node is made and how it is published.
    pub kind: FileOrDirectory,

    /// How the report names what happened: `Verb::Copy` for a copy,
    /// `Verb::Fetch` for a download.
    pub verb: Verb,

    /// Where the content comes from, as the report writes it — a repository
    /// path or a URL.
    pub origin: String,

    /// A source directory the destination must not be inside of, checked once
    /// the destination is known to be free.
    ///
    /// `Some` only for a directory copy, which would otherwise descend into
    /// what it was writing. A download has nothing on this machine for a
    /// destination to be inside of.
    pub not_inside: Option<&'a Path>,

    /// What writes the content into the staging node this run created.
    ///
    /// Reached only when the mode allows a write, so nothing behind it has to
    /// ask about the mode. It is handed the node and the path the node is at;
    /// the destination is not its to know.
    pub fill: F,
}

/// Install one thing where nothing is, or keep what is there, and say which.
///
/// The whole of the missing-only rule as a user sees it: `copy` and `fetch-file`
/// reach this once for their `dest`, and `copy-dir` once per child. What
/// occupies a destination is never examined, because nothing here would replace
/// it whatever it turned out to be.
///
/// **Under [`RunMode::DryRun`] no staging node is created.** The mode is read
/// ahead of the staging path rather than around it, so everything below — the
/// fillers included — is unreachable in that mode rather than merely unused.
pub(crate) fn seed<F>(
    what: Seed<'_, F>,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error>
where
    F: FnOnce(Staged, &Path) -> Result<(), Error>,
{
    // Asked before anything is built so the ordinary case — everything already
    // seeded — costs one call and installs nothing. The answer that decides is
    // the one taken when the finished thing is published.
    let installed = if paths::occupied(dest)? {
        false
    } else {
        // After that, not before: this refuses a destination the copy would
        // descend into, which is only a question when there is going to be a
        // copy. Asked first, it turns a destination that is merely *occupied*
        // — by a link of the user's own resolving into the source, say — into
        // an error, and a seed does not fail on an occupied destination.
        if let Some(source) = what.not_inside {
            paths::refuse_destination_inside_source(source, dest)?;
        }
        for link in directory::create_parents(dest, mode)?.removals() {
            reporter.info(&link.removal_note(mode));
        }
        // The destination was free a moment ago, so a real run would install
        // into it. Whether it still would be at the end is what only a real run
        // finds out.
        if mode.writes() {
            build_and_publish(what.kind, dest, reporter, what.fill)?
        } else {
            true
        }
    };

    if installed {
        reporter.info(&format!(
            "{} {} from {}",
            what.verb.say(mode),
            dest.display(),
            what.origin
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
    kind: FileOrDirectory,
    dest: &Path,
    reporter: &Reporter,
    fill: impl FnOnce(Staged, &Path) -> Result<(), Error>,
) -> Result<bool, Error> {
    let staging = staging_path(dest);
    // Created before anything else can fail, so that everything after it is
    // working on a node this run made. Cleanup that runs on a path this run did
    // not create is how a copy comes to delete somebody's data: the staging
    // path is predictable, and `remove_dir_all` on one that was already there
    // takes the tree with it.
    let staged = create_staging(kind, &staging)?;

    let installed = fill(staged, &staging).and_then(|()| publish(&staging, kind, dest));

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

/// The staging node this run created, ready to receive what is being installed.
///
/// Handed to a filler, which is the only thing that writes into it. A filler
/// holding one is proof that the mode allowed the write and that the node is
/// this run's own: nothing else can construct one.
pub(crate) enum Staged {
    File(fs::File),
    Directory,
}

impl Staged {
    /// The open handle, for a filler that only installs files.
    ///
    /// Which node was created is decided from the same [`Seed::kind`] the filler
    /// was written beside, so one declaring [`FileOrDirectory::File`] is handed
    /// a file. The panic states that agreement rather than defending against it
    /// — it is one struct literal apart — the way [`crate::location`] states its
    /// own resolved-root invariant. `fetch-archive` at 4.2 fills a directory and
    /// uses both arms.
    pub fn into_file(self) -> fs::File {
        match self {
            Self::File(file) => file,
            Self::Directory => panic!("a seed declaring a file is handed a file"),
        }
    }
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
#[cfg(not(unix))]
fn create_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
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
