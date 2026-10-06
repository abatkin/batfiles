//! Validate an archive, then extract its selected entries into an owned staging
//! directory. Each format reader hands over format-neutral entries; both passes
//! read the supplied open file, and unsafe paths and link traversal fail.

mod compressed;
mod detect;
mod tar;
mod zip;

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use thiserror::Error;

#[cfg(unix)]
use std::os::unix::fs::symlink;

use crate::entry_filter::{EntryFilter, Executable};
use crate::error::Error;
pub(crate) use compressed::DecompressError;
use detect::Format;

/// The value `archive-root` takes to mean "whatever the single top-level
/// directory turns out to be".
const DETECT_ROOT: &str = "*";

/// The bits `executable` adds to a file's mode.
#[cfg(unix)]
const EXECUTE_BITS: u32 = 0o111;

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

/// Failures that prevent archive extraction.
#[derive(Debug, Error)]
pub(crate) enum ArchiveError {
    /// An unsupported archive format, identified when possible.
    #[error(
        "is {saw}, and batfiles unpacks zip archives and tar archives, \
         plain or compressed with gzip or bzip2"
    )]
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

    /// A zip entry name that cannot be an entry path as written.
    #[error(
        "has an entry whose name holds a `\\`, a NUL byte, or bytes that are not the \
         UTF-8 it is flagged as: `{entry}`"
    )]
    UnusableName { entry: String },

    /// An encrypted zip entry.
    #[error("has the encrypted entry `{entry}`, and batfiles does not decrypt")]
    Encrypted { entry: String },

    /// A zip entry compressed with a method batfiles does not read.
    #[error(
        "compresses `{entry}` with {method}, and batfiles reads zip entries that are \
         stored, deflated, or compressed with bzip2"
    )]
    UnsupportedMethod { entry: String, method: String },

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
/// pass manifest validation. `filter` decides entries, and `executable` which
/// installed files gain execute permission, by their path with the root
/// stripped. Failure may leave partial content inside `into`.
pub(crate) fn extract(
    archive: &fs::File,
    into: &Path,
    root: Option<&str>,
    filter: Option<&mut EntryFilter>,
    executable: Option<&mut Executable>,
    url: &str,
) -> Result<(), Error> {
    let archive = Archive::identify(archive, url)?;
    let records = plan(&archive, root, filter, executable)?;
    unpack(&archive, &records, into)
}

/// Decompress the gzip or bzip2 body in `file`, the complete and verified download from `url`,
/// into `into`, whose path `built_at` names in diagnostics. A body that is not compressed, or
/// that decompresses to a tar, is refused. Failure may leave partial content in `into`.
pub(crate) fn decompress(
    file: &fs::File,
    into: &mut impl io::Write,
    built_at: &Path,
    url: &str,
) -> Result<(), Error> {
    use std::io::Read as _;

    let fault = |source| Error::Decompress {
        url: url.to_owned(),
        source,
    };
    let unreadable = |source| fault(DecompressError::Unreadable { source });
    let compression =
        compressed::compression_of(&head_of(file).map_err(unreadable)?).map_err(fault)?;

    let mut head = Vec::with_capacity(detect::HEADER_BYTES);
    compression
        .decoder(rewound(file).map_err(unreadable)?)
        .take(detect::HEADER_BYTES as u64)
        .read_to_end(&mut head)
        .map_err(unreadable)?;
    compressed::not_a_tar(&head, compression).map_err(fault)?;

    let written = |source| Error::Write {
        path: built_at.to_path_buf(),
        source,
    };
    let mut decoder = compression.decoder(rewound(file).map_err(unreadable)?);
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let filled = decoder.read(&mut buffer).map_err(unreadable)?;
        if filled == 0 {
            break;
        }
        into.write_all(&buffer[..filled]).map_err(written)?;
    }
    into.flush().map_err(written)
}

/// An open archive, its detected format, and its URL for diagnostics.
struct Archive<'a> {
    /// The complete, verified download. Read through this handle, not its scratch path.
    file: &'a fs::File,
    format: Format,
    /// Carried for the diagnostics alone.
    url: &'a str,
}

