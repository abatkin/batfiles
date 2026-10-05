//! Validate tar and gzip archives, then extract into an owned staging directory.
//! Both passes use the supplied open file; unsafe paths and link traversal fail.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use flate2::read::MultiGzDecoder;
use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::symlink;

use crate::entry_filter::EntryFilter;
use crate::error::Error;

/// The first bytes of a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// Number of bytes in a tar header block.
const HEADER_BYTES: usize = 512;

/// Where a tar header keeps the checksum of the block it is in.
const CHECKSUM_FIELD: std::ops::Range<usize> = 148..156;

/// The value `archive-root` takes to mean "whatever the single top-level
/// directory turns out to be".
const DETECT_ROOT: &str = "*";

/// The permission bits an unpacked entry may carry.
#[cfg(unix)]
const KEPT_BITS: u32 = 0o777;

/// The mode an entry is created with, before the archive's own is applied.
#[cfg(unix)]
const BUILDING_MODE: u32 = 0o600;

/// Fallback Unix permissions for directories with no readable archive mode.
#[cfg(unix)]
const UNSTATED_DIRECTORY_MODE: u32 = 0o755;

/// Fallback Unix permissions for files with no readable archive mode.
#[cfg(unix)]
const UNSTATED_FILE_MODE: u32 = 0o644;

/// Placeholder mode on platforms where Unix permissions are not applied.
#[cfg(not(unix))]
const UNSTATED_DIRECTORY_MODE: u32 = 0;

#[cfg(not(unix))]
fn symlink(_target: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// A tar entry backed by a plain or decompressed reader.
type ArchiveEntry<'a> = tar::Entry<'a, Box<dyn io::Read>>;

/// Failures that prevent archive extraction.
#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    /// An unsupported archive format, identified when possible.
    #[error("is {saw}, and `fetch-archive` unpacks tar archives, gzipped or plain")]
    Format { saw: &'static str },

    /// A recognized archive format whose contents cannot be read.
    #[error("could not be read: {source}")]
    Unreadable { source: io::Error },

    /// An entry path or link target that escapes the extraction directory.
    #[error("has an entry that would be written outside it: `{entry}`")]
    EscapingEntry { entry: String },

    /// An entry that is neither a file, a directory, nor a link.
    #[error("has an entry that is neither a file, a directory, nor a link: `{entry}`")]
    UnsupportedEntry { entry: String },

    /// A symlink entry on a platform where symlink creation is unsupported.
    #[error("holds the symlink `{entry}`, and symlinks are not supported on this platform")]
    SymlinkEntry { entry: String },

    /// Automatic root detection failed because the archive has no single top-level directory.
    #[error(
        "has no single top-level directory for `archive-root = \"{DETECT_ROOT}\"` to strip; \
         it has {}. Name one of them instead",
        .found.join(", ")
    )]
    AmbiguousRoot { found: Vec<String> },

    /// No archive entries exist under the requested `archive-root`.
    #[error("has nothing under `{root}`")]
    EmptyRoot { root: String },

    /// An archive with no entries.
    #[error("is empty")]
    Empty,

    /// Entry filters that leave nothing of the archive to install.
    #[error("has nothing that `include` and `exclude` select")]
    NothingSelected,

    /// A hardlink entry the filters select, to an entry they leave out.
    #[error(
        "has the hard link `{entry}` to `{target}`, which `include` and `exclude` leave out; \
         select both or neither"
    )]
    LinkTargetLeftOut { entry: String, target: String },

    /// A second read of the archive that did not line up with the first.
    #[error("changed while it was being unpacked")]
    Changed,
}

/// Validate an archive and unpack selected entries into an owned staging directory.
/// The file must contain the complete, verified download. `root` must already
/// pass manifest validation. `filter` decides entries by their path with the root
/// stripped. Failure may leave partial content inside `into`.
pub(crate) fn extract(
    archive: &fs::File,
    into: &Path,
    root: Option<&str>,
    filter: Option<&mut EntryFilter>,
    url: &str,
) -> Result<(), Error> {
    let tarball = Tarball::identify(archive, url)?;
    let records = plan(&tarball, root, filter)?;
    unpack(&tarball, &records, into)
}

