//! Stage and publish seed content, refresh it, and rebuild tool-owned content,
//! without exposing partial installations. Temporary paths are created
//! exclusively and cleanup only removes owned paths.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::action::RunContext;
use crate::directory;
use crate::error::Error;
use crate::mode::RunMode;
use crate::output::{Reporter, Verb};
use crate::paths::{self, ExistingNode, Occupancy};
use crate::replace::{self, ConflictDecision, ConflictResolver, ExistingContent};

/// Whether seed content is a file or directory.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ContentKind {
    File,
    Directory,
}

/// A seed's reporting fields and optional source-containment constraint.
pub(crate) struct SeedDescription<'a> {
    /// Verb used to report the installation.
    pub verb: Verb,

    /// Where the content comes from, as the report writes it — a repository
    /// path or a URL.
    pub origin: String,

    /// Optional local source. A directory source must not contain the destination, and no
    /// source may be inside a destination that refresh replaces.
    pub source: Option<&'a Path>,
}

/// Seed a file using `fill`, which receives the open staging file and its path for diagnostics.
/// Call `fill` only in perform mode. On error, do not publish and attempt staging cleanup.
pub(crate) fn seed_file(
    seed: SeedDescription<'_>,
    dest: &Path,
    context: &RunContext,
    fill: impl FnOnce(fs::File, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    seed_content(
        seed,
        ContentKind::File,
        dest,
        context,
        create_private_file,
        fill,
    )
}

/// Seed a directory using `fill`, which receives the staging directory path. Call `fill` only
/// in perform mode. On error, do not publish and attempt staging cleanup.
pub(crate) fn seed_directory(
    seed: SeedDescription<'_>,
    dest: &Path,
    context: &RunContext,
    fill: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    seed_content(
        seed,
        ContentKind::Directory,
        dest,
        context,
        create_private_directory,
        |(), staging| fill(staging),
    )
}

/// Build and publish a seed, keeping occupied destinations unless content refresh is enabled.
/// Check source containment and create missing parents before staging. `make` creates the
/// staging node and `fill` populates it. Dry runs report intent without creating content or
/// parents.
fn seed_content<T>(
    seed: SeedDescription<'_>,
    kind: ContentKind,
    dest: &Path,
    context: &RunContext,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    let resolver = context.resolver();
    let (mode, reporter) = (context.mode(), context.reporter());
    // Skip rebuilding occupied seeds under `--no-overwrite`.
    let refreshing = context.refresh_content() && !resolver.conflicts().skips();
    let found = match Occupancy::at(dest, context.repository())? {
        Occupancy::Vacant => None,
        _ if !refreshing => {
            report_kept(dest, mode, reporter);
            return Ok(());
        }
        Occupancy::Replaceable { .. } => Some(OccupiedDestination::Replaceable),
        Occupancy::Unmanaged(found) => Some(OccupiedDestination::Unmanaged(found)),
    };
    if let (Some(source), ContentKind::Directory) = (seed.source, kind) {
        paths::refuse_destination_inside_source(source, dest)?;
    }
    let verb = match found {
        Some(existing) => {
            if let Some(source) = seed.source {
                paths::refuse_setting_aside_a_source(source, dest)?;
            }
            if !refresh(kind, dest, existing, &resolver, make, fill)? {
                return Ok(());
            }
            Verb::Refresh
        }
        None => {
            if !directory::create_parents(dest, &resolver)? {
                return Ok(());
            }
            if mode.writes() && !build_and_publish(kind, dest, reporter, make, fill)? {
                report_kept(dest, mode, reporter);
                return Ok(());
            }
            seed.verb
        }
    };
    reporter.info(&format!(
        "{} {} from {}",
        verb.for_mode(mode),
        dest.display(),
        seed.origin
    ));
    Ok(())
}

/// What a refresh finds at an occupied seed destination.
enum OccupiedDestination {
    /// A symlink holding no content of its own, replaced without a backup.
    Replaceable,
    /// Anything else, settled under the run's conflict policy.
    Unmanaged(ExistingNode),
}

fn report_kept(dest: &Path, mode: RunMode, reporter: &Reporter) {
    reporter.detail(
        1,
        &format!("{} {}", Verb::Keep.for_mode(mode), dest.display()),
    );
}

/// Rebuild content and replace `dest` if it differs, returning whether replacement occurred.
/// Resolve conflicts only after the new content is complete. Dry runs build nothing and report
/// a possible replacement without comparing content.
fn refresh<T>(
    kind: ContentKind,
    dest: &Path,
    existing: OccupiedDestination,
    resolver: &ConflictResolver<'_>,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<bool, Error> {
    let (mode, reporter) = (resolver.mode(), resolver.reporter());
    if !mode.writes() {
        return settle(dest, existing, resolver, || Ok(()));
    }
    let staging = staging_path(dest);
    let node = create_staging(&staging, make)?;
    let refreshed = fill(node, &staging).and_then(|()| {
        if let OccupiedDestination::Unmanaged(_) = existing
            && same_content(&staging, dest)?
        {
            reporter.detail(1, &format!("unchanged {}", dest.display()));
            return Ok(false);
        }
        settle(dest, existing, resolver, || {
            publish_vacated(&staging, kind, dest)
        })
    });
    discard(&staging, kind, reporter);
    refreshed
}

/// Settle what is at `dest` and, unless that was skipped, `install` in its
/// place. Answers whether it was installed.
fn settle(
    dest: &Path,
    existing: OccupiedDestination,
    resolver: &ConflictResolver<'_>,
    install: impl FnOnce() -> Result<(), Error>,
) -> Result<bool, Error> {
    let keep = match existing {
        OccupiedDestination::Replaceable => ExistingContent::Discard,
        OccupiedDestination::Unmanaged(found) => match resolver.resolve(dest, &found)? {
            ConflictDecision::Refuse => {
                return Err(Error::DestinationExists {
                    path: dest.to_path_buf(),
                    found,
                });
            }
            ConflictDecision::Skip => return Ok(false),
            ConflictDecision::Replace(keep) => keep,
        },
    };
    resolver.replace(dest, keep, install)?;
    Ok(true)
}

/// Whether the node at `built` and the node at `existing` hold the same
/// content: the same kind, the same permissions, the same bytes or link
/// target, and for a directory the same children holding the same content.
/// Follows no symlink.
fn same_content(built: &Path, existing: &Path) -> Result<bool, Error> {
    let inspect = |path: &Path| {
        fs::symlink_metadata(path).map_err(|source| Error::Read {
            path: path.to_path_buf(),
            source,
        })
    };
    let (ours, theirs) = (inspect(built)?, inspect(existing)?);
    if ours.file_type() != theirs.file_type() {
        return Ok(false);
    }
    if ours.is_symlink() {
        let target = |path: &Path| {
            fs::read_link(path).map_err(|source| Error::Read {
                path: path.to_path_buf(),
                source,
            })
        };
        return Ok(target(built)? == target(existing)?);
    }
    if ours.permissions() != theirs.permissions() {
        return Ok(false);
    }
    if ours.is_file() {
        return Ok(ours.len() == theirs.len() && same_bytes(built, existing)?);
    }
    if ours.is_dir() {
        let children = paths::children_of(built)?;
        if children != paths::children_of(existing)? {
            return Ok(false);
        }
        for child in children {
            if !same_content(&built.join(&child), &existing.join(&child))? {
                return Ok(false);
            }
        }
        return Ok(true);
    }
    Ok(false)
}

/// Whether two files of the same length hold the same bytes.
fn same_bytes(built: &Path, existing: &Path) -> Result<bool, Error> {
    let open = |path: &Path| {
        fs::File::open(path)
            .map(io::BufReader::new)
            .map_err(|source| Error::Read {
                path: path.to_path_buf(),
                source,
            })
    };
    let (mut ours, mut theirs) = (open(built)?, open(existing)?);
    let mut buffers = ([0_u8; 8192], [0_u8; 8192]);
    loop {
        let read = ours.read(&mut buffers.0).map_err(|source| Error::Read {
            path: built.to_path_buf(),
            source,
        })?;
        if read == 0 {
            return Ok(true);
        }
        theirs
            .read_exact(&mut buffers.1[..read])
            .map_err(|source| Error::Read {
                path: existing.to_path_buf(),
                source,
            })?;
        if buffers.0[..read] != buffers.1[..read] {
            return Ok(false);
        }
    }
}

/// Stage and replace a tool-owned file without a backup. The caller must establish ownership of
/// any existing destination. `fill` receives the private, open staging file and its path.
///
/// Create missing parents using `resolver`, which should refuse conflicts. Report only
/// parent-directory changes. Dry runs create nothing and never call `fill`.
pub(crate) fn rebuild_file(
    dest: &Path,
    resolver: &ConflictResolver<'_>,
    fill: impl FnOnce(fs::File, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    rebuild(ContentKind::File, dest, resolver, create_private_file, fill)
}

/// [`rebuild_file`] for a directory: `fill` is handed the staging directory's
/// path, created private to this run.
pub(crate) fn rebuild_directory(
    dest: &Path,
    resolver: &ConflictResolver<'_>,
    fill: impl FnOnce(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    rebuild(
        ContentKind::Directory,
        dest,
        resolver,
        create_private_directory,
        |(), staging| fill(staging),
    )
}

/// Build complete content at the staging path, then publish it at a vacant
/// `dest` or [swap] it for the node there. A failure before the swap
/// leaves `dest` as it was.
fn rebuild<T>(
    kind: ContentKind,
    dest: &Path,
    resolver: &ConflictResolver<'_>,
    make: impl FnOnce(&Path) -> io::Result<T>,
    fill: impl FnOnce(T, &Path) -> Result<(), Error>,
) -> Result<(), Error> {
    let (mode, reporter) = (resolver.mode(), resolver.reporter());
    let replacing = paths::occupied(dest)?;
    if !replacing && !directory::create_parents(dest, resolver)? {
        return Ok(());
    }
    if !mode.writes() {
        return Ok(());
    }
    let staging = staging_path(dest);
    let node = create_staging(&staging, make)?;
    let built = fill(node, &staging).and_then(|()| {
        if replacing {
            swap(&staging, kind, dest, reporter)
        } else {
            publish_vacated(&staging, kind, dest)
        }
    });
    discard(&staging, kind, reporter);
    built
}

/// Replace `dest` with staged content, moving the existing node to `<dest>.batfiles-old` first.
/// That path must be vacant. Remove the old node after success; attempt to restore it if
/// publication fails.
fn swap(staging: &Path, kind: ContentKind, dest: &Path, reporter: &Reporter) -> Result<(), Error> {
    let aside = paths::beside(dest, ".batfiles-old");
    if paths::occupied(&aside)? {
        return Err(Error::StagingPathTaken { path: aside });
    }
    replace::set_aside(dest, &aside)?;
    if let Err(error) = publish_vacated(staging, kind, dest) {
        return Err(replace::put_back(dest, &aside, error).0);
    }
    replace::remove_aside(&aside, reporter);
    Ok(())
}

/// Publish staged content at a destination the caller found or made vacant. Fail if the
/// pre-publication occupancy check finds a node there.
fn publish_vacated(staging: &Path, kind: ContentKind, dest: &Path) -> Result<(), Error> {
    if publish(staging, kind, dest)? {
        Ok(())
    } else {
        Err(Error::Write {
            path: dest.to_path_buf(),
            source: io::ErrorKind::AlreadyExists.into(),
        })
    }
}

/// Build content beside its destination and publish it when complete. Return `false` if
/// publication finds the destination occupied. Cleanup is best-effort and removes only staging
/// content created by this call.
fn build_and_publish<T>(
    kind: ContentKind,
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

/// Create a private staging node using `make`. Fail if the path is already occupied.
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

/// Create a staging file exclusively, using platform-default permissions.
#[cfg(not(unix))]
fn create_private_file(staging: &Path) -> io::Result<fs::File> {
    fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(staging)
}

/// Create a staging directory exclusively, using platform-default permissions.
#[cfg(not(unix))]
fn create_private_directory(staging: &Path) -> io::Result<()> {
    fs::create_dir(staging)
}

/// Publish complete staged content; return false if the destination is occupied.
/// Files use a hard link where supported. The rename fallback and directory
/// publication have a race between the occupancy check and rename.
fn publish(staging: &Path, kind: ContentKind, dest: &Path) -> Result<bool, Error> {
    if let ContentKind::File = kind {
        match fs::hard_link(staging, dest) {
            Ok(()) => return Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Ok(false),
            // Fall back to rename on filesystems without hard-link support.
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
fn discard(staging: &Path, kind: ContentKind, reporter: &Reporter) {
    let removed = match kind {
        ContentKind::File => fs::remove_file(staging),
        ContentKind::Directory => fs::remove_dir_all(staging),
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
    discard(&scratch.path, ContentKind::File, reporter);
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
    paths::beside(dest, ".batfiles-incomplete")
}

/// Where content that has to arrive whole is downloaded to.
fn scratch_path(dest: &Path) -> PathBuf {
    paths::beside(dest, ".batfiles-download")
}
