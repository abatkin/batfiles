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

use crate::error::Error;

/// The first bytes of a gzip stream.
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

/// How much of the file has to be read to tell one format from another: one tar
/// header block, which carries its own checksum.
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

/// The mode a directory lands with where the archive does not say: the unpacked
/// tree when no entry describes the root being stripped, and any directory whose
/// header mode does not read.
#[cfg(unix)]
const UNSTATED_DIRECTORY_MODE: u32 = 0o755;

/// The same, for an entry that is not a directory.
#[cfg(unix)]
const UNSTATED_FILE_MODE: u32 = 0o644;

/// Where a mode means something other than it does on unix, nothing is applied
/// and the value is never read.
#[cfg(not(unix))]
const UNSTATED_DIRECTORY_MODE: u32 = 0;

#[cfg(not(unix))]
fn symlink(_target: &Path, _at: &Path) -> io::Result<()> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

/// One entry of the archive, as the reader hands it over. The reader is boxed so
/// that both formats reach one loop.
type ArchiveEntry<'a> = tar::Entry<'a, Box<dyn io::Read>>;

/// What an archive can be that stops it being unpacked.
#[derive(Debug, Error)]
pub(crate) enum Invalid {
    /// Bytes that are not an archive batfiles unpacks, named by what they look
    /// like where that is recognizable.
    #[error("is {saw}, and `fetch-archive` unpacks tar archives, gzipped or plain")]
    Format { saw: &'static str },

    /// An archive of the right format that does not read as one.
    #[error("could not be read: {source}")]
    Unreadable { source: io::Error },

    /// An entry naming a path outside the tree being unpacked: an absolute path,
    /// one climbing out with `..`, or a link whose target does either. The whole
    /// extraction stops, because an archive carrying one of these is not one to
    /// install part of.
    #[error("has an entry that would be written outside it: `{entry}`")]
    EscapingEntry { entry: String },

    /// An entry that is not a file, a directory, or a link. Skipping a device
    /// node or a fifo would publish an incomplete tree as a finished one.
    #[error("has an entry that is neither a file, a directory, nor a link: `{entry}`")]
    UnsupportedEntry { entry: String },

    /// A symlink entry on a platform where batfiles does not make symlinks. The
    /// archive is valid; this machine cannot install it.
    #[error("holds the symlink `{entry}`, and symlinks are not supported on this platform")]
    SymlinkEntry { entry: String },

    /// `archive-root = "*"` over an archive with no single top-level directory
    /// to strip. Which ones it has, because that is what the author writes in
    /// place of the `*`.
    #[error(
        "has no single top-level directory for `archive-root = \"{DETECT_ROOT}\"` to strip; \
         it has {}. Name one of them instead",
        .found.join(", ")
    )]
    AmbiguousRoot { found: Vec<String> },

    /// An `archive-root` over an archive holding nothing under it.
    #[error("has nothing under `{root}`")]
    EmptyRoot { root: String },

    /// An archive with no entries at all, which no `archive-root` is to blame
    /// for.
    #[error("is empty")]
    Empty,

    /// A second read of the archive that did not line up with the first.
    #[error("changed while it was being unpacked")]
    Changed,
}

/// Validate an archive and unpack selected entries into an owned staging directory.
/// The file must contain the complete, verified download. `root` must already
/// pass manifest validation. Failure may leave partial content inside `into`.
pub(crate) fn extract(
    archive: &fs::File,
    into: &Path,
    root: Option<&str>,
    url: &str,
) -> Result<(), Error> {
    let tarball = Tarball::identify(archive, url)?;
    let records = plan(&tarball, root)?;
    unpack(&tarball, &records, into)
}

/// The archive being unpacked: its open bytes, how they are wrapped, and what to
/// call it when something is wrong with it.
struct Tarball<'a> {
    /// The downloaded, hashed file, held open. Never reopened by path: the path
    /// is a scratch name that may since hold something else.
    file: &'a fs::File,
    format: Format,
    /// Carried for the diagnostics alone.
    url: &'a str,
}

impl<'a> Tarball<'a> {
    /// Read enough of the file to say what it is, or refuse it by name.
    fn identify(file: &'a fs::File, url: &'a str) -> Result<Self, Error> {
        let head = head_of(file).map_err(|source| Error::Archive {
            url: url.to_owned(),
            source: Invalid::Unreadable { source },
        })?;
        let format = if head.starts_with(&GZIP_MAGIC) {
            Format::Gzip
        } else if is_tar_header(&head) {
            Format::Plain
        } else {
            return Err(Error::Archive {
                url: url.to_owned(),
                source: Invalid::Format {
                    saw: looks_like(&head),
                },
            });
        };
        Ok(Self { file, format, url })
    }