/// An open archive, its detected format, and its URL for diagnostics.
struct Tarball<'a> {
    /// The complete, verified download. Read through this handle, not its scratch path.
    file: &'a fs::File,
    format: Format,
    /// Carried for the diagnostics alone.
    url: &'a str,
}

impl<'a> Tarball<'a> {
    /// Identify the file's archive format, or return an error naming its URL.
    fn identify(file: &'a fs::File, url: &'a str) -> Result<Self, Error> {
        let head = head_of(file).map_err(|source| Error::Archive {
            url: url.to_owned(),
            source: ArchiveError::Unreadable { source },
        })?;
        let format = if head.starts_with(&GZIP_MAGIC) {
            Format::Gzip
        } else if is_tar_header(&head) {
            Format::Plain
        } else {
            return Err(Error::Archive {
                url: url.to_owned(),
                source: ArchiveError::Format {
                    saw: looks_like(&head),
                },
            });
        };
        Ok(Self { file, format, url })
    }

    /// Attach the archive URL to an extraction error.
    fn fault(&self, invalid: ArchiveError) -> Error {
        Error::Archive {
            url: self.url.to_owned(),
            source: invalid,
        }
    }

    /// An entry that would be written outside the tree.
    fn escaping(&self, entry: &Path) -> Error {
        self.fault(ArchiveError::EscapingEntry {
            entry: display(entry),
        })
    }

    /// Wrap a read failure with the archive URL.
    fn unreadable(&self, source: io::Error) -> Error {
        self.fault(ArchiveError::Unreadable { source })
    }
}

/// Archive format detected from the file contents.
#[derive(Debug, Clone, Copy)]
enum Format {
    Gzip,
    Plain,
}

/// One entry, as [`plan`] left it.
struct EntryPlan {
    /// Archive path with `.` components removed; metadata headers retain their original paths.
    archive_path: PathBuf,
    /// Where it goes, once the root has been stripped.
    placement: Placement,
    /// What it is, and where a link points.
    kind: EntryKind,
}

/// Where an entry ends up in the tree being built.
enum Placement {
    /// At this path under the tree.
    At(PathBuf),
    /// The extraction root, including a stripped `archive-root` entry. Only its mode is
    /// applied.
    Root,
    /// Outside a named `archive-root`, and therefore not installed at all.
    NotInstalled,
    /// Under the root at this path, and left out by the entry filters.
    LeftOut(PathBuf),
}

impl Placement {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::At(path) => Some(path),
            Self::Root | Self::NotInstalled | Self::LeftOut(_) => None,
        }
    }
}

/// What an entry is, and what a link entry resolves to.
enum EntryKind {
    File,
    Directory,
    /// The symlink target exactly as stored in the archive.
    Symlink(PathBuf),
    /// The hardlink target relative to the extraction root, after stripping `archive-root`.
    Hardlink(PathBuf),
    /// A pax or GNU extension header, skipped during extraction.
    Metadata,
}

/// Whether a block is a tar header, by the checksum it carries of itself.
fn is_tar_header(head: &[u8]) -> bool {
    let Some(block) = head.get(..HEADER_BYTES) else {
        return false;
    };
    let Some(declared) = octal(&block[CHECKSUM_FIELD]) else {
        return false;
    };
    let (unsigned, signed) =
        block
            .iter()
            .enumerate()
            .fold((0u32, 0i32), |(unsigned, signed), (offset, &byte)| {
                // The checksum field is summed as spaces.
                let byte = if CHECKSUM_FIELD.contains(&offset) {
                    b' '
                } else {
                    byte
                };
                (unsigned + u32::from(byte), signed + i32::from(byte as i8))
            });
    // Accept both signed-byte and unsigned-byte tar checksums.
    declared == unsigned || i64::from(declared) == i64::from(signed)
}

