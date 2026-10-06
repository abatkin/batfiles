//! Tar, gzip, bzip2, and zip fixtures, including malformed entries.

use std::io::Write as _;

// Build archives at runtime to test absolute paths and parent traversal.

/// One entry of an archive a test builds.
#[derive(Clone)]
pub(crate) enum Member {
    /// A file, its mode, and what it holds.
    File(&'static str, u32, &'static str),
    /// A directory entry, with the mode the archive asks for.
    Directory(&'static str, u32),
    /// A symlink, and the target exactly as the archive writes it.
    Symlink(&'static str, &'static str),
    /// A hardlink, and the archive path it points at.
    Hardlink(&'static str, &'static str),
    /// A FIFO entry for unsupported-entry tests.
    Fifo(&'static str),
}

/// A gzipped tar holding exactly these members, in this order.
pub(crate) fn tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| gzipped(&bytes))
}

/// Build an uncompressed GNU tar archive from the supplied members.
pub(crate) fn plain_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| bytes)
}

/// Build an uncompressed V7 tar archive without `ustar` header magic.
pub(crate) fn v7_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::V7, |bytes| bytes)
}

/// A bzip2-compressed tar holding exactly these members, in this order.
pub(crate) fn bzipped_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| bzipped(&bytes))
}

/// Which header format an archive is built with.
#[derive(Clone, Copy)]
enum Headers {
    Gnu,
    V7,
}

/// A compressor whose streams may be concatenated.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Stream {
    Gzip,
    Bzip2,
}

/// Build a tar archive compressed as two concatenated streams.
pub(crate) fn multi_member_tarball(members: &[Member], stream: Stream) -> Vec<u8> {
    let compress = match stream {
        Stream::Gzip => gzipped,
        Stream::Bzip2 => bzipped,
    };
    tar_bytes(members, Headers::Gnu, |bytes| {
        let (first, second) = bytes.split_at(bytes.len() / 2);
        let mut compressed = compress(first);
        compressed.extend(compress(second));
        compressed
    })
}

/// `bytes` as one gzip stream.
pub(crate) fn gzipped(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("a gzip stream");
    encoder.finish().expect("a finished gzip stream")
}

/// `bytes` as one bzip2 stream.
pub(crate) fn bzipped(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
    encoder.write_all(bytes).expect("a bzip2 stream");
    encoder.finish().expect("a finished bzip2 stream")
}

fn tar_bytes(
    members: &[Member],
    headers: Headers,
    wrap: impl FnOnce(Vec<u8>) -> Vec<u8>,
) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for member in members {
        let mut header = match headers {
            Headers::Gnu => tar::Header::new_gnu(),
            Headers::V7 => tar::Header::new_old(),
        };
        // Initialize numeric fields; zero bytes are not valid numeric field values.
        header.set_size(0);
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        let body = match member {
            Member::File(_, mode, body) => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(*mode);
                header.set_size(body.len() as u64);
                Some(*body)
            }
            Member::Directory(_, mode) => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(*mode);
                None
            }
            Member::Symlink(_, target) | Member::Hardlink(_, target) => {
                header.set_entry_type(match member {
                    Member::Symlink(..) => tar::EntryType::Symlink,
                    _ => tar::EntryType::Link,
                });
                header.set_mode(0o777);
                header
                    .set_link_name(target)
                    .expect("a link target the header can hold");
                None
            }
            Member::Fifo(_) => {
                header.set_entry_type(tar::EntryType::Fifo);
                header.set_mode(0o644);
                None
            }
        };
        // Write the path directly because `append_data` rejects absolute paths and parent
        // traversal.
        write_name(&mut header, member.path());
        header.set_cksum();
        builder
            .append(&header, body.unwrap_or_default().as_bytes())
            .expect("a tar entry");
    }
    wrap(builder.into_inner().expect("a finished tar"))
}