    /// Name this archive, by URL, as the reason something failed.
    fn fault(&self, invalid: Invalid) -> Error {
        Error::Archive {
            url: self.url.to_owned(),
            source: invalid,
        }
    }

    /// An entry that would be written outside the tree.
    fn escaping(&self, entry: &Path) -> Error {
        self.fault(Invalid::EscapingEntry {
            entry: display(entry),
        })
    }

    /// An archive of the right format that does not read as one.
    fn unreadable(&self, source: io::Error) -> Error {
        self.fault(Invalid::Unreadable { source })
    }
}

/// How the archive's bytes are wrapped, decided by reading them rather than from
/// the URL, which redirects and is named by whoever published it.
#[derive(Debug, Clone, Copy)]
enum Format {
    Gzip,
    Plain,
}

/// One entry, as [`plan`] left it.
struct Record {
    /// Where the archive says it is, with `.` components dropped — the form
    /// everything below matches against, and the one a diagnostic names it by. A
    /// [`Kind::Metadata`] entry keeps the raw spelling, since nothing is matched
    /// against it and nothing is installed at it.
    archive_path: PathBuf,
    /// Where it goes, once the root has been stripped.
    placement: Placement,
    /// What it is, and where a link points.
    kind: Kind,
}

/// Where an entry ends up in the tree being built.
enum Placement {
    /// At this path under the tree.
    At(PathBuf),
    /// The tree itself: the archive's own root entry, or the directory
    /// `archive-root` stripped. It is already there, so the only thing it has
    /// left to say is what mode it should carry.
    TheTreeItself,
    /// Outside a named `archive-root`, and therefore not installed at all.
    NotInstalled,
}

impl Placement {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::At(path) => Some(path),
            Self::TheTreeItself | Self::NotInstalled => None,
        }
    }
}

/// What an entry is, and what a link entry resolves to.
enum Kind {
    File,
    Directory,
    /// The target exactly as the archive writes it, since that is what a symlink
    /// stores.
    Symlink(PathBuf),
    /// The target's place in the tree being built, root already stripped: a tar
    /// hardlink names a path within the archive rather than one relative to the
    /// entry, so it is stripped the way an entry path is.
    Hardlink(PathBuf),
    /// A pax or GNU extension header, which describes the next entry. A kind of
    /// its own so both passes skip the same entries.
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
    // Two answers because implementations disagreed about whether a byte is
    // signed. Either one is the archive agreeing with itself.
    declared == unsigned || i64::from(declared) == i64::from(signed)
}

/// A tar header's octal number field, which is digits, then padding.
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

/// Read every entry, check what will be written, and settle what the root
/// strips.
fn plan(tarball: &Tarball<'_>, root: Option<&str>) -> Result<Vec<Record>, Error> {
    let mut declared: Vec<(PathBuf, Kind)> = Vec::new();
    for_each_entry(tarball, |entry| {
        let as_written = path_of(entry, tarball)?;
        let kind = kind_of(entry, tarball)?;
        let archive_path = if matches!(kind, Kind::Metadata) {
            as_written
        } else {
            // Named in the diagnostic as the archive writes it, since the
            // spelling is what is wrong with it.
            entry_path(&as_written).ok_or_else(|| tarball.escaping(&as_written))?
        };
        declared.push((archive_path, kind));
        Ok(())
    })?;

    let strip = root_prefix(&declared, root, tarball)?;
    let mut records: Vec<Record> = declared
        .into_iter()
        .map(|(archive_path, kind)| {
            let placement = match kind {
                // Describes the entry after it rather than being one, so it is
                // installed nowhere and no root has anything to strip off it.
                Kind::Metadata => Placement::NotInstalled,
                _ => place(&archive_path, strip.as_deref()),
            };
            Record {
                archive_path,
                placement,
                kind,
            }
        })
        .collect();

    // Every symlink that will exist in the finished tree, which is what decides
    // whether a `..` elsewhere can be trusted.
    let links: BTreeSet<PathBuf> = records
        .iter()
        .filter(|record| matches!(record.kind, Kind::Symlink(_)))
        .filter_map(|record| record.placement.path().map(Path::to_path_buf))
        .collect();

    for record in &mut records {
        check_and_resolve(record, strip.as_deref(), &links, tarball)?;
    }

    if !records
        .iter()
        .any(|record| matches!(record.placement, Placement::At(_)))
    {
        return Err(tarball.fault(match strip {
            Some(root) => Invalid::EmptyRoot {
                root: display(&root),
            },
            None => Invalid::Empty,
        }));
    }
    Ok(records)
}