/// Parse a padded octal tar-header field; return `None` if invalid.
fn octal(field: &[u8]) -> Option<u32> {
    let mut value: u32 = 0;
    let mut digits = 0;
    for byte in field.iter().skip_while(|byte| **byte == b' ') {
        if !(b'0'..=b'7').contains(byte) {
            break;
        }
        value = value.checked_mul(8)?.checked_add(u32::from(byte - b'0'))?;
        digits += 1;
    }
    (digits > 0).then_some(value)
}

/// What a run of leading bytes is, in the words a diagnostic uses.
fn looks_like(head: &[u8]) -> &'static str {
    for (magic, name) in [
        (&b"PK\x03\x04"[..], "a zip archive"),
        (&b"BZh"[..], "a bzip2 archive"),
        (&b"\xfd7zXZ\x00"[..], "an xz archive"),
        (&b"\x28\xb5\x2f\xfd"[..], "a zstd archive"),
        (&b"7z\xbc\xaf\x27\x1c"[..], "a 7-zip archive"),
    ] {
        if head.starts_with(magic) {
            return name;
        }
    }
    "not an archive batfiles recognizes"
}

/// The archive's first block, or as much of it as there is.
fn head_of(file: &fs::File) -> io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut head = Vec::with_capacity(HEADER_BYTES);
    rewound(file)?
        .take(HEADER_BYTES as u64)
        .read_to_end(&mut head)?;
    Ok(head)
}

/// Clone the open file handle and rewind it. Both handles share the file cursor;
/// callers must read sequentially.
fn rewound(file: &fs::File) -> io::Result<fs::File> {
    use std::io::Seek as _;

    let mut own = file.try_clone()?;
    own.rewind()?;
    Ok(own)
}

/// Validate archive entries and compute their destinations after stripping the selected root
/// and applying the entry filters.
fn plan(
    tarball: &Tarball<'_>,
    root: Option<&str>,
    filter: Option<&mut EntryFilter>,
) -> Result<Vec<EntryPlan>, Error> {
    let mut declared: Vec<(PathBuf, EntryKind)> = Vec::new();
    for_each_entry(tarball, |entry| {
        let as_written = path_of(entry, tarball)?;
        let kind = kind_of(entry, tarball)?;
        let archive_path = if matches!(kind, EntryKind::Metadata) {
            as_written
        } else {
            entry_path(&as_written).ok_or_else(|| tarball.escaping(&as_written))?
        };
        declared.push((archive_path, kind));
        Ok(())
    })?;

    let strip = root_prefix(&declared, root, tarball)?;
    let mut records: Vec<EntryPlan> = declared
        .into_iter()
        .map(|(archive_path, kind)| {
            let placement = match kind {
                EntryKind::Metadata => Placement::NotInstalled,
                _ => place(&archive_path, strip.as_deref()),
            };
            EntryPlan {
                archive_path,
                placement,
                kind,
            }
        })
        .collect();

    if !installs_anything(&records) {
        return Err(tarball.fault(match strip {
            Some(root) => ArchiveError::EmptyRoot {
                root: display(&root),
            },
            None => ArchiveError::Empty,
        }));
    }

    // Collect all symlinks under the root before validating paths or filtering; later entries
    // can change how `..` resolves, and a filter does not change what is safe.
    let links: BTreeSet<PathBuf> = records
        .iter()
        .filter(|record| matches!(record.kind, EntryKind::Symlink(_)))
        .filter_map(|record| record.placement.path().map(Path::to_path_buf))
        .collect();

    if let Some(filter) = filter {
        leave_out(&mut records, filter);
        if !installs_anything(&records) {
            return Err(tarball.fault(ArchiveError::NothingSelected));
        }
    }

    let left_out: BTreeSet<PathBuf> = records
        .iter()
        .filter_map(|record| match &record.placement {
            Placement::LeftOut(path) => Some(path.clone()),
            _ => None,
        })
        .collect();
    for record in &mut records {
        check_and_resolve(record, strip.as_deref(), &links, &left_out, tarball)?;
    }
    Ok(records)
}

/// Whether any entry is placed in the tree.
fn installs_anything(records: &[EntryPlan]) -> bool {
    records
        .iter()
        .any(|record| matches!(record.placement, Placement::At(_)))
}