impl<'a> Archive<'a> {
    /// Identify the file's archive format, or return an error naming its URL.
    fn identify(file: &'a fs::File, url: &'a str) -> Result<Self, Error> {
        let head = head_of(file).map_err(|source| Error::Archive {
            url: url.to_owned(),
            source: ArchiveError::Unreadable { source },
        })?;
        let format = detect::identify(&head).map_err(|saw| Error::Archive {
            url: url.to_owned(),
            source: ArchiveError::Format { saw },
        })?;
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

/// One entry as a format reader hands it over, in archive order.
struct Entry<'r> {
    /// The path as the archive writes it.
    path: PathBuf,
    kind: EntryKind,
    /// The permission bits the archive records, where it records any.
    mode: Option<u32>,
    /// What a file entry holds.
    content: &'r mut dyn io::Read,
}

/// One entry, as [`plan`] left it.
struct EntryPlan {
    /// Archive path with `.` components removed; metadata headers retain their original paths.
    archive_path: PathBuf,
    /// Where it goes, once the root has been stripped.
    placement: Placement,
    /// What it is, and where a link points.
    kind: EntryKind,
    /// Whether `executable` marks this installed file.
    executable: bool,
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

/// The archive's first block, or as much of it as there is.
fn head_of(file: &fs::File) -> io::Result<Vec<u8>> {
    use std::io::Read as _;

    let mut head = Vec::with_capacity(detect::HEADER_BYTES);
    rewound(file)?
        .take(detect::HEADER_BYTES as u64)
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
/// and applying the entry filters, then mark the installed files `executable` matches.
fn plan(
    archive: &Archive<'_>,
    root: Option<&str>,
    filter: Option<&mut EntryFilter>,
    executable: Option<&mut Executable>,
) -> Result<Vec<EntryPlan>, Error> {
    let mut declared: Vec<(PathBuf, EntryKind)> = Vec::new();
    for_each_entry(archive, &mut |entry| {
        let archive_path = if matches!(entry.kind, EntryKind::Metadata) {
            entry.path
        } else {
            entry_path(&entry.path).ok_or_else(|| archive.escaping(&entry.path))?
        };
        declared.push((archive_path, entry.kind));
        Ok(())
    })?;

    let strip = root_prefix(&declared, root, archive)?;
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
                executable: false,
            }
        })
        .collect();

