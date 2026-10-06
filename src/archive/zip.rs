//! Zip entries, as the central directory lists them, in its order.

use std::io::{self, Read as _};
use std::path::PathBuf;

use zip::{CompressionMethod, HasZipMetadata as _, ZipArchive};

use super::{Archive, ArchiveError, Entry, EntryKind};
use crate::error::Error;

/// The bits of a Unix mode that give a node's type.
const TYPE_BITS: u32 = 0o170_000;
const DIRECTORY_TYPE: u32 = 0o040_000;
const FILE_TYPE: u32 = 0o100_000;
const SYMLINK_TYPE: u32 = 0o120_000;

/// The bits of a Unix mode that are not its type.
const PERMISSION_BITS: u32 = 0o7777;

/// The longest symlink target a link entry's contents may hold.
const MAX_LINK_TARGET: u64 = 4096;

/// Read the zip in `reader` through its central directory, handing each entry to `visit`.
pub(super) fn for_each_entry(
    archive: &Archive<'_>,
    reader: impl io::Read + io::Seek,
    visit: &mut dyn FnMut(Entry<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let unreadable = |error: zip::result::ZipError| archive.unreadable(error.into());
    let mut zip = ZipArchive::new(reader).map_err(unreadable)?;
    for index in 0..zip.len() {
        let (path, recorded) = {
            let raw = zip.by_index_raw(index).map_err(unreadable)?;
            let data = raw.get_metadata();
            let listed = Listed {
                name_raw: &data.file_name_raw,
                name: &data.file_name,
                is_utf8: data.is_utf8,
                external_attributes: data.external_attributes,
                encrypted: data.encrypted,
                method: data.compression_method,
            };
            let path = path_of(&listed).map_err(|invalid| archive.fault(invalid))?;
            readable(&listed, &path).map_err(|invalid| archive.fault(invalid))?;
            (path, recorded_mode(&listed))
        };
        let mut entry = zip.by_index(index).map_err(unreadable)?;
        let kind = match kind_of(&path, recorded) {
            Some(Kind::Directory) => EntryKind::Directory,
            Some(Kind::File) => EntryKind::File,
            Some(Kind::Symlink) => EntryKind::Symlink(link_target(&mut entry, archive)?),
            None => {
                return Err(archive.fault(ArchiveError::UnsupportedEntry { entry: path }));
            }
        };
        visit(Entry {
            path: PathBuf::from(path),
            kind,
            mode: recorded.map(|mode| mode & PERMISSION_BITS),
            content: &mut entry,
        })?;
    }
    Ok(())
}

/// What the central directory says of one entry.
struct Listed<'a> {
    /// The name's bytes, as written or as a Unicode path field gave them.
    name_raw: &'a [u8],
    /// The name as the `zip` crate decoded it.
    name: &'a str,
    /// Whether the name is flagged, or was given, as UTF-8.
    is_utf8: bool,
    external_attributes: u32,
    encrypted: bool,
    method: CompressionMethod,
}

/// What a zip entry is, before a link's target is read.
#[derive(Debug, PartialEq, Eq)]
enum Kind {
    Directory,
    File,
    Symlink,
}

/// The kind of the entry named `path` with the recorded Unix mode, or `None` for a device,
/// FIFO, or socket. A name ending in `/` is a directory whatever its mode says.
fn kind_of(path: &str, recorded: Option<u32>) -> Option<Kind> {
    if path.ends_with('/') {
        return Some(Kind::Directory);
    }
    match recorded.map(|mode| mode & TYPE_BITS) {
        None | Some(0 | FILE_TYPE) => Some(Kind::File),
        Some(DIRECTORY_TYPE) => Some(Kind::Directory),
        Some(SYMLINK_TYPE) => Some(Kind::Symlink),
        Some(_) => None,
    }
}

/// The Unix mode an entry records in the high half of its external attributes, where it
/// records one. A zip made on Windows usually records none.
fn recorded_mode(listed: &Listed<'_>) -> Option<u32> {
    let mode = listed.external_attributes >> 16;
    (mode != 0).then_some(mode)
}

/// The entry's name: UTF-8 where it is flagged as UTF-8 or its bytes are, and otherwise
/// code page 437, the zip format's original encoding. A name holding `\` or a NUL byte, or
/// flagged as UTF-8 and not, is refused.
fn path_of(listed: &Listed<'_>) -> Result<String, ArchiveError> {
    let name = match std::str::from_utf8(listed.name_raw) {
        Ok(name) => name.to_owned(),
        Err(_) if !listed.is_utf8 => listed.name.to_owned(),
        Err(_) => {
            return Err(ArchiveError::UnusableName {
                entry: listed.name.to_owned(),
            });
        }
    };
    if name.contains(['\\', '\0']) {
        return Err(ArchiveError::UnusableName {
            entry: name.escape_debug().to_string(),
        });
    }
    Ok(name)
}