/// Mark what `filter` does not select as left out, keeping a directory entry that holds
/// something it does.
fn leave_out(records: &mut [EntryPlan], filter: &mut EntryFilter) {
    let mut selected = BTreeSet::new();
    let mut holding = BTreeSet::new();
    for record in records.iter() {
        if let Placement::At(path) = &record.placement
            && filter.selects(path)
        {
            selected.insert(path.clone());
            holding.extend(path.ancestors().skip(1).map(Path::to_path_buf));
        }
    }
    for record in records.iter_mut() {
        if let Placement::At(path) = &record.placement {
            let held = matches!(record.kind, EntryKind::Directory) && holding.contains(path);
            if !selected.contains(path) && !held {
                record.placement = Placement::LeftOut(path.clone());
            }
        }
    }
}

/// Check an entry that is going to be installed, and settle where a link points. A hardlink
/// may not point at an entry in `left_out`.
fn check_and_resolve(
    record: &mut EntryPlan,
    strip: Option<&Path>,
    links: &BTreeSet<PathBuf>,
    left_out: &BTreeSet<PathBuf>,
    tarball: &Tarball<'_>,
) -> Result<(), Error> {
    let Some(inside) = record.placement.path() else {
        return Ok(());
    };
    if walks_through_a_link(inside, links) {
        return Err(tarball.escaping(&record.archive_path));
    }
    match &mut record.kind {
        EntryKind::File | EntryKind::Directory | EntryKind::Metadata => Ok(()),
        EntryKind::Symlink(target) => {
            if cfg!(not(unix)) {
                return Err(tarball.fault(ArchiveError::SymlinkEntry {
                    entry: display(&record.archive_path),
                }));
            }
            let from = inside.parent().unwrap_or_else(|| Path::new(""));
            if stays_inside(&from.join(target.as_path()), links) {
                Ok(())
            } else {
                Err(tarball.escaping(&record.archive_path))
            }
        }
        EntryKind::Hardlink(target) => {
            let resolved = entry_path(target)
                .map(|named| place(&named, strip))
                .and_then(|placement| placement.path().map(Path::to_path_buf))
                .filter(|resolved| !walks_through_a_link(resolved, links))
                .ok_or_else(|| tarball.escaping(&record.archive_path))?;
            if left_out.contains(&resolved) {
                return Err(tarball.fault(ArchiveError::LinkTargetLeftOut {
                    entry: display(&record.archive_path),
                    target: display(target),
                }));
            }
            *target = resolved;
            Ok(())
        }
    }
}