/// Check an entry that is going to be installed, and settle where a link points.
fn check_and_resolve(
    record: &mut Record,
    strip: Option<&Path>,
    links: &BTreeSet<PathBuf>,
    tarball: &Tarball<'_>,
) -> Result<(), Error> {
    let Some(inside) = record.placement.path() else {
        return Ok(());
    };
    if walks_through_a_link(inside, links) {
        return Err(tarball.escaping(&record.archive_path));
    }
    match &mut record.kind {
        Kind::File | Kind::Directory | Kind::Metadata => Ok(()),
        Kind::Symlink(target) => {
            if cfg!(not(unix)) {
                return Err(tarball.fault(Invalid::SymlinkEntry {
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
        Kind::Hardlink(target) => {
            *target = entry_path(target)
                .map(|named| place(&named, strip))
                .and_then(|placement| placement.path().map(Path::to_path_buf))
                .filter(|resolved| !walks_through_a_link(resolved, links))
                .ok_or_else(|| tarball.escaping(&record.archive_path))?;
            Ok(())
        }
    }
}

/// The prefix every entry is written without, resolved from what the manifest
/// asked for.
fn root_prefix(
    declared: &[(PathBuf, Kind)],
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
        .filter(|(_, kind)| !matches!(kind, Kind::Metadata))
        .filter_map(|(path, _)| path.components().next())
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    match tops.len() {
        0 => Err(tarball.fault(Invalid::Empty)),
        1 => Ok(tops.into_iter().next().map(PathBuf::from)),
        _ => Err(tarball.fault(Invalid::AmbiguousRoot {
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
        None => Placement::TheTreeItself,
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
fn unpack(tarball: &Tarball<'_>, records: &[Record], into: &Path) -> Result<(), Error> {
    let mut root_mode = UNSTATED_DIRECTORY_MODE;
    let mut directories: Vec<(PathBuf, u32)> = Vec::new();
    let mut planned = records.iter();

    for_each_entry(tarball, |entry| {
        let Some(record) = planned.next() else {
            return Err(tarball.fault(Invalid::Changed));
        };
        let built_at = match &record.placement {
            Placement::At(inside) => into.join(inside),
            Placement::TheTreeItself => {
                if matches!(record.kind, Kind::Directory) {
                    root_mode = mode_of(entry, &record.kind);
                }
                return Ok(());
            }
            Placement::NotInstalled => return Ok(()),
        };

        match &record.kind {
            Kind::Directory => {
                create_directory(&built_at)?;
                directories.push((built_at, mode_of(entry, &record.kind)));
            }
            Kind::File => {
                create_parents(&built_at)?;
                let mut file = create_private_file(&built_at)?;
                io::copy(entry, &mut file).map_err(|source| Error::Write {
                    path: built_at.clone(),
                    source,
                })?;
                // Last, so an interrupted run leaves nothing readable behind.
                set_mode(&built_at, mode_of(entry, &record.kind))?;
            }
            Kind::Symlink(target) => {
                create_parents(&built_at)?;
                symlink(target, &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            // The target is already the path within the tree, resolved when the
            // root was known.
            Kind::Hardlink(target) => {
                create_parents(&built_at)?;
                fs::hard_link(into.join(target), &built_at).map_err(|source| Error::Write {
                    path: built_at,
                    source,
                })?;
            }
            Kind::Metadata => {}
        }
        Ok(())
    })?;
    if planned.next().is_some() {
        return Err(tarball.fault(Invalid::Changed));
    }

    // Deepest first, so a directory the archive marks unwritable takes that mode
    // only once nothing more is going into it.
    directories.sort_by(|(left, _), (right, _)| right.cmp(left));
    for (path, mode) in directories {
        set_mode(&path, mode)?;
    }
    // The tree itself last of all, for the same reason: everything under it is
    // finished, and it is what the caller publishes.
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
fn kind_of(entry: &ArchiveEntry<'_>, tarball: &Tarball<'_>) -> Result<Kind, Error> {
    let entry_type = entry.header().entry_type();
    if entry_type.is_pax_global_extensions()
        || entry_type.is_pax_local_extensions()
        || entry_type.is_gnu_longname()
        || entry_type.is_gnu_longlink()
    {
        return Ok(Kind::Metadata);
    }
    if entry_type.is_dir() {
        return Ok(Kind::Directory);
    }
    if entry_type.is_file() {
        return Ok(Kind::File);
    }
    let unsupported = || {
        tarball.fault(Invalid::UnsupportedEntry {
            entry: entry
                .path()
                .map_or_else(|_| String::from("?"), |path| display(&path)),
        })
    };
    let Some(target) = entry.link_name().ok().flatten() else {
        return Err(unsupported());
    };
    if entry_type.is_symlink() {
        Ok(Kind::Symlink(target.into_owned()))
    } else if entry_type.is_hard_link() {
        Ok(Kind::Hardlink(target.into_owned()))
    } else {
        Err(unsupported())
    }
}

/// Create a directory of the tree, and every directory above it the archive did
/// not name. All of it is under the staging root, which is closed, so the modes
/// these take in the meantime are reachable by nobody.
fn create_directory(at: &Path) -> Result<(), Error> {
    fs::create_dir_all(at).map_err(|source| Error::Write {
        path: at.to_path_buf(),
        source,
    })
}

/// Create the directories an entry sits under, for an archive that names a file
/// without naming the directory holding it.
fn create_parents(at: &Path) -> Result<(), Error> {
    match at.parent() {
        Some(parent) => create_directory(parent),
        None => Ok(()),
    }
}

/// Create a private file of the tree, failing rather than truncating if the path
/// is taken: inside a staging node this run just made, a name already taken is an
/// archive naming one entry twice.
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

/// Where a mode means something other than it does on unix, this is ordinary
/// exclusive creation: the permissions batfiles carries across are the unix
/// ones, and there is nothing here to narrow.
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
fn mode_of(entry: &ArchiveEntry<'_>, kind: &Kind) -> u32 {
    let unstated = match kind {
        Kind::Directory => UNSTATED_DIRECTORY_MODE,
        _ => UNSTATED_FILE_MODE,
    };
    entry.header().mode().unwrap_or(unstated) & KEPT_BITS
}

/// Where a mode means something other than it does on unix, an entry's bits are
/// not carried across, so there is nothing to read and nothing to apply.
#[cfg(not(unix))]
fn mode_of(_entry: &ArchiveEntry<'_>, _kind: &Kind) -> u32 {
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
        // What `tar czf x.tgz .` writes for the archive's own root, which names
        // the tree rather than anything in it.
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
            Placement::TheTreeItself | Placement::NotInstalled => None,
        };
        assert_eq!(at("tool-1.0/bin/tool"), Some(PathBuf::from("bin/tool")));
        // Outside the prefix, so not installed. A sibling whose name merely
        // starts with the prefix is not under it.
        assert_eq!(at("other/x"), None);
        assert_eq!(at("tool-1.0-docs/x"), None);
        // The root's own entry is the tree, told apart from the entries that are
        // not installed at all because it still has a mode to give.
        assert!(matches!(
            place(Path::new("tool-1.0"), strip),
            Placement::TheTreeItself
        ));
        assert!(matches!(
            place(Path::new("other/x"), strip),
            Placement::NotInstalled
        ));
        // With no root, every entry keeps the path it was written with, and the
        // archive's own root is still the tree.
        assert!(matches!(
            place(Path::new("bin/tool"), None),
            Placement::At(_)
        ));
        assert!(matches!(
            place(Path::new(""), None),
            Placement::TheTreeItself
        ));
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
        // And the ordinary ways out.
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
        // An empty body is not an archive either, and asking about its first
        // bytes must not panic.
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
        // POSIX, GNU, and V7 — the last of which carries no magic at all, and is
        // the one a check for `ustar` at offset 257 would refuse.
        for magic in [&b"ustar\0"[..], b"ustar  \0", b""] {
            assert!(is_tar_header(&header(magic)), "{magic:?}");
        }
    }

    #[test]
    fn something_that_is_not_a_tar_header_is_not_taken_for_one() {
        // A block that is the right length and says the wrong thing about
        // itself, which is what the checksum is for.
        let mut wrong = header(b"ustar\0");
        wrong[0] = b'z';
        assert!(!is_tar_header(&wrong));
        // Too short to hold a header, all zeroes, and ordinary text.
        assert!(!is_tar_header(b"ustar"));
        assert!(!is_tar_header(&[0u8; HEADER_BYTES]));
        assert!(!is_tar_header(&[b'x'; HEADER_BYTES]));
    }
}
