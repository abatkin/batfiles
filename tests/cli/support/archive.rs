//! Tar and gzip fixtures, including malformed entries.

use std::io::Write as _;

// Build archives at runtime to test absolute paths and parent traversal.

/// One entry of an archive a test builds.
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
    tar_bytes(members, Headers::Gnu, |bytes| gzip(&bytes))
}

/// Build an uncompressed GNU tar archive from the supplied members.
pub(crate) fn plain_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| bytes)
}

/// Build an uncompressed V7 tar archive without `ustar` header magic.
pub(crate) fn v7_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::V7, |bytes| bytes)
}

/// Which header format an archive is built with.
#[derive(Clone, Copy)]
enum Headers {
    Gnu,
    V7,
}

/// Build a tar archive compressed as two concatenated gzip members.
pub(crate) fn multi_member_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| {
        let (first, second) = bytes.split_at(bytes.len() / 2);
        let mut stream = gzip(first);
        stream.extend(gzip(second));
        stream
    })
}

fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(bytes).expect("a gzip stream");
    encoder.finish().expect("a finished gzip stream")
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