/// Refuse an entry whose contents batfiles cannot read: an encrypted one, or one compressed
/// with a method other than deflate or bzip2.
fn readable(listed: &Listed<'_>, path: &str) -> Result<(), ArchiveError> {
    if listed.encrypted {
        return Err(ArchiveError::Encrypted {
            entry: path.to_owned(),
        });
    }
    match listed.method {
        CompressionMethod::Stored | CompressionMethod::Deflated | CompressionMethod::Bzip2 => {
            Ok(())
        }
        other => Err(ArchiveError::UnsupportedMethod {
            entry: path.to_owned(),
            method: match other.to_string().as_str() {
                "Unknown" => "a method batfiles does not recognize".to_owned(),
                named => named.to_owned(),
            },
        }),
    }
}

/// The target a symlink entry holds as its contents.
fn link_target(entry: &mut impl io::Read, archive: &Archive<'_>) -> Result<PathBuf, Error> {
    let mut target = Vec::new();
    entry
        .take(MAX_LINK_TARGET + 1)
        .read_to_end(&mut target)
        .map_err(|source| archive.unreadable(source))?;
    if target.len() as u64 > MAX_LINK_TARGET {
        return Err(archive.unreadable(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("a symlink target is longer than {MAX_LINK_TARGET} bytes"),
        )));
    }
    Ok(path_from_bytes(target))
}

/// A link target's bytes as a Unix path: any bytes, as written.
#[cfg(unix)]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    use std::os::unix::ffi::OsStringExt as _;

    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

/// A link target's bytes elsewhere, where symlink entries are refused before they are made.
#[cfg(not(unix))]
fn path_from_bytes(bytes: Vec<u8>) -> PathBuf {
    PathBuf::from(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed<'a>(name_raw: &'a [u8], name: &'a str, is_utf8: bool) -> Listed<'a> {
        Listed {
            name_raw,
            name,
            is_utf8,
            external_attributes: 0,
            encrypted: false,
            method: CompressionMethod::Stored,
        }
    }

    #[test]
    fn a_name_is_utf8_where_its_bytes_are_and_code_page_437_where_they_are_not() {
        let utf8 = "caf\u{e9}/menu";
        assert_eq!(
            path_of(&listed(utf8.as_bytes(), "caf\u{251c}\u{2310}/menu", false)).ok(),
            Some(utf8.to_owned())
        );
        // 0x82 is `é` in code page 437, and no UTF-8 sequence.
        assert_eq!(
            path_of(&listed(b"caf\x82", "caf\u{e9}", false)).ok(),
            Some("caf\u{e9}".to_owned())
        );
        assert!(path_of(&listed(b"caf\x82", "caf\u{fffd}", true)).is_err());
    }

    #[test]
    fn a_name_with_a_backslash_or_a_nul_is_refused() {
        for refused in ["bin\\tool", "bin/to\0ol"] {
            assert!(
                path_of(&listed(refused.as_bytes(), refused, true)).is_err(),
                "{refused:?}"
            );
        }
    }

    #[test]
    fn a_mode_is_read_only_where_the_archive_records_one() {
        let attributes = |external_attributes| Listed {
            external_attributes,
            ..listed(b"a", "a", true)
        };
        assert_eq!(
            recorded_mode(&attributes((FILE_TYPE | 0o755) << 16)),
            Some(FILE_TYPE | 0o755)
        );
        // MS-DOS attributes alone, as a zip made on Windows writes: archive and read-only.
        assert_eq!(recorded_mode(&attributes(0x21)), None);
    }

    #[test]
    fn an_entry_is_what_its_name_and_its_mode_say() {
        assert_eq!(
            kind_of("bin/", Some(FILE_TYPE | 0o644)),
            Some(Kind::Directory)
        );
        assert_eq!(
            kind_of("bin", Some(DIRECTORY_TYPE | 0o755)),
            Some(Kind::Directory)
        );
        assert_eq!(kind_of("bin/tool", None), Some(Kind::File));
        assert_eq!(kind_of("bin/tool", Some(0o755)), Some(Kind::File));
        assert_eq!(
            kind_of("bin/tool", Some(SYMLINK_TYPE | 0o777)),
            Some(Kind::Symlink)
        );
        // A FIFO.
        assert_eq!(kind_of("pipe", Some(0o010_644)), None);
    }
}