impl Member {
    fn path(&self) -> &'static str {
        match self {
            Self::File(path, ..)
            | Self::Directory(path, _)
            | Self::Symlink(path, _)
            | Self::Hardlink(path, _)
            | Self::Fifo(path) => path,
        }
    }
}

/// Write a tar-header path directly, bypassing path validation for malformed fixtures.
fn write_name(header: &mut tar::Header, path: &str) {
    let name = &mut header.as_old_mut().name;
    assert!(
        path.len() < name.len(),
        "`{path}` is too long for a tar header's name field"
    );
    name.fill(0);
    name[..path.len()].copy_from_slice(path.as_bytes());
}

/// A zip holding exactly these members, in this order, each compressed with `method` and
/// recording the Unix mode its member gives, as a zip made on Unix does.
pub(crate) fn zip_archive(members: &[Member], method: zip::CompressionMethod) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(method)
        .system(zip::System::Unix);
    for member in members {
        match member {
            Member::File(path, mode, body) => {
                writer
                    .start_file(*path, options.unix_permissions(*mode))
                    .expect("a zip entry");
                writer
                    .write_all(body.as_bytes())
                    .expect("a zip entry's body");
            }
            Member::Directory(path, mode) => writer
                .add_directory(*path, options.unix_permissions(*mode))
                .expect("a zip directory"),
            Member::Symlink(path, target) => writer
                .add_symlink(*path, *target, options)
                .expect("a zip symlink"),
            Member::Hardlink(..) | Member::Fifo(_) => panic!("a zip has no {}", member.path()),
        }
    }
    writer.finish().expect("a finished zip").into_inner()
}

/// The zip as one made on Windows writes it: MS-DOS attributes, and no Unix modes.
pub(crate) fn as_if_made_on_windows(zip: Vec<u8>) -> Vec<u8> {
    patch_central_directory(zip, |name, header| {
        // The host system: 0 is MS-DOS.
        header[5] = 0;
        // The directory attribute or the archive attribute, and nothing above them.
        let attributes: u32 = if name.ends_with('/') { 0x10 } else { 0x20 };
        header[38..42].copy_from_slice(&attributes.to_le_bytes());
    })
}

/// The zip with the entry `named` flagged as encrypted.
pub(crate) fn with_entry_encrypted(zip: Vec<u8>, named: &str) -> Vec<u8> {
    patch_central_directory(zip, |name, header| {
        if name == named {
            header[8] |= 1;
        }
    })
}

/// The zip with the entry `named` declaring compression `method`.
pub(crate) fn with_entry_method(zip: Vec<u8>, named: &str, method: u16) -> Vec<u8> {
    patch_central_directory(zip, |name, header| {
        if name == named {
            header[10..12].copy_from_slice(&method.to_le_bytes());
        }
    })
}

/// Hand each central directory header to `patch` with its entry's name. A header is the
/// fixed 46 bytes before the name.
fn patch_central_directory(mut zip: Vec<u8>, patch: impl Fn(&str, &mut [u8])) -> Vec<u8> {
    let field = |zip: &[u8], at: usize, width: usize| {
        zip[at..at + width]
            .iter()
            .rev()
            .fold(0usize, |value, byte| value << 8 | usize::from(*byte))
    };
    let end = zip
        .windows(4)
        .rposition(|window| window == b"PK\x05\x06")
        .expect("the end of a central directory");
    let count = field(&zip, end + 10, 2);
    let mut at = field(&zip, end + 16, 4);
    for _ in 0..count {
        assert_eq!(
            &zip[at..at + 4],
            b"PK\x01\x02",
            "a central directory header"
        );
        let name_length = field(&zip, at + 28, 2);
        let next = at + 46 + name_length + field(&zip, at + 30, 2) + field(&zip, at + 32, 2);
        let name =
            String::from_utf8(zip[at + 46..at + 46 + name_length].to_vec()).expect("a UTF-8 name");
        patch(&name, &mut zip[at..at + 46]);
        at = next;
    }
    zip
}
