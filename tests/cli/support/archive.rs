//! Tar and gzip fixtures, including malformed entries.

use std::io::Write as _;

// Archives the fetching tests serve. Built here rather than committed, because
// most of what `fetch-archive` has to refuse cannot be committed: an entry
// spelled `../../.ssh/authorized_keys` is not a file a checkout can hold, and
// one spelled `/etc/passwd` is not either.

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
    /// Something batfiles installs none of: a fifo, here by its tar type.
    Fifo(&'static str),
}

/// A gzipped tar holding exactly these members, in this order.
pub(crate) fn tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| gzip(&bytes))
}

/// The same archive, uncompressed, for the half of the format sniffing that is
/// about a plain `.tar`.
pub(crate) fn plain_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::Gnu, |bytes| bytes)
}

/// The same archive in the original V7 format, whose headers carry no `ustar`
/// magic at all — the shape a detector that looks for that magic refuses and a
/// tar reader accepts.
pub(crate) fn v7_tarball(members: &[Member]) -> Vec<u8> {
    tar_bytes(members, Headers::V7, |bytes| bytes)
}

/// Which header format an archive is built with.
#[derive(Clone, Copy)]
enum Headers {
    Gnu,
    V7,
}

/// The same archive, gzipped as two members concatenated, which is what `pigz`
/// writes and what `cat a.gz b.gz` produces.
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
        // A `new_gnu` header is all zero bytes, and a zeroed numeric field is
        // not a number to a reader. Every entry gets the ones that are not
        // about what it is; only the size varies.
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
        // The name goes into the header's bytes directly, and the entry is
        // appended rather than built: `Builder::append_data` refuses a path
        // holding `..` or starting at a root, which is exactly what half of
        // these archives are for.
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

/// Put a name into a tar header without asking the crate's opinion of it.
fn write_name(header: &mut tar::Header, path: &str) {
    let name = &mut header.as_old_mut().name;
    assert!(
        path.len() < name.len(),
        "`{path}` is too long for a tar header's name field"
    );
    name.fill(0);
    name[..path.len()].copy_from_slice(path.as_bytes());
}