/// Resolve the `archive-root` prefix from the declared entry paths.
fn root_prefix(
    declared: &[(PathBuf, EntryKind)],
    root: Option<&str>,
    tarball: &Tarball<'_>,
) -> Result<Option<PathBuf>, Error> {
    let Some(root) = root else {
        return Ok(None);
    };
    if root != DETECT_ROOT {
        return Ok(Some(entry_path(Path::new(root)).expect(
            "the manifest refuses an archive-root that is not a path inside an archive",
        )));
    }
    let tops: BTreeSet<String> = declared
        .iter()
        .filter(|(_, kind)| !matches!(kind, EntryKind::Metadata))
        .filter_map(|(path, _)| path.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    match tops.len() {
        0 => Err(tarball.fault(ArchiveError::Empty)),
        1 => Ok(tops.into_iter().next().map(PathBuf::from)),
        _ => Err(tarball.fault(ArchiveError::AmbiguousRoot {
            found: tops.into_iter().collect(),
        })),
    }
}

/// Where an entry lands once the root has been stripped.
fn place(path: &Path, strip: Option<&Path>) -> Placement {
    let remainder = match strip {
        Some(prefix) => match path.strip_prefix(prefix) {
            Ok(remainder) => remainder,
            Err(_) => return Placement::NotInstalled,
        },
        None => path,
    };
    match remainder.components().next() {
        Some(_) => Placement::At(remainder.to_path_buf()),
        None => Placement::Root,
    }
}

/// Remove `.` components from an archive path. Reject roots, platform prefixes,
/// and every `..` component; an empty path represents the archive root.
fn entry_path(path: &Path) -> Option<PathBuf> {
    let mut cleaned = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => cleaned.push(name),
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(cleaned)
}

/// Whether a path passes *through* one of the archive's own symlinks.
fn walks_through_a_link(path: &Path, links: &BTreeSet<PathBuf>) -> bool {
    path.ancestors()
        .skip(1)
        .any(|ancestor| links.contains(ancestor))
}

/// Check a relative symlink target for root escape. Reject `..` immediately
/// after an archive symlink, whose filesystem resolution cannot be canceled lexically.
fn stays_inside(path: &Path, links: &BTreeSet<PathBuf>) -> bool {
    let mut walked = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(name) => walked.push(name),
            Component::ParentDir => {
                if walked.components().next().is_none() || links.contains(&walked) {
                    return false;
                }
                walked.pop();
            }
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    walked.components().next().is_some()
}

/// Write the records into the tree being built.
fn unpack(tarball: &Tarball<'_>, records: &[EntryPlan], into: &Path) -> Result<(), Error> {
    let mut root_mode = UNSTATED_DIRECTORY_MODE;
    let mut directories: Vec<(PathBuf, u32)> = Vec::new();
    let mut planned = records.iter();

    for_each_entry(tarball, |entry| {
        let Some(record) = planned.next() else {
            return Err(tarball.fault(ArchiveError::Changed));
        };
        let built_at = match &record.placement {
            Placement::At(inside) => into.join(inside),
            Placement::Root => {
                if matches!(record.kind, EntryKind::Directory) {
                    root_mode = mode_of(entry, &record.kind);
                }
                return Ok(());
            }
            Placement::NotInstalled | Placement::LeftOut(_) => return Ok(()),
        };

        match &record.kind {
            EntryKind::Directory => {
                create_directory(&built_at)?;
                directories.push((built_at, mode_of(entry, &record.kind)));
            }
            EntryKind::File => {
                create_parents(&built_at)?;
                let mut file = create_private_file(&built_at)?;
                io::copy(entry, &mut file).map_err(|source| Error::Write {
                    path: built_at.clone(),
                    source,
                })?;
                // Keep the file private until its contents are complete.
                set_mode(&built_at, mode_of(entry, &record.kind))?;
            }
            EntryKind::Symlink(target) => {
                create_parents(&built_at)?;
                symlink(target, &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            EntryKind::Hardlink(target) => {
                create_parents(&built_at)?;
                fs::hard_link(into.join(target), &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            EntryKind::Metadata => {}
        }
        Ok(())
    })?;
    if planned.next().is_some() {
        return Err(tarball.fault(ArchiveError::Changed));
    }

    // Apply descendant modes before ancestors that may become unwritable.
    directories.sort_by(|(left, _), (right, _)| right.cmp(left));
    for (path, mode) in directories {
        set_mode(&path, mode)?;
    }
    set_mode(into, root_mode)
}

/// Read the archive from the start, handing each entry to `visit`.
fn for_each_entry(
    tarball: &Tarball<'_>,
    mut visit: impl FnMut(&mut ArchiveEntry<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let file = rewound(tarball.file).map_err(|source| tarball.unreadable(source))?;
    let reader: Box<dyn io::Read> = match tarball.format {
        Format::Gzip => Box::new(MultiGzDecoder::new(file)),
        Format::Plain => Box::new(file),
    };
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries()
        .map_err(|source| tarball.unreadable(source))?
    {
        let mut entry = entry.map_err(|source| tarball.unreadable(source))?;
        visit(&mut entry)?;
    }
    Ok(())
}

/// The path an entry names, as the archive writes it.
fn path_of(entry: &ArchiveEntry<'_>, tarball: &Tarball<'_>) -> Result<PathBuf, Error> {
    entry
        .path()
        .map(|path| path.into_owned())
        .map_err(|source| tarball.unreadable(source))
}

/// Which kind an entry is, refusing the ones batfiles has no way to install.
fn kind_of(entry: &ArchiveEntry<'_>, tarball: &Tarball<'_>) -> Result<EntryKind, Error> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
    {
        return Ok(EntryKind::Metadata);
    }
    if entry_type.is_dir() {
        return Ok(EntryKind::Directory);
    }
    if entry_type.is_file() {
        return Ok(EntryKind::File);
    }
    let unsupported = || {
        tarball.fault(ArchiveError::UnsupportedEntry {
            entry: entry
                .path()
                .map_or_else(|_| String::from("?"), |path| display(&path)),
        })
    };
    let Some(target) = entry.link_name().ok().flatten() else {
        return Err(unsupported());
    };
    if entry_type.is_symlink() {
        Ok(EntryKind::Symlink(target.into_owned()))
    } else if entry_type.is_hard_link() {
        Ok(EntryKind::Hardlink(target.into_owned()))
    } else {
        Err(unsupported())
    }
}

/// Create a directory and any missing parents under the private staging root.
fn create_directory(at: &Path) -> Result<(), Error> {
    fs::create_dir_all(at).map_err(|source| Error::Write {
        path: at.to_path_buf(),
        source,
    })
}

/// Create any missing parent directories for an entry.
fn create_parents(at: &Path) -> Result<(), Error> {
    match at.parent() {
        Some(parent) => create_directory(parent),
        None => Ok(()),
    }
}

/// Create a file with private permissions; fail if the path already exists.
#[cfg(unix)]
fn create_private_file(at: &Path) -> Result<fs::File, Error> {
    use std::os::unix::fs::OpenOptionsExt as _;

    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(BUILDING_MODE)
        .open(at)
        .map_err(|source| Error::Write {
            path: at.to_path_buf(),
            source,
        })
}

/// Create a file exclusively, without applying Unix permissions.
#[cfg(not(unix))]
fn create_private_file(at: &Path) -> Result<fs::File, Error> {
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(at)
        .map_err(|source| Error::Write {
            path: at.to_path_buf(),
            source,
        })
}

/// The permission bits an entry asks for, with the ones batfiles will not grant
/// removed.
#[cfg(unix)]
fn mode_of(entry: &ArchiveEntry<'_>, kind: &EntryKind) -> u32 {
    let unstated = match kind {
        EntryKind::Directory => UNSTATED_DIRECTORY_MODE,
        _ => UNSTATED_FILE_MODE,
    };
    entry.header().mode().unwrap_or(unstated) & KEPT_BITS
}

/// Return a placeholder mode on platforms where Unix permissions are not applied.
#[cfg(not(unix))]
fn mode_of(_entry: &ArchiveEntry<'_>, _kind: &EntryKind) -> u32 {
    0
}

#[cfg(unix)]
fn set_mode(at: &Path, mode: u32) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(at, fs::Permissions::from_mode(mode)).map_err(|source| Error::Write {
        path: at.to_path_buf(),
        source,
    })
}

#[cfg(not(unix))]
fn set_mode(_at: &Path, _mode: u32) -> Result<(), Error> {
    Ok(())
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn linking(paths: &[&str]) -> BTreeSet<PathBuf> {
        paths.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn an_entry_path_keeps_its_components_and_drops_its_dots() {
        assert_eq!(entry_path(Path::new("a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(entry_path(Path::new("./a/b")), Some(PathBuf::from("a/b")));
        assert_eq!(entry_path(Path::new("a/./b")), Some(PathBuf::from("a/b")));
        // Tar uses `./` for the archive root.
        assert_eq!(entry_path(Path::new("./")), Some(PathBuf::new()));
        assert_eq!(entry_path(Path::new("")), Some(PathBuf::new()));
    }

    #[test]
    fn an_entry_path_that_could_leave_the_tree_is_refused_rather_than_cancelled() {
        for refused in ["/etc/passwd", "..", "../a", "a/../b", "a/../../b"] {
            assert_eq!(entry_path(Path::new(refused)), None, "`{refused}`");
        }
    }

    #[test]
    fn a_root_is_stripped_off_what_is_under_it_and_nothing_else() {
        let strip = Some(Path::new("tool-1.0"));
        let at = |path: &str| match place(Path::new(path), strip) {
            Placement::At(inside) => Some(inside),
            Placement::Root | Placement::NotInstalled | Placement::LeftOut(_) => None,
        };
        assert_eq!(at("tool-1.0/bin/tool"), Some(PathBuf::from("bin/tool")));
        assert_eq!(at("other/x"), None);
        assert_eq!(at("tool-1.0-docs/x"), None);
        assert!(matches!(
            place(Path::new("tool-1.0"), strip),
            Placement::Root
        ));
        assert!(matches!(
            place(Path::new("other/x"), strip),
            Placement::NotInstalled
        ));
        assert!(matches!(
            place(Path::new("bin/tool"), None),
            Placement::At(_)
        ));
        assert!(matches!(place(Path::new(""), None), Placement::Root));
    }

    #[test]
    fn nothing_is_written_under_a_symlink_the_archive_declares() {
        let links = linking(&["bin/tool"]);
        assert!(walks_through_a_link(Path::new("bin/tool/x"), &links));
        assert!(walks_through_a_link(Path::new("bin/tool/deep/x"), &links));
        // The link itself is not *through* the link: creating it is the entry.
        assert!(!walks_through_a_link(Path::new("bin/tool"), &links));
        assert!(!walks_through_a_link(Path::new("bin/other"), &links));
    }

    #[test]
    fn a_symlink_target_may_climb_past_a_directory_and_not_past_a_link() {
        let links = linking(&["a/b", "lib/libfoo.so.1"]);
        for inside in ["x", "bin/../lib/x", "a/b", "lib/libfoo.so.1"] {
            assert!(stays_inside(Path::new(inside), &links), "`{inside}`");
        }
        assert!(!stays_inside(Path::new("a/b/../../outside"), &links));
        // One `..` past the link is already wrong, before it leaves the tree.
        assert!(!stays_inside(Path::new("a/b/../c"), &links));
        for refused in ["/etc/shadow", "../../outside", ".."] {
            assert!(!stays_inside(Path::new(refused), &links), "`{refused}`");
        }
    }

    #[test]
    fn leading_bytes_are_named_where_they_are_recognizable() {
        assert_eq!(looks_like(b"PK\x03\x04rest"), "a zip archive");
        assert_eq!(looks_like(b"BZh9"), "a bzip2 archive");
        assert_eq!(
            looks_like(b"<!DOCTYPE html>"),
            "not an archive batfiles recognizes"
        );
        assert_eq!(looks_like(b""), "not an archive batfiles recognizes");
    }

    /// One header block, checksummed, with `magic` written where a `ustar`
    /// archive carries one and a V7 archive carries nothing.
    fn header(magic: &[u8]) -> Vec<u8> {
        let mut block = vec![0u8; HEADER_BYTES];
        block[..8].copy_from_slice(b"a/b\0\0\0\0\0");
        block[100..108].copy_from_slice(b"000644 \0");
        block[124..136].copy_from_slice(b"00000000002\0");
        block[257..257 + magic.len()].copy_from_slice(magic);
        block[CHECKSUM_FIELD].fill(b' ');
        let sum: u32 = block.iter().map(|&byte| u32::from(byte)).sum();
        let written = format!("{sum:06o}\0 ");
        block[CHECKSUM_FIELD].copy_from_slice(written.as_bytes());
        block
    }

    #[test]
    fn a_tar_is_recognized_by_its_checksum_rather_than_by_its_format() {
        // V7 headers lack the `ustar` magic; their checksum still identifies them as tar.
        for magic in [&b"ustar\0"[..], b"ustar  \0", b""] {
            assert!(is_tar_header(&header(magic)), "{magic:?}");
        }
    }

    #[test]
    fn something_that_is_not_a_tar_header_is_not_taken_for_one() {
        let mut wrong = header(b"ustar\0");
        wrong[0] = b'z';
        assert!(!is_tar_header(&wrong));
        assert!(!is_tar_header(b"ustar"));
        assert!(!is_tar_header(&[0u8; HEADER_BYTES]));
        assert!(!is_tar_header(&[b'x'; HEADER_BYTES]));
    }
}
