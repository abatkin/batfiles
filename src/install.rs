//! Stage and publish seed content, and rebuild tool-owned content, without
//! exposing partial installations. Temporary paths are created exclusively and
//! cleanup only removes owned paths.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::directory;
use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths;

/// The kind of node a seed installs, for a caller that settles it at runtime
/// and then picks the entry point it answers to.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SeedKind {
    File,
    Directory,
}

/// A seed's reporting fields and optional source-containment constraint.
pub(crate) struct Seed<'a> {
    /// How the report names what happened: `Verb::Copy` for a copy,
    /// `Verb::Fetch` for a download.
    pub verb: Verb,

    /// Where the content comes from, as the report writes it — a repository
    /// path or a URL.
    pub origin: String,

    /// A source directory the destination must not be inside of, checked once
    /// the destination is known to be free.
    pub source_directory: Option<&'a Path>,
}

/// Seed one file: `fill` is handed the staging file, opened for writing and
/// reachable by nobody else, and the path it is at for diagnostics.
///
/// `fill` runs only in perform mode. An error prevents publication and triggers
/// best-effort cleanup.
pub(crate) fn seed_file(
    what: Seed<'_>,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    fill: impl FnOnce(fs::File, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    seed(
        what,
        SeedKind::File,
        dest,
        mode,
        reporter,
        create_private_file,
        fill,
    )
}

/// Seed one directory: `fill` is handed the staging directory's path, created
/// private to this run, and fills it with the content that belongs there.
///
/// `fill` runs only in perform mode. An error prevents publication and triggers
/// best-effort cleanup.
pub(crate) fn seed_directory(
    what: Seed<'_>,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    fill: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    seed(
        what,
        SeedKind::Directory,
        dest,
        mode,
        reporter,
        create_private_directory,
        |(), staging| fill(staging),
    )
}