    if !installs_anything(&records) {
        return Err(archive.fault(match strip {
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
            return Err(archive.fault(ArchiveError::NothingSelected));
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
        check_and_resolve(record, strip.as_deref(), &links, &left_out, archive)?;
    }
    if let Some(executable) = executable {
        for record in &mut records {
            if let (EntryKind::File, Placement::At(path)) = (&record.kind, &record.placement) {
                record.executable = executable.marks(path);
            }
        }
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
    archive: &Archive<'_>,
) -> Result<(), Error> {
    let Some(inside) = record.placement.path() else {
        return Ok(());
    };
    if walks_through_a_link(inside, links) {
        return Err(archive.escaping(&record.archive_path));
    }
    match &mut record.kind {
        EntryKind::File | EntryKind::Directory | EntryKind::Metadata => Ok(()),
        EntryKind::Symlink(target) => {
            if cfg!(not(unix)) {
                return Err(archive.fault(ArchiveError::SymlinkEntry {
                    entry: display(&record.archive_path),
                }));
            }
            let from = inside.parent().unwrap_or_else(|| Path::new(""));
            if stays_inside(&from.join(target.as_path()), links) {
                Ok(())
            } else {
                Err(archive.escaping(&record.archive_path))
            }
        }
        EntryKind::Hardlink(target) => {
            let resolved = entry_path(target)
                .map(|named| place(&named, strip))
                .and_then(|placement| placement.path().map(Path::to_path_buf))
                .filter(|resolved| !walks_through_a_link(resolved, links))
                .ok_or_else(|| archive.escaping(&record.archive_path))?;
            if left_out.contains(&resolved) {
                return Err(archive.fault(ArchiveError::LinkTargetLeftOut {
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
    archive: &Archive<'_>,
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
        0 => Err(archive.fault(ArchiveError::Empty)),
        1 => Ok(tops.into_iter().next().map(PathBuf::from)),
        _ => Err(archive.fault(ArchiveError::AmbiguousRoot {
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
fn unpack(archive: &Archive<'_>, records: &[EntryPlan], into: &Path) -> Result<(), Error> {
    let mut root_mode = UNSTATED_DIRECTORY_MODE;
    let mut directories: Vec<(PathBuf, u32)> = Vec::new();
    let mut planned = records.iter();

    for_each_entry(archive, &mut |entry| {
        let Some(record) = planned.next() else {
            return Err(archive.fault(ArchiveError::Changed));
        };
        let built_at = match &record.placement {
            Placement::At(inside) => into.join(inside),
            Placement::Root => {
                if matches!(record.kind, EntryKind::Directory) {
                    root_mode = mode_of(entry.mode, &record.kind);
                }
                return Ok(());
            }
            Placement::NotInstalled | Placement::LeftOut(_) => return Ok(()),
        };

        match &record.kind {
            EntryKind::Directory => {
                create_directory(&built_at)?;
                directories.push((built_at, mode_of(entry.mode, &record.kind)));
            }
            EntryKind::File => {
                create_parents(&built_at)?;
                let mut file = create_private_file(&built_at)?;
                io::copy(entry.content, &mut file).map_err(|source| Error::Write {
                    path: built_at.clone(),
                    source,
                })?;
                // Keep the file private until its contents are complete.
                set_mode(&built_at, file_mode(entry.mode, record.executable))?;
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
        return Err(archive.fault(ArchiveError::Changed));
    }

    // Apply descendant modes before ancestors that may become unwritable.
    directories.sort_by(|(left, _), (right, _)| right.cmp(left));
    for (path, mode) in directories {
        set_mode(&path, mode)?;
    }
    set_mode(into, root_mode)
}

/// Read the archive from the start, handing each entry to `visit` in archive order.
fn for_each_entry(
    archive: &Archive<'_>,
    visit: &mut dyn FnMut(Entry<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let file = rewound(archive.file).map_err(|source| archive.unreadable(source))?;
    match archive.format {
        Format::Tar(compression) => tar::for_each_entry(archive, compression, file, visit),
        Format::Zip => zip::for_each_entry(archive, file, visit),
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
fn mode_of(recorded: Option<u32>, kind: &EntryKind) -> u32 {
    let unstated = match kind {
        EntryKind::Directory => UNSTATED_DIRECTORY_MODE,
        _ => UNSTATED_FILE_MODE,
    };
    recorded.unwrap_or(unstated) & KEPT_BITS
}

/// The mode a file is given: what it asks for, and execute permission where `executable`
/// marked it.
#[cfg(unix)]
fn file_mode(recorded: Option<u32>, executable: bool) -> u32 {
    let mode = mode_of(recorded, &EntryKind::File);
    if executable {
        mode | EXECUTE_BITS
    } else {
        mode
    }
}

/// Return a placeholder mode on platforms where Unix permissions are not applied.
#[cfg(not(unix))]
fn mode_of(_recorded: Option<u32>, _kind: &EntryKind) -> u32 {
    0
}

#[cfg(not(unix))]
fn file_mode(_recorded: Option<u32>, _executable: bool) -> u32 {
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

/// A path inside an archive, as a diagnostic writes it.
fn display(path: &Path) -> String {
    // An archive spells its paths with `/`, whatever the host's separator.
    path.display()
        .to_string()
        .replace(std::path::MAIN_SEPARATOR, "/")
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
    fn an_archive_path_is_written_with_slashes_on_every_platform() {
        assert_eq!(display(&Path::new("bin").join("core")), "bin/core");
    }
}
