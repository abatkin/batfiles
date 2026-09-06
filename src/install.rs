//! Stage and publish seed content without exposing partial installations.
//! Temporary paths are created exclusively and cleanup only removes owned paths.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::directory;
use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths;

/// The kind of node to create for a seed installation.
#[derive(Debug, Clone, Copy)]
pub(crate) enum FileOrDirectory {
    File,
    Directory,
}

/// Content kind, reporting fields, and builder for a missing-only installation.
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
    pub source_directory: Option<&'a Path>,

    /// Fill the newly created staging node. Called only in perform mode.
    /// An error prevents publication and triggers best-effort cleanup.
    pub fill: F,
}

/// Keep an occupied destination, or build and publish a complete seed.
/// Checks directory containment and creates missing parents before staging.
/// Dry runs report intent without creating parents, staging files, or content.
pub(crate) fn seed<F>(
    what: Seed<'_, F>,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
) -> Result<(), Error>
where
    F: FnOnce(Staged, &Path) -> Result<(), Error>,
{
    let installed = if paths::occupied(dest)? {
        false
    } else {
        if let Some(source) = what.source_directory {
            paths::refuse_destination_inside_source(source, dest)?;
        }
        for link in directory::create_parents(dest, mode)?.removals() {
            reporter.info(&link.removal_note(mode));
        }
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

/// Build content beside its destination and publish it when complete.
/// Returns false if publication finds the destination occupied. Cleanup is
/// best-effort and restricted to the staging node created by this call.
fn build_and_publish(
    kind: FileOrDirectory,
    dest: &Path,
    reporter: &Reporter,
    fill: impl FnOnce(Staged, &Path) -> Result<(), Error>,
) -> Result<bool, Error> {
    let staging = staging_path(dest);
    let staged = create_staging(kind, &staging)?;

    let installed = fill(staged, &staging).and_then(|()| publish(&staging, kind, dest));

    discard(&staging, kind, reporter);
    installed
}

/// Exclusively create a private staging node. An occupied path is an error.
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
pub(crate) enum Staged {
    File(fs::File),
    Directory,
}

impl Staged {
    /// Return the staged file handle. Panics if this is a directory; the caller
    /// must supply a file builder only with `Seed::kind = FileOrDirectory::File`.
    pub fn into_file(self) -> fs::File {
        match self {
            Self::File(file) => file,
            Self::Directory => panic!("a seed declaring a file is handed a file"),
        }
    }
}

/// Create a staging node no one but its owner can reach into.
#[cfg(unix)]
fn create_closed(kind: FileOrDirectory, staging: &Path) -> io::Result<Staged> {
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

    match kind {
        FileOrDirectory::File => fs::OpenOptions::new()
            .read(true)
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

/// Publish complete staged content; return false if the destination is occupied.
/// Files use a hard link where supported. The rename fallback and directory
/// publication have a race between the occupancy check and rename.
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
fn discard(staging: &Path, kind: FileOrDirectory, reporter: &Reporter) {
    let removed = match kind {
        FileOrDirectory::File => fs::remove_file(staging),
        FileOrDirectory::Directory => fs::remove_dir_all(staging),
    };
    match removed {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => reporter.warn(&format!(
            "could not remove the incomplete work at {}: {error}",
            staging.display()
        )),
    }
}

/// Create a file, failing rather than truncating if the path is taken.
#[cfg(not(unix))]
fn create_new(path: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)
}

/// Lend a callback an exclusively created scratch file beside `dest`.
/// Remove the scratch file on return, including failure, with best-effort cleanup.
pub(crate) fn with_scratch<T>(
    dest: &Path,
    reporter: &Reporter,
    work: impl FnOnce(&mut Scratch) -> Result<T, Error>,
) -> Result<T, Error> {
    let path = scratch_path(dest);
    let mut scratch = Scratch {
        file: create_staging(FileOrDirectory::File, &path)?.into_file(),
        path,
    };
    let done = work(&mut scratch);
    discard(&scratch.path, FileOrDirectory::File, reporter);
    done
}

/// A scratch file this run created beside a destination.
pub(crate) struct Scratch {
    path: PathBuf,
    file: fs::File,
}

impl Scratch {
    /// Where it is, for naming it in a diagnostic.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the open scratch file. Reads share its cursor with writes.
    pub fn file(&self) -> &fs::File {
        &self.file
    }
}

impl io::Write for Scratch {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Where a copy is built while it is still incomplete.
fn staging_path(dest: &Path) -> PathBuf {
    beside(dest, ".batfiles-incomplete")
}

/// Where content that has to arrive whole is downloaded to.
fn scratch_path(dest: &Path) -> PathBuf {
    beside(dest, ".batfiles-download")
}

/// Append a suffix to the destination filename to name a sibling.
fn beside(dest: &Path, suffix: &str) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    dest.with_file_name(name)
}