/// Keep an occupied destination, or build and publish a complete seed.
/// Checks directory containment and creates missing parents before staging.
/// Dry runs report intent without creating parents, staging files, or content.
///
/// Shared by both entry points: `make` creates the staging node and `fill`
/// fills it.
fn seed<T>(
    what: Seed<'_>,
    kind: SeedKind,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    let installed = if paths::occupied(dest)? {
        false
    } else {
        if let Some(source) = what.source_directory {
            paths::refuse_destination_inside_source(source, dest)?;
        }
        directory::create_parents(dest, mode)?.report_removals(mode, reporter);
        if mode.writes() {
            build_and_publish(kind, dest, reporter, make, fill)?
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

/// Build tool-owned content beside `dest` and put it there, replacing without a
/// backup whatever is there already: the caller must have established that it
/// is an earlier build of its own. `fill` is handed the staging file, opened for
/// writing and reachable by nobody else, and the path it is at.
///
/// Creates missing parents. Reports nothing; the caller words the result. Dry
/// runs create nothing and never call `fill`.
pub(crate) fn rebuild_file(
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    fill: impl FnOnce(fs::File, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    rebuild(
        SeedKind::File,
        dest,
        mode,
        reporter,
        create_private_file,
        fill,
    )
}

/// [`rebuild_file`] for a directory: `fill` is handed the staging directory's
/// path, created private to this run.
pub(crate) fn rebuild_directory(
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    fill: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    rebuild(
        SeedKind::Directory,
        dest,
        mode,
        reporter,
        create_private_directory,
        |(), staging| fill(staging),
    )
}

/// Build complete content at the staging path, then publish it at a vacant
/// `dest` or [swap](swap) it for the node there. A failure before the swap
/// leaves `dest` as it was.
fn rebuild<T>(
    kind: SeedKind,
    dest: &Path,
    mode: RunMode,
    reporter: &Reporter,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    let replacing = paths::occupied(dest)?;
    if !replacing {
        directory::create_parents(dest, mode)?.report_removals(mode, reporter);
    }
    if !mode.writes() {
        return Ok(());
    }
    let staging = staging_path(dest);
    let node = create_staging(&staging, make)?;
    let built = fill(node, &staging).and_then(|()| {
        if replacing {
            swap(&staging, dest, reporter)
        } else if publish(&staging, kind, dest)? {
            Ok(())
        } else {
            // Something arrived at `dest` while the content was being built,
            // and it is nobody's to replace.
            Err(Error::Write {
                path: dest.to_path_buf(),
                source: io::ErrorKind::AlreadyExists.into(),
            })
        }
    });
    discard(&staging, kind, reporter);
    built
}

/// Put complete staged content at `dest` in place of the node there: move that
/// node aside, rename the staged content in, then remove what was moved aside.
/// If the rename fails, the earlier node is moved back.
///
/// The aside path is `<dest>.batfiles-old`; an occupied one fails before
/// anything moves, as an occupied staging path does.
fn swap(staging: &Path, dest: &Path, reporter: &Reporter) -> Result<(), Error> {
    let aside = beside(dest, ".batfiles-old");
    if paths::occupied(&aside)? {
        return Err(Error::StagingPathTaken { path: aside });
    }
    let failed = |source| Error::Write {
        path: dest.to_path_buf(),
        source,
    };
    fs::rename(dest, &aside).map_err(failed)?;
    if let Err(error) = fs::rename(staging, dest) {
        let _ = fs::rename(&aside, dest);
        return Err(failed(error));
    }
    let removed = match fs::symlink_metadata(&aside) {
        Ok(found) if found.is_dir() => fs::remove_dir_all(&aside),
        Ok(_) => fs::remove_file(&aside),
        Err(error) => Err(error),
    };
    if let Err(error) = removed {
        reporter.warn(&format!(
            "could not remove the replaced content at {}: {error}",
            aside.display()
        ));
    }
    Ok(())
}

/// Build content beside its destination and publish it when complete.
/// Returns false if publication finds the destination occupied. Cleanup is
/// best-effort and restricted to the staging node created by this call.
///
/// The node is created before any cleanup can run: a path already taken fails
/// creation and is left untouched, so cleanup removes only what this call
/// created.
fn build_and_publish<T>(
    kind: SeedKind,
    dest: &Path,
    reporter: &Reporter,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<bool, Error> {
    let staging = staging_path(dest);
    let node = create_staging(&staging, make)?;

    let installed = fill(node, &staging).and_then(|()| publish(&staging, kind, dest));

    discard(&staging, kind, reporter);
    installed
}

/// Exclusively create a private staging node, `make` deciding which kind. An
/// occupied path is an error, since the node it would build in is not this
/// run's to fill.
fn create_staging<T>(
    staging: &Path,
    make: impl FnOnce(&Path) -> io::Result<T>,
) -> Result<T, Error> {
    make(staging).map_err(|error| {
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

/// Create a staging file no one but its owner can read or write.
#[cfg(unix)]
fn create_private_file(staging: &Path) -> io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;

    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(staging)
}

/// Create a staging directory no one but its owner can reach into.
#[cfg(unix)]
fn create_private_directory(staging: &Path) -> io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;

    fs::DirBuilder::new().mode(0o700).create(staging)
}

/// Where a mode means something other than it does on unix, this is ordinary
/// exclusive creation: the permissions batfiles carries across are the unix
/// ones, and there is nothing here to narrow.
#[cfg(not(unix))]
fn create_private_file(staging: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(staging)
}

/// The same, for a directory: exclusive creation and nothing to narrow.
#[cfg(not(unix))]
fn create_private_directory(staging: &Path) -> io::Result<()> {
    fs::create_dir(staging)
}

/// Publish complete staged content; return false if the destination is occupied.
/// Files use a hard link where supported. The rename fallback and directory
/// publication have a race between the occupancy check and rename.
fn publish(staging: &Path, kind: SeedKind, dest: &Path) -> Result<bool, Error> {
    if let SeedKind::File = kind {
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
fn discard(staging: &Path, kind: SeedKind, reporter: &Reporter) {
    let removed = match kind {
        SeedKind::File => fs::remove_file(staging),
        SeedKind::Directory => fs::remove_dir_all(staging),
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

/// Lend a callback an exclusively created scratch file beside `dest`.
/// Remove the scratch file on return, including failure, with best-effort cleanup.
pub(crate) fn with_scratch<T>(
    dest: &Path,
    reporter: &Reporter,
    work: impl FnOnce(&mut Scratch) -> Result<T, Error>,
) -> Result<T, Error> {
    let path = scratch_path(dest);
    let mut scratch = Scratch {
        file: create_staging(&path, create_private_file)?,
        path,
    };
    let done = work(&mut scratch);
    discard(&scratch.path, SeedKind::File, reporter);
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
